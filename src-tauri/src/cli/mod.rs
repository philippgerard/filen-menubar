//! CLI module for managing the Filen CLI subprocess
//!
//! This module handles:
//! - Finding the Filen CLI binary on the system (`discovery`)
//! - Parsing JSON events from the CLI's verbose output (`events`)
//! - Detecting network errors for offline status (`network`)
//! - Managing the CLI subprocess lifecycle (`CliManager`)
//!
//! ## Architecture
//!
//! ```text
//! CliManager
//!     ├── start_sync() -> spawns CLI process with --verbose
//!     ├── stop_sync() -> kills CLI process
//!     └── monitors stdout/stderr for JSON events
//!          └── handle_cli_event() updates AppState
//! ```
//!
//! ## Event Flow
//!
//! 1. CLI emits JSON events on stdout in `--verbose` mode
//! 2. Events are parsed into `CliEvent` variants
//! 3. `handle_cli_event()` processes events and updates `AppState`
//! 4. State changes propagate to the tray UI

mod discovery;
mod events;
pub mod network;

pub use discovery::find_filen_cli;
pub use events::{CliErrorEvent, CliEvent};

use crate::config::Config;
use crate::error::CliError;
use crate::state::{AppState, CurrentTransfer, StorageInfo, SyncState, TransferDirection};
use network::is_network_error;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// Messages sent from CLI process monitor
#[allow(dead_code)]
#[derive(Debug)]
pub enum CliMessage {
    StateChanged(SyncState),
    StorageUpdated(StorageInfo),
    Error(String),
}

/// Handle a parsed CLI event and update app state accordingly
async fn handle_cli_event(state: &AppState, event: CliEvent) {
    match event {
        CliEvent::CycleStarted => {
            // Don't set syncing on cycleStarted - cycles run frequently even when idle
        }
        CliEvent::CycleGettingTreesStarted => {
            // Scanning local and remote file trees - show scanning status
            let current = state.get_sync_state().await;
            if current != SyncState::Scanning && current != SyncState::Syncing {
                log::info!("Scanning file trees");
                state.set_sync_state(SyncState::Scanning).await;
            }
        }
        CliEvent::CycleGettingTreesDone => {
            // Tree scanning complete - will transition to syncing if there are deltas
            log::debug!("File tree scan complete");
        }
        CliEvent::CycleProcessingTasksStarted => {
            if state.get_sync_state().await != SyncState::Syncing {
                log::info!("Processing tasks started");
                state.set_sync_state(SyncState::Syncing).await;
            }
        }
        CliEvent::CycleSuccess => {
            log::info!("Sync cycle completed");
            state.set_sync_state(SyncState::Synced).await;
            state.set_pending_count(0).await;
            state.set_current_transfer(None).await;
        }
        CliEvent::CycleError { error } => {
            let error_msg = error.as_deref().unwrap_or("");
            if is_network_error(error_msg) {
                log::warn!("Network error detected: {:?}", error);
                state.set_sync_state(SyncState::Offline).await;
            } else {
                log::error!("Sync cycle error: {:?}", error);
                state.set_sync_state(SyncState::Error).await;
            }
            state.set_pending_count(0).await;
            state.set_current_transfer(None).await;
        }
        CliEvent::DeltasCount { data } => {
            state.set_pending_count(data.count).await;
            if data.count > 0 {
                log::info!("Syncing {} files", data.count);
                state.set_sync_state(SyncState::Syncing).await;
            }
        }
        CliEvent::Transfer { data } => {
            if let Some(ref transfer_data) = data {
                // Determine direction for all transfer types
                let direction = match transfer_data.operation.as_deref() {
                    Some("upload") | Some("uploadFile") => Some(TransferDirection::Upload),
                    Some("download") | Some("downloadFile") => Some(TransferDirection::Download),
                    _ => None, // createRemoteDirectory, etc. don't show indicator
                };

                // Check if this transfer completed successfully
                if transfer_data.transfer_type.as_deref() == Some("success")
                    || transfer_data.transfer_type.as_deref() == Some("finished")
                {
                    let current = state.get_pending_count().await;
                    if current > 0 {
                        let new_count = current - 1;
                        log::debug!("Transfer complete, {} files remaining", new_count);
                        state.set_pending_count(new_count).await;
                    }
                    // Clear current transfer when this file is done
                    state.set_current_transfer(None).await;
                } else if transfer_data.transfer_type.as_deref() == Some("started")
                    || transfer_data.transfer_type.as_deref() == Some("progress")
                    || transfer_data.transfer_type.as_deref() == Some("queued")
                {
                    // Update current transfer info (only for actual file transfers)
                    if let (Some(dir), Some(path)) = (direction, &transfer_data.relative_path) {
                        // Extract filename from path
                        let filename = std::path::Path::new(path)
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.clone());

                        let size = transfer_data.size.unwrap_or(0);
                        let bytes = transfer_data.bytes.unwrap_or(0);

                        let mut transfer = CurrentTransfer::new(dir, filename, size);
                        transfer.bytes = bytes;

                        log::debug!(
                            "Transfer progress: {:?} {}% ({}/{})",
                            dir,
                            transfer.progress_percent(),
                            bytes,
                            size
                        );
                        state.set_current_transfer(Some(transfer)).await;
                    }
                } else if transfer_data.transfer_type.as_deref() == Some("error") {
                    // Clear current transfer on error
                    state.set_current_transfer(None).await;
                }
            }
            // Ensure we're in syncing state while transfers are happening
            if state.get_sync_state().await != SyncState::Syncing {
                log::info!("File transfer in progress");
                state.set_sync_state(SyncState::Syncing).await;
            }
        }
        CliEvent::UploadProgress { .. } | CliEvent::DownloadProgress { .. } => {
            if state.get_sync_state().await != SyncState::Syncing {
                log::info!("File transfer in progress");
                state.set_sync_state(SyncState::Syncing).await;
            }
        }
        CliEvent::Success { .. } => {
            // Note: Success events are typically embedded in Transfer events as data.type="success"
            // This handles any standalone success events
            let current = state.get_pending_count().await;
            if current > 0 {
                let new_count = current - 1;
                log::debug!(
                    "Transfer complete (standalone), {} files remaining",
                    new_count
                );
                state.set_pending_count(new_count).await;
            }
        }
        CliEvent::Unknown => {
            // Ignore unknown event types
        }
    }
}

