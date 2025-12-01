use crate::config::Config;
use crate::state::{AppState, StorageInfo, SyncState};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// Find the filen CLI binary by searching common installation paths.
/// This is necessary because GUI apps launched from Finder don't inherit shell PATH.
fn find_filen_cli() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    // Common installation paths to search
    let search_paths: Vec<PathBuf> = vec![
        // Standard system paths
        PathBuf::from("/usr/local/bin/filen"),
        PathBuf::from("/opt/homebrew/bin/filen"),
        // User local bin
        home.join(".local/bin/filen"),
        // npm global installs
        home.join(".npm/bin/filen"),
        home.join(".npm-global/bin/filen"),
    ];

    // Check standard paths first
    for path in &search_paths {
        if path.exists() {
            log::info!("Found filen CLI at: {:?}", path);
            return Some(path.clone());
        }
    }

    // Search fnm (Fast Node Manager) installations
    let fnm_base = home.join(".local/share/fnm/node-versions");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                let filen_path = entry.path().join("installation/bin/filen");
                if filen_path.exists() {
                    log::info!("Found filen CLI in fnm at: {:?}", filen_path);
                    return Some(filen_path);
                }
            }
        }
    }

    // Search nvm (Node Version Manager) installations
    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let filen_path = entry.path().join("bin/filen");
                if filen_path.exists() {
                    log::info!("Found filen CLI in nvm at: {:?}", filen_path);
                    return Some(filen_path);
                }
            }
        }
    }

    // Search volta installations
    let volta_base = home.join(".volta/bin/filen");
    if volta_base.exists() {
        log::info!("Found filen CLI in volta at: {:?}", volta_base);
        return Some(volta_base);
    }

    // Fallback to just "filen" (will use PATH if available)
    log::warn!("filen CLI not found in common paths, falling back to PATH lookup");
    None
}

/// Get the filen CLI command path
fn get_filen_command() -> String {
    find_filen_cli()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "filen".to_string())
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
        let filen_cmd = get_filen_command();
        log::debug!("Checking filen CLI availability at: {}", filen_cmd);
        Command::new(&filen_cmd)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
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
        let filen_cmd = get_filen_command();
        log::info!("Using filen CLI at: {}", filen_cmd);
        let mut cmd = Command::new(&filen_cmd);
        cmd.arg("--verbose")
            .arg("sync")
            .arg(&sync_pair)
            .arg("--continuous")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

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

                            // Parse CLI output to determine state
                            // With --verbose, CLI outputs JSON events with "type" field
                            // Note: We don't set Syncing on cycleStarted because cycles run
                            // frequently even when idle. We only show Syncing when there's
                            // actual work (deltasCount > 0 or file transfers).
                            if line.contains("\"type\": \"cycleProcessingTasksStarted\"") {
                                // Tasks are about to be processed - set syncing early
                                if state.get_sync_state().await != SyncState::Syncing {
                                    log::info!("Processing tasks started");
                                    state.set_sync_state(SyncState::Syncing).await;
                                }
                            } else if line.contains("\"type\": \"cycleSuccess\"") {
                                // Sync cycle completed successfully
                                log::info!("Sync cycle completed");
                                state.set_sync_state(SyncState::Synced).await;
                                // Clear current file when sync completes
                                state.set_pending_count(0).await;
                            } else if line.contains("\"type\": \"cycleError\"") {
                                // Sync cycle had an error
                                log::error!("Sync cycle error");
                                state.set_sync_state(SyncState::Error).await;
                                state.set_pending_count(0).await;
                            } else if line.contains("\"count\":") {
                                // Parse pending count from deltasCount event
                                // The line looks like: "count": 5
                                let count_str: String = line
                                    .chars()
                                    .skip_while(|c| !c.is_ascii_digit())
                                    .take_while(|c| c.is_ascii_digit())
                                    .collect();
                                if let Ok(count) = count_str.parse::<u32>() {
                                    state.set_pending_count(count).await;
                                    if count > 0 {
                                        log::info!("Syncing {} files", count);
                                        state.set_sync_state(SyncState::Syncing).await;
                                    }
                                }
                            } else if line.contains("\"type\": \"transfer\"") {
                                // A transfer event - ensure we show Syncing
                                if state.get_sync_state().await != SyncState::Syncing {
                                    log::info!("File transfer in progress");
                                    state.set_sync_state(SyncState::Syncing).await;
                                }
                            } else if line.contains("\"type\": \"success\"") {
                                // A file transfer completed successfully - decrement pending count
                                let current = state.get_pending_count().await;
                                if current > 0 {
                                    let new_count = current - 1;
                                    log::info!("Transfer complete, {} files remaining", new_count);
                                    state.set_pending_count(new_count).await;
                                }
                            } else if line.contains("\"type\": \"uploadProgress\"")
                                || line.contains("\"type\": \"downloadProgress\"")
                            {
                                // Active file transfer - ensure we show Syncing
                                if state.get_sync_state().await != SyncState::Syncing {
                                    log::info!("File transfer in progress");
                                    state.set_sync_state(SyncState::Syncing).await;
                                }
                            }
                            // Fallback for non-verbose mode or text output
                            else if line.starts_with("Done syncing") {
                                if state.get_sync_state().await != SyncState::Synced {
                                    log::info!("Sync completed (text)");
                                    state.set_sync_state(SyncState::Synced).await;
                                    state.set_pending_count(0).await;
                                }
                            } else if line.starts_with("Syncing ")
                                && !line.contains("{")
                                && state.get_sync_state().await != SyncState::Syncing
                            {
                                state.set_sync_state(SyncState::Syncing).await;
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
                    // Check for error patterns
                    if line.contains("error") || line.contains("Error") || line.contains("failed") {
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
    pub async fn sync_once(&self, config: &Config) -> Result<(), CliError> {
        let sync_pair = format!(
            "{}:{}:{}",
            config.local_path.display(),
            config.sync_mode,
            config.remote_path
        );

        log::info!("Running one-shot sync: {}", sync_pair);
        self.state.set_sync_state(SyncState::Syncing).await;

        let filen_cmd = get_filen_command();
        let output = Command::new(&filen_cmd)
            .arg("sync")
            .arg(&sync_pair)
            .output()
            .await?;

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
