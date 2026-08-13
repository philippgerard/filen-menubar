//! CLI module for managing the Filen CLI subprocess
//!
//! This module handles:
//! - Running the app-bundled Filen CLI backend (`discovery`)
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

pub use discovery::{bundled_cli_version, bundled_cli_version_output, FilenCliRuntime};
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
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

/// Maximum stderr bytes retained while waiting for a record terminator.
/// Diagnostics are useful, but a malformed helper must not be able to grow the
/// tray application's memory indefinitely by writing one newline-free record.
const MAX_STDERR_RECORD_BYTES: usize = 64 * 1024;

struct BoundedLineFramer {
    buffer: Vec<u8>,
    discarding: bool,
    max_bytes: usize,
}

impl BoundedLineFramer {
    fn new(max_bytes: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(max_bytes.min(8 * 1024)),
            discarding: false,
            max_bytes,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut records = Vec::new();

        for &byte in chunk {
            if self.discarding {
                if byte == b'\n' {
                    self.discarding = false;
                }
                continue;
            }

            if byte == b'\n' {
                if let Some(record) = self.take_record() {
                    records.push(record);
                }
                continue;
            }

            if self.buffer.len() == self.max_bytes {
                log::warn!(
                    "Discarding oversized CLI stderr record after {} buffered bytes",
                    self.buffer.len()
                );
                self.buffer.clear();
                self.discarding = true;
                continue;
            }

            self.buffer.push(byte);
        }

        records
    }

    fn finish(&mut self) -> Option<String> {
        if self.discarding {
            self.discarding = false;
            self.buffer.clear();
            None
        } else {
            self.take_record()
        }
    }

    fn take_record(&mut self) -> Option<String> {
        if self.buffer.last() == Some(&b'\r') {
            self.buffer.pop();
        }
        if self.buffer.is_empty() {
            return None;
        }

        let record = String::from_utf8_lossy(&self.buffer).into_owned();
        self.buffer.clear();
        Some(record)
    }
}

/// Manages the Filen CLI process
pub struct CliManager {
    runtime: FilenCliRuntime,
    state: AppState,
    activity: Arc<ActivityHistory>,
    /// Serializes complete start/stop transactions. In particular, a stop that
    /// is queued after a start cannot finish before that start spawns a child.
    lifecycle: Mutex<CliLifecycle>,
    /// Identifies the process whose output is allowed to update application
    /// state. Zero means that no generation is active (including during stop).
    active_generation: Arc<AtomicU64>,
    /// Tracks the direct child independently of pipe-monitor teardown so a
    /// natural exit is reflected immediately by `is_running`.
    running: Arc<AtomicBool>,
    /// User intent is distinct from whether the current child is alive. A
    /// natural crash keeps this true so the status loop may retry; pause,
    /// logout, and quit clear it before waiting for the lifecycle lock.
    desired_running: AtomicBool,
    /// Invalidates restart decisions made before a user stop request.
    start_epoch: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SyncStartPermit {
    epoch: u64,
}

#[derive(Default)]
struct CliLifecycle {
    next_generation: u64,
    stop_tx: Option<oneshot::Sender<()>>,
    supervisor: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct ProcessGeneration {
    state: AppState,
    activity: Arc<ActivityHistory>,
    active_generation: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    number: u64,
}

impl ProcessGeneration {
    fn is_active(&self) -> bool {
        self.active_generation.load(Ordering::SeqCst) == self.number
    }
}

impl CliManager {
    pub fn new(state: AppState, runtime: FilenCliRuntime) -> Self {
        Self::with_activity(state, Arc::new(ActivityHistory::in_memory()), runtime)
    }