/// Handle non-JSON text output from CLI (fallback for text mode)
async fn handle_text_output(state: &AppState, line: &str) {
    if line.starts_with("Done syncing") {
        if state.get_sync_state().await != SyncState::Synced {
            log::info!("Sync completed (text)");
            state.set_sync_state(SyncState::Synced).await;
            state.set_pending_count(0).await;
        }
    } else if line.starts_with("Syncing ") && !line.contains('{') {
        let current = state.get_sync_state().await;
        // Don't override Scanning or Syncing states
        if current != SyncState::Syncing && current != SyncState::Scanning {
            state.set_sync_state(SyncState::Syncing).await;
        }
    }
}

/// Manages the Filen CLI process
pub struct CliManager {
    process: Arc<RwLock<Option<Child>>>,
    state: AppState,
    shutdown_tx: Arc<RwLock<Option<mpsc::Sender<()>>>>,
}

impl CliManager {
    pub fn new(state: AppState) -> Self {
        Self {
            process: Arc::new(RwLock::new(None)),
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
        }
    }

    /// Check if filen CLI is installed (single attempt)
    async fn check_cli_once() -> bool {
        // Run filesystem search in blocking context to avoid blocking async runtime
        let cli_info = match tokio::task::spawn_blocking(find_filen_cli).await {
            Ok(info) => info,
            Err(e) => {
                log::error!("Failed to search for filen CLI: {}", e);
                return false;
            }
        };

        log::info!("Checking filen CLI availability at: {}", cli_info.command);

        let mut cmd = Command::new(&cli_info.command);
        cmd.arg("--version")
            .stdin(Stdio::null()) // Prevent hanging on stdin when running from autostart
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Set PATH if we found a specific installation (needed for node-based CLI)
        if let Some(ref path_env) = cli_info.path_env {
            log::debug!("Using PATH: {}", path_env);
            cmd.env("PATH", path_env);
        }

        // Use a timeout to avoid hanging if the CLI is stuck
        match timeout(Duration::from_secs(5), cmd.status()).await {
            Ok(Ok(status)) => {
                let available = status.success();
                log::info!("Filen CLI available: {}", available);
                available
            }
            Ok(Err(e)) => {
                log::warn!("Failed to run filen CLI: {}", e);
                false
            }
            Err(_) => {
                log::warn!("Timeout checking filen CLI availability");
                false
            }
        }
    }

