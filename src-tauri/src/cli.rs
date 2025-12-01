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
        (PathBuf::from("/usr/local/bin/filen"), Some(PathBuf::from("/usr/local/bin"))),
        (PathBuf::from("/opt/homebrew/bin/filen"), Some(PathBuf::from("/opt/homebrew/bin"))),
        // User local bin
        (home.join(".local/bin/filen"), Some(home.join(".local/bin"))),
        // npm global installs
        (home.join(".npm/bin/filen"), Some(home.join(".npm/bin"))),
        (home.join(".npm-global/bin/filen"), Some(home.join(".npm-global/bin"))),
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

/// Build a PATH environment variable that includes the given bin directory
/// along with essential system paths
fn build_path_env(bin_dir: &std::path::Path) -> String {
    let system_paths = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";
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
        let cli_info = find_filen_cli();
        log::debug!("Checking filen CLI availability at: {}", cli_info.command);

        let mut cmd = Command::new(&cli_info.command);
        cmd.arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Set PATH if we found a specific installation (needed for node-based CLI)
        if let Some(ref path_env) = cli_info.path_env {
            cmd.env("PATH", path_env);
        }

        cmd.status()
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
