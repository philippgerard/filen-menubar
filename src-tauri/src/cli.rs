use crate::config::Config;
use crate::state::{AppState, CurrentTransfer, StorageInfo, SyncState, TransferDirection};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// Nested data for deltasCount event
#[derive(Debug, Deserialize)]
struct DeltasCountData {
    count: u32,
}

/// Nested data for transfer event
#[derive(Debug, Deserialize)]
struct TransferData {
    /// The operation type: "upload", "download", "createRemoteDirectory", etc.
    #[serde(rename = "of")]
    operation: Option<String>,
    /// The status: "queued", "started", "progress", "finished", "success", "error"
    #[serde(rename = "type")]
    transfer_type: Option<String>,
    /// The relative path of the file
    #[serde(rename = "relativePath")]
    relative_path: Option<String>,
    /// Bytes transferred so far (for progress events)
    bytes: Option<u64>,
    /// Total file size in bytes
    size: Option<u64>,
}

/// CLI event types emitted in --verbose mode
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CliEvent {
    #[serde(rename = "cycleStarted")]
    CycleStarted,

    #[serde(rename = "cycleProcessingTasksStarted")]
    CycleProcessingTasksStarted,

    #[serde(rename = "cycleSuccess")]
    CycleSuccess,

    #[serde(rename = "cycleError")]
    CycleError {
        #[allow(dead_code)]
        error: Option<String>,
    },

    #[serde(rename = "deltasCount")]
    DeltasCount { data: DeltasCountData },

    #[serde(rename = "transfer")]
    Transfer { data: Option<TransferData> },

    #[serde(rename = "success")]
    Success {
        #[allow(dead_code)]
        path: Option<String>,
    },

    #[serde(rename = "uploadProgress")]
    UploadProgress {
        #[allow(dead_code)]
        path: Option<String>,
        #[allow(dead_code)]
        progress: Option<f32>,
    },

    #[serde(rename = "downloadProgress")]
    DownloadProgress {
        #[allow(dead_code)]
        path: Option<String>,
        #[allow(dead_code)]
        progress: Option<f32>,
    },

    #[serde(other)]
    Unknown,
}

/// CLI error event structure for stderr parsing
#[derive(Debug, Deserialize)]
struct CliErrorEvent {
    #[serde(rename = "type")]
    event_type: Option<String>,
    error: Option<String>,
    message: Option<String>,
}

/// Information about the filen CLI location
struct FilenCliInfo {
    /// Path to the filen binary
    command: String,
    /// PATH environment variable to use (includes node binary directory)
    path_env: Option<String>,
}

/// Find the filen CLI binary by searching common installation paths.
/// This is necessary because GUI apps launched from Finder don't inherit shell PATH.
/// Returns both the filen path and the PATH env needed to run it (for node-based installs).
fn find_filen_cli() -> FilenCliInfo {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!("Could not determine home directory");
            return FilenCliInfo {
                command: "filen".to_string(),
                path_env: None,
            };
        }
    };

    // Common installation paths to search (with their bin directories for PATH)
    let search_paths: Vec<(PathBuf, Option<PathBuf>)> = vec![
        // Standard system paths - node should be in system PATH
        (
            PathBuf::from("/usr/local/bin/filen"),
            Some(PathBuf::from("/usr/local/bin")),
        ),
        (
            PathBuf::from("/opt/homebrew/bin/filen"),
            Some(PathBuf::from("/opt/homebrew/bin")),
        ),
        // User local bin
        (home.join(".local/bin/filen"), Some(home.join(".local/bin"))),
        // npm global installs
        (home.join(".npm/bin/filen"), Some(home.join(".npm/bin"))),
        (
            home.join(".npm-global/bin/filen"),
            Some(home.join(".npm-global/bin")),
        ),
    ];

    // Check standard paths first
    for (filen_path, bin_dir) in &search_paths {
        if filen_path.exists() {
            log::info!("Found filen CLI at: {:?}", filen_path);
            let path_env = bin_dir.as_ref().map(|d| build_path_env(d));
            return FilenCliInfo {
                command: filen_path.to_string_lossy().to_string(),
                path_env,
            };
        }
    }

    // Search fnm (Fast Node Manager) installations
    let fnm_base = home.join(".local/share/fnm/node-versions");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("installation/bin");
                let filen_path = bin_dir.join("filen");
                if filen_path.exists() {
                    let path_env = build_path_env(&bin_dir);
                    log::info!("Found filen CLI in fnm at: {:?}", filen_path);
                    return FilenCliInfo {
                        command: filen_path.to_string_lossy().to_string(),
                        path_env: Some(path_env),
                    };
                }
            }
        }
    }

    // Search nvm (Node Version Manager) installations
    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("bin");
                let filen_path = bin_dir.join("filen");
                if filen_path.exists() {
                    log::info!("Found filen CLI in nvm at: {:?}", filen_path);
                    return FilenCliInfo {
                        command: filen_path.to_string_lossy().to_string(),
                        path_env: Some(build_path_env(&bin_dir)),
                    };
                }
            }
        }
    }

    // Search volta installations
    let volta_bin = home.join(".volta/bin");
    let volta_filen = volta_bin.join("filen");
    if volta_filen.exists() {
        log::info!("Found filen CLI in volta at: {:?}", volta_filen);
        return FilenCliInfo {
            command: volta_filen.to_string_lossy().to_string(),
            path_env: Some(build_path_env(&volta_bin)),
        };
    }

    // Fallback to just "filen" (will use PATH if available)
    log::warn!("filen CLI not found in common paths, falling back to PATH lookup");
    FilenCliInfo {
        command: "filen".to_string(),
        path_env: None,
    }
}

