//! CLI module for managing the Filen CLI subprocess
//!
//! This module handles:
//! - Finding the Filen CLI binary on the system (`discovery`)
//! - Parsing JSON events from the CLI's verbose output (`events`)
//! - Detecting network errors for offline status (`network`)
//! - Process abstraction for testability (`process`)
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
//!
//! ## Testing
//!
//! The `process` module provides a `ProcessRunner` trait that can be mocked
//! for testing CLI interactions without spawning real processes.

mod discovery;
mod events;
pub mod framer;
pub mod network;
pub mod process;

pub use discovery::{find_filen_cli, FilenCliInfo};
pub use events::{CliErrorEvent, CliEvent, TransferData};
use framer::{Frame, JsonFramer};

// Re-export process types for dependency injection (currently unused, for future testability)
#[allow(unused_imports)]
pub use process::{ProcessHandle, ProcessRunner, TokioProcessRunner};

use crate::activity::ActivityHistory;
use crate::config::Config;
use crate::error::CliError;
use crate::state::{AppState, CurrentTransfer, StorageInfo, SyncState, TransferDirection};
use network::is_network_error;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// Whether an operation name identifies a canonical sync task rather than a
/// lower-level upload/download progress event.
fn is_task_operation(operation: Option<&str>) -> bool {
    matches!(
        operation,
        Some(
            "uploadFile"
                | "downloadFile"
                | "createLocalDirectory"
                | "createRemoteDirectory"
                | "deleteLocalDirectory"
                | "deleteLocalFile"
                | "deleteRemoteDirectory"
                | "deleteRemoteFile"
                | "renameLocalDirectory"
                | "renameLocalFile"
                | "renameRemoteDirectory"
                | "renameRemoteFile"
        )
    )
}

/// Handle a parsed CLI event and update app state accordingly.
///
/// The CLI emits two completion events for file copies: a low-level
/// `upload`/`download` event with `type="finished"`, followed by the canonical
/// `uploadFile`/`downloadFile` task event with `type="success"`. Only canonical
/// task events affect the pending count and activity history.
async fn handle_cli_event(state: &AppState, activity: &ActivityHistory, event: CliEvent) {
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
            state.set_last_synced_now().await;
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
                let operation = transfer_data.operation.as_deref();
                let transfer_type = transfer_data.transfer_type.as_deref();
                let is_task = is_task_operation(operation);
                if is_task && matches!(transfer_type, Some("success" | "error")) {
                    // Do not content-deduplicate task events: the same path may
                    // legitimately be changed more than once in one cycle.
                    let _ = activity.record_transfer(transfer_data);
                }

                // Determine direction for all transfer types
                let direction = match operation {
                    Some("upload") | Some("uploadFile") => Some(TransferDirection::Upload),
                    Some("download") | Some("downloadFile") => Some(TransferDirection::Download),
                    _ => None, // createRemoteDirectory, etc. don't show indicator
                };

                // Only task-level success is a completed sync action. Raw
                // upload/download `finished` events are progress lifecycle
                // signals and are followed by uploadFile/downloadFile success.
                if is_task && transfer_type == Some("success") {
                    let current = state.get_pending_count().await;
                    if current > 0 {
                        let new_count = current - 1;
                        log::debug!("Transfer complete, {} files remaining", new_count);
                        state.set_pending_count(new_count).await;
                    }
                    // Clear current transfer when this file is done
                    state.set_current_transfer(None).await;
                } else if is_task && transfer_type == Some("error") {
                    state.set_current_transfer(None).await;
                } else if matches!(operation, Some("upload" | "download"))
                    && matches!(transfer_type, Some("finished" | "success" | "error"))
                {
                    // Clear the live progress indicator, but do not decrement
                    // or record: a canonical task terminal event follows.
                    state.set_current_transfer(None).await;
                } else if transfer_type == Some("started")
                    || transfer_type == Some("progress")
                    || transfer_type == Some("queued")
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
                } else if transfer_type == Some("error") {
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
            // Legacy standalone success events do not identify which canonical
            // task completed. Only task-level Transfer success events decrement
            // the pending count or enter activity history.
        }
        CliEvent::Unknown => {
            // Ignore unknown event types
        }
    }
}

async fn handle_cli_frames(state: &AppState, activity: &ActivityHistory, frames: Vec<Frame>) {
    for frame in frames {
        match frame {
            Frame::Json(complete_json) => match serde_json::from_str::<CliEvent>(&complete_json) {
                Ok(event) => {
                    handle_cli_event(state, activity, event).await;
                }
                Err(e) => {
                    log::debug!(
                        "Failed to parse JSON event: {} - {}",
                        e,
                        text_preview(&complete_json, 100)
                    );
                }
            },
            Frame::Text(text) => {
                handle_text_output(state, &text).await;
            }
        }
    }
}