    /// Check if filen CLI is installed, with retries for macOS Login Item boot timing.
    ///
    /// When launched as a Login Item at macOS boot, the app may start before the
    /// filesystem (especially version manager directories like fnm/nvm) is fully ready.
    /// This function retries with exponential backoff to handle this race condition.
    pub async fn is_cli_available() -> bool {
        // Retry delays: 0s (immediate), 2s, 4s, 8s
        let retry_delays = [0, 2, 4, 8];

        for (attempt, delay_secs) in retry_delays.iter().enumerate() {
            if *delay_secs > 0 {
                log::info!(
                    "CLI not found, retrying in {}s (attempt {}/{})",
                    delay_secs,
                    attempt + 1,
                    retry_delays.len()
                );
                tokio::time::sleep(Duration::from_secs(*delay_secs)).await;
            }

            if Self::check_cli_once().await {
                if attempt > 0 {
                    log::info!("CLI found after {} retries", attempt);
                }
                return true;
            }
        }

        log::error!(
            "Filen CLI not found after {} attempts. Please install it with: npm install -g @filen/cli",
            retry_delays.len()
        );
        false
    }

    /// Start the sync process (uses CLI's stored session)
    pub async fn start_sync(&self, config: &Config) -> Result<(), CliError> {
        // Stop any existing process
        self.stop_sync().await;

        // Generate syncPairs.json with ignore patterns
        let sync_pairs_path = config.write_sync_pairs().map_err(|e| {
            log::error!("Failed to write sync pairs: {}", e);
            CliError::SyncPairs(e.to_string())
        })?;

        log::info!("Generated syncPairs.json at: {:?}", sync_pairs_path);
        log::info!(
            "Sync config: local={}, remote={}, mode={}, ignore={:?}, excludeDotFiles={}",
            config.local_path.display(),
            config.remote_path,
            config.sync_mode,
            config.ignore,
            config.exclude_dot_files
        );

        // Don't pass credentials - CLI will use its stored session
        // Use --verbose to get detailed file sync information
        let cli_info = find_filen_cli();
        log::info!("Using filen CLI at: {}", cli_info.command);
        if let Some(ref path_env) = cli_info.path_env {
            log::info!("Setting PATH for CLI: {}", path_env);
        }

        let mut cmd = Command::new(&cli_info.command);
        cmd.arg("--verbose")
            .arg("sync")
            .arg(&sync_pairs_path)
            .arg("--continuous")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Set PATH if we found a specific installation (needed for node-based CLI)
        if let Some(ref path_env) = cli_info.path_env {
            cmd.env("PATH", path_env);
        }

        let mut child = cmd.spawn()?;

        // Get stdout and stderr
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Store the process
        *self.process.write().await = Some(child);

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        // Note: Initial state is already set by caller (lib.rs) to Scanning
        // CLI events will update to Syncing when transfers begin, or Synced when done

        // Spawn output monitoring task
        let state = self.state.clone();
        tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                // Buffer for accumulating multi-line JSON objects
                // CLI outputs pretty-printed JSON spanning multiple lines
                let mut json_buffer = String::new();
                let mut brace_depth = 0;

                loop {
                    // Check for shutdown signal (non-blocking)
                    if shutdown_rx.try_recv().is_ok() {
                        log::info!("Sync monitor received shutdown signal");
                        break;
                    }

                    // Try to read a line with a short timeout
                    match timeout(Duration::from_secs(1), lines.next_line()).await {
                        Ok(Ok(Some(line))) => {
                            log::debug!("CLI stdout: {}", line);

                            // Count braces to detect complete JSON objects
                            for ch in line.chars() {
                                match ch {
                                    '{' => brace_depth += 1,
                                    '}' => brace_depth -= 1,
                                    _ => {}
                                }
                            }

                            // Accumulate lines into buffer
                            json_buffer.push_str(&line);
                            json_buffer.push('\n');

                            // When brace depth returns to 0, we have a complete JSON object
                            if brace_depth == 0 && !json_buffer.trim().is_empty() {
                                let complete_json = json_buffer.trim();

                                // Try to parse as JSON event
                                if complete_json.starts_with('{') {
                                    match serde_json::from_str::<CliEvent>(complete_json) {
                                        Ok(event) => {
                                            handle_cli_event(&state, event).await;
                                        }
                                        Err(e) => {
                                            log::debug!(
                                                "Failed to parse JSON event: {} - {}",
                                                e,
                                                &complete_json[..complete_json.len().min(100)]
                                            );
                                        }
                                    }
                                } else {
                                    // Non-JSON text output
                                    handle_text_output(&state, complete_json).await;
                                }

                                json_buffer.clear();
                            }
                        }
                        Ok(Ok(None)) => {
                            // EOF - process exited
                            log::warn!("CLI process stdout closed");
                            // Give stderr handler time to process network errors
                            // (stderr and stdout handlers run concurrently)
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            // Preserve Offline state if already set by stderr handler
                            // (network errors often cause CLI to crash)
                            let current_state = state.get_sync_state().await;
                            if current_state != SyncState::Offline {
                                state.set_sync_state(SyncState::Error).await;
                            }
                            break;
                        }
                        Ok(Err(e)) => {
                            log::error!("Error reading CLI stdout: {}", e);
                            // Give stderr handler time to process network errors
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            // Preserve Offline state if already set by stderr handler
                            let current_state = state.get_sync_state().await;
                            if current_state != SyncState::Offline {
                                state.set_sync_state(SyncState::Error).await;
                            }
                            break;
                        }
                        Err(_) => {
                            // Timeout - no output, that's fine - just continue
                        }
                    }
                }
            }
        });

        // Spawn stderr monitoring task
        let state_for_stderr = self.state.clone();
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                // Track if we've detected a network error in this stderr stream
                // Once detected, we shouldn't downgrade to Error
                let mut network_error_detected = false;

                while let Ok(Some(line)) = lines.next_line().await {
                    log::warn!("CLI stderr: {}", line);

                    // Try to parse as JSON error event
                    if let Ok(err_event) = serde_json::from_str::<CliErrorEvent>(&line) {
                        if err_event.event_type.as_deref() == Some("error") {
                            let msg = err_event.error.or(err_event.message).unwrap_or_default();
                            if is_network_error(&msg) {
                                log::warn!("Network error from stderr: {}", msg);
                                state_for_stderr.set_sync_state(SyncState::Offline).await;
                                network_error_detected = true;
                            } else if !network_error_detected {
                                log::error!("CLI error: {}", msg);
                                state_for_stderr.set_sync_state(SyncState::Error).await;
                            }
                        }
                    } else if is_network_error(&line) {
                        // Text-based network error detection
                        log::warn!("Network error detected in stderr: {}", line);
                        state_for_stderr.set_sync_state(SyncState::Offline).await;
                        network_error_detected = true;
                    } else if !network_error_detected
                        && (line.to_lowercase().contains("error") || line.contains("failed"))
                    {
                        // Fallback text detection for non-JSON errors
                        // Only set Error if we haven't detected a network error
                        state_for_stderr.set_sync_state(SyncState::Error).await;
                    }
                }
            });
        }

        Ok(())
    }

    /// Stop the sync process
    pub async fn stop_sync(&self) {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(()).await;
        }

        // Kill the process - only set Paused if there was actually a process running
        if let Some(mut child) = self.process.write().await.take() {
            log::info!("Stopping sync process");
            let _ = child.kill().await;
            self.state.set_sync_state(SyncState::Paused).await;
        }
    }

    /// Check if sync is running
    #[allow(dead_code)]
    pub async fn is_running(&self) -> bool {
        self.process.read().await.is_some()
    }

    /// Query storage quota from CLI (uses CLI's stored session)
    /// NOTE: The Filen CLI v0.0.39 doesn't have a storage quota command
    /// This is a placeholder that always returns default values
    #[allow(dead_code)]
    pub async fn query_storage(&self) -> Result<StorageInfo, CliError> {
        // The Filen CLI doesn't currently expose a storage quota command
        // Return default values for now
        Ok(StorageInfo::default())
    }

    /// Trigger a manual sync (one-shot, uses CLI's stored session)
    #[allow(dead_code)]
    pub async fn sync_once(&self, config: &Config) -> Result<(), CliError> {
        // Generate syncPairs.json with ignore patterns
        let sync_pairs_path = config.write_sync_pairs().map_err(|e| {
            log::error!("Failed to write sync pairs: {}", e);
            CliError::SyncPairs(e.to_string())
        })?;

        log::info!("Running one-shot sync with config: {:?}", sync_pairs_path);
        self.state.set_sync_state(SyncState::Syncing).await;

        let cli_info = find_filen_cli();
        let mut cmd = Command::new(&cli_info.command);
        cmd.arg("sync").arg(&sync_pairs_path);

        // Set PATH if we found a specific installation (needed for node-based CLI)
        if let Some(ref path_env) = cli_info.path_env {
            cmd.env("PATH", path_env);
        }

        let output = cmd.output().await?;

        if output.status.success() {
            log::info!("One-shot sync completed successfully");
            self.state.set_sync_state(SyncState::Synced).await;
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("One-shot sync failed: {}", stderr);
            self.state.set_sync_state(SyncState::Error).await;
        }

        Ok(())
    }
}
