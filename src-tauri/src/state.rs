use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Represents the current sync state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SyncState {
    /// Application is starting up
    #[default]
    Starting,
    /// Not logged in
    NotLoggedIn,
    /// Successfully synced, idle
    Synced,
    /// Currently syncing files
    Syncing,
    /// Sync paused
    Paused,
    /// Error occurred during sync
    Error,
    /// Filen CLI not found/not installed
    CliNotFound,
}

impl SyncState {
    /// Get the display text for the status menu item
    pub fn status_text(&self) -> String {
        match self {
            SyncState::Starting => rust_i18n::t!("status.starting").to_string(),
            SyncState::NotLoggedIn => rust_i18n::t!("status.not_logged_in").to_string(),
            SyncState::Synced => rust_i18n::t!("status.synced").to_string(),
            SyncState::Syncing => rust_i18n::t!("status.syncing").to_string(),
            SyncState::Paused => rust_i18n::t!("status.paused").to_string(),
            SyncState::Error => rust_i18n::t!("status.error").to_string(),
            SyncState::CliNotFound => rust_i18n::t!("status.cli_not_found").to_string(),
        }
    }

    /// Get the icon name suffix for this state
    #[allow(dead_code)]
    pub fn icon_suffix(&self) -> &'static str {
        match self {
            SyncState::Starting => "idle",
            SyncState::NotLoggedIn => "idle",
            SyncState::Synced => "idle",
            SyncState::Syncing => "syncing",
            SyncState::Paused => "idle",
            SyncState::Error => "error",
            SyncState::CliNotFound => "error",
        }
    }
}

/// Storage information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageInfo {
    /// Used storage in bytes
    pub used: u64,
    /// Total storage in bytes
    pub total: u64,
}

/// Direction of file transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// Current file transfer information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentTransfer {
    /// Upload or download
    pub direction: TransferDirection,
    /// Filename (extracted from path)
    pub filename: String,
    /// Bytes transferred so far
    pub bytes: u64,
    /// Total file size
    pub size: u64,
}

impl CurrentTransfer {
    /// Create a new transfer
    pub fn new(direction: TransferDirection, filename: String, size: u64) -> Self {
        Self {
            direction,
            filename,
            size,
            bytes: 0,
        }
    }

    /// Get progress as percentage (0-100)
    pub fn progress_percent(&self) -> u8 {
        if self.size == 0 {
            return 0;
        }
        ((self.bytes as f64 / self.size as f64) * 100.0).min(100.0) as u8
    }

    /// Format for display: "↑ filename.pdf (45%)" or "↓ filename.pdf (45%)"
    pub fn display_text(&self, max_filename_len: usize) -> String {
        let arrow = match self.direction {
            TransferDirection::Upload => "↑",
            TransferDirection::Download => "↓",
        };

        let filename = if self.filename.len() > max_filename_len {
            format!("{}…", &self.filename[..max_filename_len - 1])
        } else {
            self.filename.clone()
        };

        format!("{} {} ({}%)", arrow, filename, self.progress_percent())
    }
}

impl StorageInfo {
    /// Format storage as human-readable string
    #[allow(dead_code)]
    pub fn format(&self) -> String {
        let used_gb = self.used as f64 / 1_073_741_824.0;
        let total_gb = self.total as f64 / 1_073_741_824.0;
        format!("{:.1} / {:.1} GB", used_gb, total_gb)
    }
}

/// Application state shared across the app
#[derive(Debug, Clone)]
pub struct AppState {
    inner: Arc<RwLock<AppStateInner>>,
}

#[derive(Debug)]
struct AppStateInner {
    sync_state: SyncState,
    storage_info: StorageInfo,
    is_logged_in: bool,
    pending_count: u32,
    current_transfer: Option<CurrentTransfer>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                sync_state: SyncState::Starting,
                storage_info: StorageInfo::default(),
                is_logged_in: false,
                pending_count: 0,
                current_transfer: None,
            })),
        }
    }

    pub async fn get_sync_state(&self) -> SyncState {
        self.inner.read().await.sync_state
    }

    pub async fn set_sync_state(&self, state: SyncState) {
        self.inner.write().await.sync_state = state;
    }

    #[allow(dead_code)]
    pub async fn get_storage_info(&self) -> StorageInfo {
        self.inner.read().await.storage_info.clone()
    }

    #[allow(dead_code)]
    pub async fn set_storage_info(&self, info: StorageInfo) {
        self.inner.write().await.storage_info = info;
    }

    #[allow(dead_code)]
    pub async fn is_logged_in(&self) -> bool {
        self.inner.read().await.is_logged_in
    }

    pub async fn set_logged_in(&self, logged_in: bool) {
        let mut inner = self.inner.write().await;
        inner.is_logged_in = logged_in;
        if !logged_in {
            inner.sync_state = SyncState::NotLoggedIn;
        }
    }

    pub async fn get_pending_count(&self) -> u32 {
        self.inner.read().await.pending_count
    }

    pub async fn set_pending_count(&self, count: u32) {
        self.inner.write().await.pending_count = count;
    }

    pub async fn get_current_transfer(&self) -> Option<CurrentTransfer> {
        self.inner.read().await.current_transfer.clone()
    }

    pub async fn set_current_transfer(&self, transfer: Option<CurrentTransfer>) {
        self.inner.write().await.current_transfer = transfer;
    }

    /// Update progress of the current transfer (bytes transferred)
    #[allow(dead_code)]
    pub async fn update_transfer_progress(&self, bytes: u64) {
        let mut inner = self.inner.write().await;
        if let Some(ref mut transfer) = inner.current_transfer {
            transfer.bytes = bytes;
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