    pub fn with_activity(
        state: AppState,
        activity: Arc<ActivityHistory>,
        runtime: FilenCliRuntime,
    ) -> Self {
        Self {
            runtime,
            state,
            activity,
            lifecycle: Mutex::new(CliLifecycle::default()),
            active_generation: Arc::new(AtomicU64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            desired_running: AtomicBool::new(false),
            start_epoch: AtomicU64::new(0),
        }
    }

    /// Activity history receiving canonical task events from this manager.
    pub fn activity_history(&self) -> &ActivityHistory {
        self.activity.as_ref()
    }

    /// Record an explicit start/resume request and return the permit carried
    /// through the asynchronous start transaction.
    pub(crate) fn request_sync_start(&self) -> SyncStartPermit {
        self.desired_running.store(true, Ordering::SeqCst);
        SyncStartPermit {
            epoch: self.start_epoch.load(Ordering::SeqCst),
        }
    }

    /// Capture a permit for an automatic retry without overriding pause/logout.
    pub(crate) fn retry_sync_permit(&self) -> Option<SyncStartPermit> {
        let epoch = self.start_epoch.load(Ordering::SeqCst);
        self.desired_running
            .load(Ordering::SeqCst)
            .then_some(SyncStartPermit { epoch })
    }

    fn permit_is_current(&self, permit: SyncStartPermit) -> bool {
        self.desired_running.load(Ordering::SeqCst)
            && self.start_epoch.load(Ordering::SeqCst) == permit.epoch
    }

    /// Check that the bundled helper exists and is exactly the patched version.
    async fn check_cli_once(&self) -> bool {
        self.check_cli_once_with_timeout(Duration::from_secs(5))
            .await
    }

    async fn check_cli_once_with_timeout(&self, probe_timeout: Duration) -> bool {
        log::info!(
            "Checking bundled Filen CLI at: {}",
            self.runtime.command().display()
        );

        let mut cmd = Command::new(self.runtime.command());
        cmd.args(self.runtime.common_args())
            .arg("--version")
            .stdin(Stdio::null()) // Prevent hanging on stdin when running from autostart
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        // Use a timeout to avoid hanging if the CLI is stuck
        match timeout(probe_timeout, cmd.output()).await {
            Ok(Ok(output)) => {
                let version = String::from_utf8_lossy(&output.stdout);
                let expected_version = bundled_cli_version_output();
                let available = output.status.success() && version.trim() == expected_version;
                if available {
                    log::info!("Bundled Filen CLI available: {}", version.trim());
                } else {
                    log::warn!(
                        "Bundled Filen CLI failed version validation: status={}, output={:?}, expected={:?}",
                        output.status,
                        version.trim(),
                        expected_version
                    );
                }
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

    /// Check whether the exact helper packaged beside this executable runs.
    /// Unlike PATH discovery, a bundled resource cannot appear later, so one
    /// bounded probe is sufficient and avoids delaying a useful error state.
    pub async fn is_cli_available(&self) -> bool {
        self.check_cli_once().await
    }

    fn sync_command(&self, sync_pairs_path: &std::path::Path) -> Command {
        let mut cmd = Command::new(self.runtime.command());
        cmd.args(self.runtime.common_args())
            .arg("--verbose")
            .arg("sync")
            .arg(sync_pairs_path)
            .arg("--continuous")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Run the CLI in its own process group so we can terminate the whole
        // tree if the backend spawns helper processes.
        #[cfg(unix)]
        cmd.process_group(0);

        cmd
    }

    /// Start the sync process (uses CLI's stored session)
    pub async fn start_sync(&self, config: &Config) -> Result<(), CliError> {
        let permit = self.request_sync_start();
        self.start_sync_if_permitted(config, permit).await
    }

    pub(crate) async fn start_sync_if_permitted(
        &self,
        config: &Config,
        permit: SyncStartPermit,
    ) -> Result<(), CliError> {
        let mut lifecycle = self.lifecycle.lock().await;
        if !self.permit_is_current(permit) {
            log::info!("Discarding a stale sync start request");
            return Ok(());
        }

        self.stop_sync_locked(&mut lifecycle).await;
        if !self.permit_is_current(permit) {
            log::info!("Sync start was cancelled while stopping the previous process");
            return Ok(());
        }

        // Generate syncPairs.json with ignore patterns
        let sync_pairs_path = config.write_sync_pairs().map_err(|e| {
            log::error!("Failed to write sync pairs: {}", e);
            CliError::SyncPairs(e.to_string())
        })?;

        log::info!("Generated syncPairs.json at: {:?}", sync_pairs_path);
        if !self.permit_is_current(permit) {
            log::info!("Sync start was cancelled before process launch");
            return Ok(());
        }
        self.start_sync_locked(config, &sync_pairs_path, &mut lifecycle)
            .await
    }

    #[cfg(test)]
    async fn start_sync_with_pairs_path(
        &self,
        config: &Config,
        sync_pairs_path: &std::path::Path,
    ) -> Result<(), CliError> {
        let permit = self.request_sync_start();
        self.start_sync_with_pairs_path_if_permitted(config, sync_pairs_path, permit)
            .await
    }

    #[cfg(test)]
    async fn start_sync_with_pairs_path_if_permitted(
        &self,
        config: &Config,
        sync_pairs_path: &std::path::Path,
        permit: SyncStartPermit,
    ) -> Result<(), CliError> {
        let mut lifecycle = self.lifecycle.lock().await;
        if !self.permit_is_current(permit) {
            return Ok(());
        }
        self.stop_sync_locked(&mut lifecycle).await;
        if !self.permit_is_current(permit) {
            return Ok(());
        }
        self.start_sync_locked(config, sync_pairs_path, &mut lifecycle)
            .await
    }

    async fn start_sync_locked(
        &self,
        config: &Config,
        sync_pairs_path: &std::path::Path,
        lifecycle: &mut CliLifecycle,
    ) -> Result<(), CliError> {
        log::info!(
            "Sync config: local={}, remote={}, mode={}, ignore={:?}, excludeDotFiles={}",
            config.local_path.display(),
            config.remote_path,
            config.sync_mode,
            config.ignore,
            config.exclude_dot_files
        );

        // Don't pass credentials - CLI will use its stored session.
        // Use --verbose to get detailed file sync information.
        log::info!(
            "Using bundled Filen CLI at: {}",
            self.runtime.command().display()
        );

        let mut cmd = self.sync_command(sync_pairs_path);
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Automatic retries do not set Scanning at their call site: doing so
        // before permit validation lets a completed pause be overwritten by a
        // stale restart. The lifecycle lock and successful spawn make this the
        // authoritative transition. Explicit starts already set the same state.
        self.state.set_sync_state(SyncState::Scanning).await;

        lifecycle.next_generation = lifecycle.next_generation.wrapping_add(1).max(1);
        let generation = lifecycle.next_generation;
        self.active_generation.store(generation, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);

        let (stop_tx, stop_rx) = oneshot::channel();
        lifecycle.stop_tx = Some(stop_tx);
        let process_generation = ProcessGeneration {
            state: self.state.clone(),
            activity: self.activity.clone(),
            active_generation: self.active_generation.clone(),
            running: self.running.clone(),
            number: generation,
        };
        lifecycle.supervisor = Some(tokio::spawn(Self::supervise_process(
            child,
            stdout,
            stderr,
            process_generation,
            stop_rx,
        )));

        Ok(())
    }

    /// Stop the sync process.
    ///
    /// Deliberately does NOT change the sync state: callers know the intent
    /// (pause sets Paused, logout sets NotLoggedIn, restart sets Scanning).
    /// Setting Paused here used to cause a visible flicker when start_sync
    /// stopped a crashed process during restart cleanup.
    pub async fn stop_sync(&self) {
        // Publish user intent before waiting for the transaction mutex. Any
        // already-decided retry carrying the previous epoch is now stale.
        self.desired_running.store(false, Ordering::SeqCst);
        self.start_epoch.fetch_add(1, Ordering::SeqCst);

        let mut lifecycle = self.lifecycle.lock().await;
        self.stop_sync_locked(&mut lifecycle).await;
    }

    async fn stop_sync_locked(&self, lifecycle: &mut CliLifecycle) {
        // Invalidate the old generation before asking it to stop. Its monitor
        // tasks are then joined before this transaction can launch a replacement.
        self.active_generation.store(0, Ordering::SeqCst);

        if let Some(tx) = lifecycle.stop_tx.take() {
            log::info!("Stopping sync process");
            let _ = tx.send(());
        }

        if let Some(supervisor) = lifecycle.supervisor.take() {
            if let Err(error) = supervisor.await {
                log::error!("CLI process supervisor failed: {error}");
            }
        }

        self.running.store(false, Ordering::SeqCst);
    }

    async fn supervise_process(
        mut child: Child,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
        generation: ProcessGeneration,
        mut stop_rx: oneshot::Receiver<()>,
    ) {
        let stdout_monitor =
            stdout.map(|stdout| tokio::spawn(Self::monitor_stdout(stdout, generation.clone())));
        let stderr_monitor =
            stderr.map(|stderr| tokio::spawn(Self::monitor_stderr(stderr, generation.clone())));

        let intentional = tokio::select! {
            biased;
            _ = &mut stop_rx => {
                Self::terminate_process_tree(&mut child).await;
                true
            }
            result = child.wait() => {
                match result {
                    Ok(status) => log::warn!("CLI process exited unexpectedly: {status}"),
                    Err(error) => log::error!("Failed to wait for CLI process: {error}"),
                }
                false
            }
        };

        // `child.wait()` has completed (naturally or through termination), so
        // the direct child is no longer running and has been reaped.
        generation.running.store(false, Ordering::SeqCst);

        let ((), ()) = tokio::join!(
            Self::join_monitor(stdout_monitor, "stdout"),
            Self::join_monitor(stderr_monitor, "stderr")
        );

        if intentional || !generation.is_active() {
            log::info!("CLI process stopped intentionally");
            return;
        }

        // Stderr has now been fully processed, so preserve its more specific
        // Offline diagnosis instead of replacing it with a generic error.
        if generation.state.get_sync_state().await != SyncState::Offline {
            generation.state.set_sync_state(SyncState::Error).await;
        }
        let _ = generation.active_generation.compare_exchange(
            generation.number,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    async fn monitor_stdout(stdout: tokio::process::ChildStdout, generation: ProcessGeneration) {
        let mut reader = BufReader::new(stdout);
        let mut chunk = [0_u8; 64 * 1024];
        let mut pending = Vec::new();
        let mut framer = JsonFramer::new();

        loop {
            if !generation.is_active() {
                break;
            }

            match reader.read(&mut chunk).await {
                Ok(bytes_read) if bytes_read > 0 => {
                    if !generation.is_active() {
                        break;
                    }
                    pending.extend_from_slice(&chunk[..bytes_read]);
                    if let Some(text) = take_valid_utf8(&mut pending) {
                        log::debug!("Read {} bytes from CLI stdout", bytes_read);
                        let frames = framer.push_chunk(&text);
                        handle_cli_frames(&generation.state, &generation.activity, frames).await;
                    }
                }
                Ok(_) => {
                    if !pending.is_empty() && generation.is_active() {
                        let text = String::from_utf8_lossy(&pending).into_owned();
                        let frames = framer.push_chunk(&text);
                        handle_cli_frames(&generation.state, &generation.activity, frames).await;
                    }
                    break;
                }
                Err(error) => {
                    if generation.is_active() {
                        log::warn!("Failed reading CLI stdout: {error}");
                    }
                    break;
                }
            }
        }
    }

    async fn monitor_stderr(stderr: tokio::process::ChildStderr, generation: ProcessGeneration) {
        let mut reader = BufReader::new(stderr);
        let mut chunk = [0_u8; 16 * 1024];
        let mut framer = BoundedLineFramer::new(MAX_STDERR_RECORD_BYTES);
        let mut network_error_detected = false;

        loop {
            if !generation.is_active() {
                break;
            }

            match reader.read(&mut chunk).await {
                Ok(bytes_read) if bytes_read > 0 => {
                    if !generation.is_active() {
                        break;
                    }
                    for line in framer.push_chunk(&chunk[..bytes_read]) {
                        if !generation.is_active() {
                            break;
                        }
                        Self::handle_stderr_record(&generation, &mut network_error_detected, &line)
                            .await;
                    }
                }
                Ok(_) => {
                    if generation.is_active() {
                        if let Some(line) = framer.finish() {
                            Self::handle_stderr_record(
                                &generation,
                                &mut network_error_detected,
                                &line,
                            )
                            .await;
                        }
                    }
                    break;
                }
                Err(error) => {
                    if generation.is_active() {
                        log::warn!("Failed reading CLI stderr: {error}");
                    }
                    break;
                }
            }
        }
    }

    async fn handle_stderr_record(
        generation: &ProcessGeneration,
        network_error_detected: &mut bool,
        line: &str,
    ) {
        log::warn!("CLI stderr: {}", line);

        if let Ok(err_event) = serde_json::from_str::<CliErrorEvent>(line) {
            if err_event.event_type.as_deref() == Some("error") {
                let msg = err_event.error.or(err_event.message).unwrap_or_default();
                if is_network_error(&msg) {
                    log::warn!("Network error from stderr: {}", msg);
                    generation.state.set_sync_state(SyncState::Offline).await;
                    *network_error_detected = true;
                } else if !*network_error_detected {
                    log::error!("CLI error: {}", msg);
                    generation.state.set_sync_state(SyncState::Error).await;
                }
            }
        } else if is_network_error(line) {
            log::warn!("Network error detected in stderr: {}", line);
            generation.state.set_sync_state(SyncState::Offline).await;
            *network_error_detected = true;
        } else if !*network_error_detected
            && (line.to_ascii_lowercase().contains("error") || line.contains("failed"))
        {
            generation.state.set_sync_state(SyncState::Error).await;
        }
    }

    async fn join_monitor(mut monitor: Option<JoinHandle<()>>, stream: &str) {
        let Some(mut monitor) = monitor.take() else {
            return;
        };

        match timeout(Duration::from_secs(2), &mut monitor).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::error!("CLI {stream} monitor failed: {error}"),
            Err(_) => {
                log::warn!("CLI {stream} monitor did not finish; aborting it");
                monitor.abort();
                let _ = monitor.await;
            }
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
                if matches!(
                    timeout(Duration::from_secs(2), child.wait()).await,
                    Ok(Ok(_))
                ) {
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
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    /// Check if sync is running
    #[allow(dead_code)]
    pub async fn is_running(&self) -> bool {
        if !self.running.load(Ordering::SeqCst) {
            return false;
        }

        let lifecycle = self.lifecycle.lock().await;
        let running = lifecycle
            .supervisor
            .as_ref()
            .is_some_and(|supervisor| !supervisor.is_finished());
        if !running {
            self.running.store(false, Ordering::SeqCst);
        }
        running
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
    use super::{
        bundled_cli_version_output, handle_cli_event, take_valid_utf8, text_preview,
        BoundedLineFramer, CliEvent, CliManager, FilenCliRuntime, TransferData,
    };
    use crate::activity::ActivityHistory;
    use crate::config::Config;
    use crate::state::{AppState, CurrentTransfer, SyncState, TransferDirection};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

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

    #[cfg(unix)]
    fn fake_sync_manager(
        script_body: &str,
    ) -> (tempfile::TempDir, Arc<CliManager>, Config, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("create fake CLI directory");
        let entrypoint = temp.path().join("fake-filen-cli.sh");
        let command = temp.path().join("fake-node");
        std::fs::write(&entrypoint, format!("#!/bin/sh\n{script_body}\n"))
            .expect("write fake sync CLI");
        std::fs::set_permissions(&entrypoint, std::fs::Permissions::from_mode(0o700))
            .expect("make fake sync CLI executable");
        std::fs::write(
            &command,
            "#!/bin/sh\n[ \"$1\" = \"--disable-warning=DEP0169\" ] || exit 41\nshift\nentrypoint=$1\nshift\nexec \"$entrypoint\" \"$@\"\n",
        )
        .expect("write fake Node wrapper");
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o700))
            .expect("make fake Node wrapper executable");

        let pairs_path = temp.path().join("syncPairs.json");
        std::fs::write(&pairs_path, "[]\n").expect("write fake sync pairs");
        let config = Config {
            local_path: temp.path().join("sync-root"),
            ..Config::default()
        };
        let runtime = FilenCliRuntime::new(command, entrypoint, temp.path().join("data"));
        let manager = Arc::new(CliManager::new(AppState::new(), runtime));

        (temp, manager, config, pairs_path)
    }

    #[cfg(unix)]
    async fn wait_for_pid_count(path: &Path, expected: usize) -> Vec<i32> {
        for _ in 0..100 {
            let pids = std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.parse::<i32>().ok())
                .collect::<Vec<_>>();
            if pids.len() >= expected {
                return pids;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        panic!(
            "expected at least {expected} fake process ids in {}",
            path.display()
        );
    }

    #[cfg(unix)]
    fn process_is_alive(pid: i32) -> bool {
        (unsafe { libc::kill(pid, 0) }) == 0
    }

    #[cfg(unix)]
    async fn wait_for_process_exit(pid: i32) {
        for _ in 0..100 {
            if !process_is_alive(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("fake CLI process {pid} remained alive");
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

    #[test]
    fn stderr_framer_discards_oversized_newline_free_record_and_recovers() {
        let mut framer = BoundedLineFramer::new(8);

        assert!(framer.push_chunk(b"1234567890123456").is_empty());
        assert!(framer.buffer.is_empty());
        assert!(framer.discarding);

        assert_eq!(
            framer.push_chunk(b"still discarded\nhealthy\r\n"),
            vec!["healthy".to_string()]
        );
        assert!(!framer.discarding);
    }

    #[test]
    fn stderr_framer_flushes_a_bounded_final_record() {
        let mut framer = BoundedLineFramer::new(16);

        assert!(framer.push_chunk(b"final error").is_empty());
        assert_eq!(framer.finish().as_deref(), Some("final error"));
        assert!(framer.finish().is_none());
    }

    #[cfg(unix)]
    fn fake_version_cli(version: &str) -> (tempfile::TempDir, FilenCliRuntime) {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("create fake CLI directory");
        let command = temp.path().join("fake-node");
        let entrypoint = temp.path().join("filen-cli.cjs");
        let script = format!(
            "#!/bin/sh\n\
             [ \"$1\" = \"--disable-warning=DEP0169\" ] || exit 41\n\
             [ \"$2\" = \"{}\" ] || exit 42\n\
             [ \"$3\" = \"--skip-update\" ] || exit 43\n\
             [ \"$4\" = \"--data-dir\" ] || exit 44\n\
             [ -n \"$5\" ] || exit 45\n\
             [ \"$6\" = \"--version\" ] || exit 46\n\
             printf '%s\\n' '{}'\n",
            entrypoint.display(),
            version
        );
        std::fs::write(&command, script).expect("write fake CLI");
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o700))
            .expect("make fake CLI executable");
        let data_dir = temp.path().join("data with spaces");

        (temp, FilenCliRuntime::new(command, entrypoint, data_dir))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn availability_accepts_only_the_pinned_bundled_version() {
        let expected_version = bundled_cli_version_output();
        let (_temp, runtime) = fake_version_cli(&expected_version);
        let manager = CliManager::new(AppState::new(), runtime);
        assert!(manager.check_cli_once().await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn availability_rejects_a_stock_classic_cli() {
        let (_temp, runtime) = fake_version_cli("Filen CLI v0.0.39");
        let manager = CliManager::new(AppState::new(), runtime);
        assert!(!manager.check_cli_once().await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_version_probe_kills_the_helper() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("create fake CLI directory");
        let command = temp.path().join("fake-node");
        let entrypoint = temp.path().join("filen-cli.cjs");
        let pid_file = temp.path().join("pid");
        let script = format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nwhile :; do :; done\n",
            pid_file.display()
        );
        std::fs::write(&command, script).expect("write hanging fake CLI");
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o700))
            .expect("make fake CLI executable");
        let runtime = FilenCliRuntime::new(command, entrypoint, temp.path().join("data"));
        let manager = CliManager::new(AppState::new(), runtime);

        assert!(
            !manager
                .check_cli_once_with_timeout(Duration::from_secs(1))
                .await
        );
        let pid = std::fs::read_to_string(&pid_file)
            .expect("probe wrote its pid")
            .parse::<i32>()
            .expect("pid is numeric");

        for _ in 0..20 {
            if unsafe { libc::kill(pid, 0) } == -1 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        panic!("timed-out helper process {pid} remained alive");
    }

    #[test]
    fn sync_command_uses_the_injected_runtime_and_common_arguments() {
        use std::ffi::OsString;

        let runtime = FilenCliRuntime::new(
            PathBuf::from("/Applications/Filen Menubar.app/node"),
            PathBuf::from("/Applications/Filen Menubar.app/filen-cli.cjs"),
            PathBuf::from("/tmp/Filen CLI Data"),
        );
        let manager = CliManager::new(AppState::new(), runtime);
        let command = manager.sync_command(Path::new("/tmp/sync pairs.json"));
        let args = command
            .as_std()
            .get_args()
            .map(OsString::from)
            .collect::<Vec<_>>();

        assert_eq!(
            command.as_std().get_program(),
            std::ffi::OsStr::new("/Applications/Filen Menubar.app/node")
        );
        assert_eq!(
            args,
            vec![
                "--disable-warning=DEP0169",
                "/Applications/Filen Menubar.app/filen-cli.cjs",
                "--skip-update",
                "--data-dir",
                "/tmp/Filen CLI Data",
                "--verbose",
                "sync",
                "/tmp/sync pairs.json",
                "--continuous",
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stop_queued_after_start_prevents_a_late_process_launch() {
        let (_temp, manager, config, pairs_path) = fake_sync_manager(
            r#"mkdir -p "$3"
printf '%s\n' "$$" >> "$3/pids"
trap 'exit 0' TERM INT
while :; do sleep 1; done"#,
        );
        manager.state.set_sync_state(SyncState::Scanning).await;

        // Hold the transaction lock long enough to queue start first and stop
        // second. Tokio's mutex is FIFO, so pause/logout semantics must win.
        let gate = manager.lifecycle.lock().await;

        let manager_for_start = manager.clone();
        let config_for_start = config.clone();
        let pairs_for_start = pairs_path.clone();
        let (start_entered_tx, start_entered_rx) = tokio::sync::oneshot::channel();
        let start_task = tokio::spawn(async move {
            let _ = start_entered_tx.send(());
            manager_for_start
                .start_sync_with_pairs_path(&config_for_start, &pairs_for_start)
                .await
        });
        start_entered_rx.await.expect("start task entered");
        tokio::task::yield_now().await;

        let manager_for_stop = manager.clone();
        let (stop_entered_tx, stop_entered_rx) = tokio::sync::oneshot::channel();
        let stop_task = tokio::spawn(async move {
            let _ = stop_entered_tx.send(());
            manager_for_stop.stop_sync().await;
        });
        stop_entered_rx.await.expect("stop task entered");
        tokio::task::yield_now().await;
        drop(gate);

        start_task
            .await
            .expect("start task joined")
            .expect("fake CLI started");
        stop_task.await.expect("stop task joined");
        assert!(!manager.is_running().await);

        let pid_file = manager.runtime.data_dir().join("pids");
        if let Some(pid) = std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|contents| contents.lines().next()?.parse::<i32>().ok())
        {
            wait_for_process_exit(pid).await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn startup_start_captured_before_state_publication_cannot_override_pause() {
        let (_temp, manager, config, pairs_path) = fake_sync_manager(
            r#"mkdir -p "$3"
printf '%s\n' "$$" >> "$3/pids"
trap 'exit 0' TERM INT
while :; do sleep 1; done"#,
        );
        // Startup captures its intent before publishing Scanning. A pause that
        // arrives after that publication must invalidate the captured permit.
        let startup_permit = manager.request_sync_start();
        manager.state.set_sync_state(SyncState::Scanning).await;

        // Pause fully completes before the startup task reaches process launch.
        manager.stop_sync().await;
        manager.state.set_sync_state(SyncState::Paused).await;
        manager
            .start_sync_with_pairs_path_if_permitted(&config, &pairs_path, startup_permit)
            .await
            .expect("stale startup was discarded without a spawn error");

        assert!(!manager.is_running().await);
        assert_eq!(manager.state.get_sync_state().await, SyncState::Paused);
        assert!(!manager.runtime.data_dir().join("pids").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn concurrent_starts_join_stale_monitors_and_leave_one_process() {
        let (_temp, manager, config, pairs_path) = fake_sync_manager(
            r#"mkdir -p "$3"
printf '%s\n' "$$" >> "$3/pids"
trap 'printf "%s\n" "error: stale monitor" >&2; exit 0' TERM INT
while :; do sleep 1; done"#,
        );
        manager.state.set_sync_state(SyncState::Scanning).await;
        manager
            .start_sync_with_pairs_path(&config, &pairs_path)
            .await
            .expect("initial fake CLI started");
        let pid_file = manager.runtime.data_dir().join("pids");
        let initial_pid = wait_for_pid_count(&pid_file, 1).await[0];

        let gate = manager.lifecycle.lock().await;
        let mut starts = Vec::new();
        for _ in 0..2 {
            let manager = manager.clone();
            let config = config.clone();
            let pairs_path = pairs_path.clone();
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            starts.push(tokio::spawn(async move {
                let _ = entered_tx.send(());
                manager
                    .start_sync_with_pairs_path(&config, &pairs_path)
                    .await
            }));
            entered_rx.await.expect("start task entered");
            tokio::task::yield_now().await;
        }
        drop(gate);

        for start in starts {
            start
                .await
                .expect("start task joined")
                .expect("replacement fake CLI started");
        }

        let mut observed_pids = Vec::new();
        for _ in 0..100 {
            observed_pids = std::fs::read_to_string(&pid_file)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.parse::<i32>().ok())
                .collect();
            if observed_pids.len() >= 2
                && observed_pids
                    .iter()
                    .filter(|pid| process_is_alive(**pid))
                    .count()
                    == 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(!process_is_alive(initial_pid));
        assert_eq!(
            observed_pids
                .iter()
                .filter(|pid| process_is_alive(**pid))
                .count(),
            1,
            "only the newest generation may remain alive"
        );
        assert!(manager.is_running().await);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(manager.state.get_sync_state().await, SyncState::Scanning);

        manager.stop_sync().await;
        manager.state.set_sync_state(SyncState::Paused).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(manager.state.get_sync_state().await, SyncState::Paused);
        for pid in observed_pids {
            wait_for_process_exit(pid).await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn natural_child_exit_is_reaped_and_clears_running() {
        let (_temp, manager, config, pairs_path) = fake_sync_manager(
            r#"mkdir -p "$3"
printf '%s\n' "$$" > "$3/pids"
exit 23"#,
        );
        manager.state.set_sync_state(SyncState::Scanning).await;
        manager
            .start_sync_with_pairs_path(&config, &pairs_path)
            .await
            .expect("fake CLI started");

        let pid_file = manager.runtime.data_dir().join("pids");
        let pid = wait_for_pid_count(&pid_file, 1).await[0];
        for _ in 0..100 {
            if !manager.is_running().await
                && manager.state.get_sync_state().await == SyncState::Error
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(!manager.is_running().await);
        assert_eq!(manager.state.get_sync_state().await, SyncState::Error);
        wait_for_process_exit(pid).await;
        assert!(
            manager.retry_sync_permit().is_some(),
            "a natural crash must preserve desired-running intent for retry"
        );
        manager.stop_sync().await;
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