/// Decode the valid UTF-8 prefix currently available in `buffer`.
///
/// A read may end in the middle of a multi-byte character. Retain only that
/// incomplete suffix for the next chunk; actual invalid UTF-8 is lossily
/// decoded so malformed CLI diagnostics cannot stall the monitor.
fn take_valid_utf8(buffer: &mut Vec<u8>) -> Option<String> {
    if buffer.is_empty() {
        return None;
    }

    match std::str::from_utf8(buffer) {
        Ok(_) => String::from_utf8(std::mem::take(buffer)).ok(),
        Err(error) if error.error_len().is_none() => {
            let valid_len = error.valid_up_to();
            if valid_len == 0 {
                return None;
            }

            let incomplete_suffix = buffer.split_off(valid_len);
            let valid_prefix = std::mem::replace(buffer, incomplete_suffix);
            String::from_utf8(valid_prefix).ok()
        }
        Err(_) => {
            let invalid = std::mem::take(buffer);
            Some(String::from_utf8_lossy(&invalid).into_owned())
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

/// Return at most `max_chars` Unicode scalar values for diagnostic logging.
fn text_preview(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Manages the Filen CLI process
pub struct CliManager {
    process: Arc<RwLock<Option<Child>>>,
    state: AppState,
    activity: Arc<ActivityHistory>,
    shutdown_tx: Arc<RwLock<Option<mpsc::Sender<()>>>>,
    /// Set while the app itself is stopping the CLI (pause, logout, quit, restart).
    /// The output monitors check this so an intentional kill is not reported as
    /// an Error state — which would otherwise trigger the auto-restart loop.
    stopping: Arc<AtomicBool>,
}

impl CliManager {
    pub fn new(state: AppState) -> Self {
        Self::with_activity(state, Arc::new(ActivityHistory::in_memory()))
    }

    pub fn with_activity(state: AppState, activity: Arc<ActivityHistory>) -> Self {
        Self {
            process: Arc::new(RwLock::new(None)),
            state,
            activity,
            shutdown_tx: Arc::new(RwLock::new(None)),
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Activity history receiving canonical task events from this manager.
    pub fn activity_history(&self) -> &ActivityHistory {
        self.activity.as_ref()
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
        // Discovery walks the filesystem; keep it off the async runtime
        let cli_info = tokio::task::spawn_blocking(find_filen_cli)
            .await
            .map_err(|e| CliError::Spawn(std::io::Error::other(e)))?;
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

        // Run the CLI in its own process group so we can terminate the whole
        // tree (the CLI is a Node app that may spawn children)
        #[cfg(unix)]
        cmd.process_group(0);

        // Set PATH if we found a specific installation (needed for node-based CLI)
        if let Some(ref path_env) = cli_info.path_env {
            cmd.env("PATH", path_env);
        }

        // New process: future exits are real crashes until stop_sync says otherwise
        self.stopping.store(false, Ordering::SeqCst);

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
        let activity = self.activity.clone();
        let stopping = self.stopping.clone();
        tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut reader = BufReader::new(stdout);
                let mut chunk = [0_u8; 64 * 1024];
                let mut pending = Vec::new();

                // Frames multi-line pretty-printed JSON objects from the CLI
                let mut framer = JsonFramer::new();

                loop {
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            log::info!("Sync monitor received shutdown signal");
                            break;
                        }
                        result = reader.read(&mut chunk) => match result {
                        Ok(bytes_read) if bytes_read > 0 => {
                            pending.extend_from_slice(&chunk[..bytes_read]);
                            if let Some(text) = take_valid_utf8(&mut pending) {
                                log::debug!("Read {} bytes from CLI stdout", bytes_read);
                                let frames = framer.push_chunk(&text);
                                handle_cli_frames(&state, &activity, frames).await;
                            }
                        }
                        Ok(_) | Err(_) => {
                            if !pending.is_empty() {
                                let text = String::from_utf8_lossy(&pending).into_owned();
                                let frames = framer.push_chunk(&text);
                                handle_cli_frames(&state, &activity, frames).await;
                            }

                            // EOF or read error - process exited
                            if stopping.load(Ordering::SeqCst) {
                                // We killed it on purpose (pause/logout/quit/restart);
                                // don't report an error state
                                log::info!("CLI process stopped intentionally");
                                break;
                            }
                            log::warn!("CLI process stdout closed unexpectedly");
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
                        },
                    }
                }
            }
        });

        // Spawn stderr monitoring task
        let state_for_stderr = self.state.clone();
        let stopping_for_stderr = self.stopping.clone();
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                // Track if we've detected a network error in this stderr stream
                // Once detected, we shouldn't downgrade to Error
                let mut network_error_detected = false;

                while let Ok(Some(line)) = lines.next_line().await {
                    log::warn!("CLI stderr: {}", line);

                    // Ignore the noise a killed process flushes during intentional stop
                    if stopping_for_stderr.load(Ordering::SeqCst) {
                        continue;
                    }

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

    /// Stop the sync process.
    ///
    /// Deliberately does NOT change the sync state: callers know the intent
    /// (pause sets Paused, logout sets NotLoggedIn, restart sets Scanning).
    /// Setting Paused here used to cause a visible flicker when start_sync
    /// stopped a crashed process during restart cleanup.
    pub async fn stop_sync(&self) {
        // Mark this as an intentional stop BEFORE killing, so the output
        // monitors don't interpret the process exit as a crash
        self.stopping.store(true, Ordering::SeqCst);

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(()).await;
        }

        // Kill the process
        if let Some(mut child) = self.process.write().await.take() {
            log::info!("Stopping sync process");
            Self::terminate_process_tree(&mut child).await;
        }
    }

    /// Terminate the CLI and any children it spawned.
    ///
    /// On Unix the CLI runs in its own process group (see `start_sync`), so we
    /// first send SIGTERM to the group for a graceful shutdown, then escalate
    /// to SIGKILL if it hasn't exited shortly after.
    async fn terminate_process_tree(child: &mut Child) {
        #[cfg(unix)]
        {
            if let Some(pid) = child.id() {
                let pgid = pid as i32;
                unsafe {
                    libc::killpg(pgid, libc::SIGTERM);
                }
                // Give the CLI a moment to shut down cleanly
                if timeout(Duration::from_secs(2), child.wait()).await.is_ok() {
                    // Reap any stragglers in the group
                    unsafe {
                        libc::killpg(pgid, libc::SIGKILL);
                    }
                    return;
                }
                log::warn!("CLI did not exit after SIGTERM, sending SIGKILL");
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                }
            }
        }
        let _ = child.kill().await;
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
}

#[cfg(test)]
mod tests {
    use super::{handle_cli_event, take_valid_utf8, text_preview, CliEvent, TransferData};
    use crate::activity::ActivityHistory;
    use crate::state::{AppState, CurrentTransfer, TransferDirection};

    fn transfer_event(operation: &str, transfer_type: &str) -> CliEvent {
        CliEvent::Transfer {
            data: Some(TransferData {
                operation: Some(operation.to_string()),
                transfer_type: Some(transfer_type.to_string()),
                relative_path: Some("documents/report.pdf".to_string()),
                bytes: None,
                size: Some(1024),
            }),
        }
    }

    #[test]
    fn utf8_extraction_retains_incomplete_character() {
        let mut buffer = b"first\ncaf\xc3".to_vec();
        assert_eq!(take_valid_utf8(&mut buffer).as_deref(), Some("first\ncaf"));
        assert_eq!(buffer, b"\xc3");

        buffer.extend_from_slice(b"\xa9\n");
        assert_eq!(take_valid_utf8(&mut buffer).as_deref(), Some("\u{e9}\n"));
        assert!(buffer.is_empty());
    }

    #[test]
    fn utf8_extraction_handles_many_lines_in_one_chunk() {
        let expected = "ignored\n".repeat(110_000);
        let mut buffer = expected.as_bytes().to_vec();
        assert_eq!(
            take_valid_utf8(&mut buffer).as_deref(),
            Some(expected.as_str())
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn text_preview_preserves_short_text_and_zero_limit() {
        assert_eq!(text_preview("short é text", 100), "short é text");
        assert_eq!(text_preview("not empty", 0), "");
    }

    #[test]
    fn text_preview_truncates_at_unicode_character_boundary() {
        // The old 100-byte slice ended in the middle of `é`.
        let text = format!("{}é-tail", "a".repeat(99));
        assert_eq!(text_preview(&text, 100), format!("{}é", "a".repeat(99)));
    }

    #[tokio::test]
    async fn raw_finished_clears_progress_without_counting_or_recording_twice() {
        let state = AppState::new();
        state.set_pending_count(2).await;
        state
            .set_current_transfer(Some(CurrentTransfer::new(
                TransferDirection::Upload,
                "report.pdf".to_string(),
                1024,
            )))
            .await;
        let activity = ActivityHistory::in_memory();

        handle_cli_event(&state, &activity, transfer_event("upload", "finished")).await;

        assert_eq!(state.get_pending_count().await, 2);
        assert!(state.get_current_transfer().await.is_none());
        assert!(activity.snapshot_newest_first().is_empty());

        handle_cli_event(&state, &activity, transfer_event("uploadFile", "success")).await;

        assert_eq!(state.get_pending_count().await, 1);
        assert_eq!(activity.snapshot_newest_first().len(), 1);
    }

    #[tokio::test]
    async fn task_error_is_recorded_without_decrementing_pending_count() {
        let state = AppState::new();
        state.set_pending_count(1).await;
        let activity = ActivityHistory::in_memory();

        handle_cli_event(
            &state,
            &activity,
            transfer_event("deleteRemoteFile", "error"),
        )
        .await;

        assert_eq!(state.get_pending_count().await, 1);
        assert_eq!(activity.snapshot_newest_first().len(), 1);
    }

    #[tokio::test]
    async fn standalone_success_does_not_decrement_pending_count() {
        let state = AppState::new();
        state.set_pending_count(1).await;
        let activity = ActivityHistory::in_memory();

        handle_cli_event(
            &state,
            &activity,
            CliEvent::Success {
                path: Some("documents/report.pdf".to_string()),
            },
        )
        .await;

        assert_eq!(state.get_pending_count().await, 1);
        assert!(activity.snapshot_newest_first().is_empty());
    }
}