/// Find node binary in common version manager locations
fn find_node_bin_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    // Check fnm (Fast Node Manager)
    let fnm_base = home.join(".local/share/fnm/node-versions");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("installation/bin");
                if bin_dir.join("node").exists() {
                    log::debug!("Found node in fnm at: {:?}", bin_dir);
                    return Some(bin_dir);
                }
            }
        }
    }

    // Check nvm (Node Version Manager)
    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("bin");
                if bin_dir.join("node").exists() {
                    log::debug!("Found node in nvm at: {:?}", bin_dir);
                    return Some(bin_dir);
                }
            }
        }
    }

    // Check volta
    let volta_bin = home.join(".volta/bin");
    if volta_bin.join("node").exists() {
        log::debug!("Found node in volta at: {:?}", volta_bin);
        return Some(volta_bin);
    }

    None
}

/// Build a PATH environment variable that includes the given bin directory
/// along with essential system paths and node binary location
fn build_path_env(bin_dir: &std::path::Path) -> String {
    let system_paths = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

    // Check if bin_dir already contains node
    if bin_dir.join("node").exists() {
        return format!("{}:{}", bin_dir.display(), system_paths);
    }

    // Try to find node in version managers
    if let Some(node_bin_dir) = find_node_bin_dir() {
        return format!(
            "{}:{}:{}",
            bin_dir.display(),
            node_bin_dir.display(),
            system_paths
        );
    }

    format!("{}:{}", bin_dir.display(), system_paths)
}

#[derive(Error, Debug)]
pub enum CliError {
    #[error("Failed to spawn CLI process: {0}")]
    Spawn(#[from] std::io::Error),
    #[allow(dead_code)]
    #[error("CLI not found. Please install filen-cli")]
    NotFound,
    #[allow(dead_code)]
    #[error("CLI process exited unexpectedly")]
    ProcessExited,
}

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
            log::error!("Sync cycle error: {:?}", error);
            state.set_sync_state(SyncState::Error).await;
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
    } else if line.starts_with("Syncing ")
        && !line.contains('{')
        && state.get_sync_state().await != SyncState::Syncing
    {
        state.set_sync_state(SyncState::Syncing).await;
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

    /// Check if filen CLI is installed
    pub async fn is_cli_available() -> bool {
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

    /// Start the sync process (uses CLI's stored session)
    pub async fn start_sync(&self, config: &Config) -> Result<(), CliError> {
        // Stop any existing process
        self.stop_sync().await;

        // Build the sync command
        let sync_pair = format!(
            "{}:{}:{}",
            config.local_path.display(),
            config.sync_mode,
            config.remote_path
        );

        log::info!("Starting sync with pair: {}", sync_pair);

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
            .arg(&sync_pair)
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

        // Set initial state - we'll transition to Synced after initial sync completes
        self.state.set_sync_state(SyncState::Syncing).await;

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
                            state.set_sync_state(SyncState::Error).await;
                            break;
                        }
                        Ok(Err(e)) => {
                            log::error!("Error reading CLI stdout: {}", e);
                            state.set_sync_state(SyncState::Error).await;
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

                while let Ok(Some(line)) = lines.next_line().await {
                    log::warn!("CLI stderr: {}", line);

                    // Try to parse as JSON error event
                    if let Ok(err_event) = serde_json::from_str::<CliErrorEvent>(&line) {
                        if err_event.event_type.as_deref() == Some("error") {
                            let msg = err_event.error.or(err_event.message).unwrap_or_default();
                            log::error!("CLI error: {}", msg);
                            state_for_stderr.set_sync_state(SyncState::Error).await;
                        }
                    } else if line.to_lowercase().contains("error") || line.contains("failed") {
                        // Fallback text detection for non-JSON errors
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

        // Kill the process
        if let Some(mut child) = self.process.write().await.take() {
            log::info!("Stopping sync process");
            let _ = child.kill().await;
        }

        self.state.set_sync_state(SyncState::Paused).await;
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
        let sync_pair = format!(
            "{}:{}:{}",
            config.local_path.display(),
            config.sync_mode,
            config.remote_path
        );

        log::info!("Running one-shot sync: {}", sync_pair);
        self.state.set_sync_state(SyncState::Syncing).await;

        let cli_info = find_filen_cli();
        let mut cmd = Command::new(&cli_info.command);
        cmd.arg("sync").arg(&sync_pair);

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
