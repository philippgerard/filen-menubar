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

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== CurrentTransfer tests ====================

    #[test]
    fn test_progress_percent_zero_size_file() {
        let transfer = CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 0);
        assert_eq!(transfer.progress_percent(), 0);
    }

    #[test]
    fn test_progress_percent_partial() {
        let mut transfer =
            CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 1000);
        transfer.bytes = 500;
        assert_eq!(transfer.progress_percent(), 50);
    }

    #[test]
    fn test_progress_percent_complete() {
        let mut transfer =
            CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 1000);
        transfer.bytes = 1000;
        assert_eq!(transfer.progress_percent(), 100);
    }

    #[test]
    fn test_progress_percent_overflow_capped() {
        let mut transfer =
            CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 1000);
        transfer.bytes = 2000; // More than total size
        assert_eq!(transfer.progress_percent(), 100);
    }

    #[test]
    fn test_display_text_upload_arrow() {
        let transfer = CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 100);
        let text = transfer.display_text(50);
        assert!(text.starts_with("↑"));
    }

    #[test]
    fn test_display_text_download_arrow() {
        let transfer =
            CurrentTransfer::new(TransferDirection::Download, "file.txt".to_string(), 100);
        let text = transfer.display_text(50);
        assert!(text.starts_with("↓"));
    }

    #[test]
    fn test_display_text_truncation() {
        let transfer = CurrentTransfer::new(
            TransferDirection::Upload,
            "very_long_filename_that_exceeds_limit.txt".to_string(),
            100,
        );
        let text = transfer.display_text(10);
        assert!(text.contains("…"));
        // Truncated part should be shorter than original
        assert!(text.len() < "very_long_filename_that_exceeds_limit.txt".len() + 10);
    }

    #[test]
    fn test_display_text_short_filename_no_truncation() {
        let transfer = CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 100);
        let text = transfer.display_text(50);
        assert!(text.contains("file.txt"));
        assert!(!text.contains("…"));
    }

    #[test]
    fn test_display_text_percentage_format() {
        let mut transfer =
            CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 100);
        transfer.bytes = 45;
        let text = transfer.display_text(50);
        assert!(text.contains("(45%)"));
    }

    // ==================== SyncState tests ====================

    #[test]
    fn test_sync_state_status_text_not_empty() {
        // Initialize i18n for tests
        rust_i18n::set_locale("en");

        let states = [
            SyncState::Starting,
            SyncState::NotLoggedIn,
            SyncState::Synced,
            SyncState::Syncing,
            SyncState::Paused,
            SyncState::Error,
            SyncState::CliNotFound,
        ];

        for state in states {
            let text = state.status_text();
            assert!(
                !text.is_empty(),
                "Status text for {:?} should not be empty",
                state
            );
        }
    }

    #[test]
    fn test_sync_state_icon_suffix_valid_values() {
        let idle_states = [
            SyncState::Starting,
            SyncState::NotLoggedIn,
            SyncState::Synced,
            SyncState::Paused,
        ];
        for state in idle_states {
            assert_eq!(
                state.icon_suffix(),
                "idle",
                "Expected 'idle' for {:?}",
                state
            );
        }

        assert_eq!(SyncState::Syncing.icon_suffix(), "syncing");
        assert_eq!(SyncState::Error.icon_suffix(), "error");
        assert_eq!(SyncState::CliNotFound.icon_suffix(), "error");
    }

    // ==================== StorageInfo tests ====================

    #[test]
    fn test_storage_info_format_zero() {
        let info = StorageInfo { used: 0, total: 0 };
        assert_eq!(info.format(), "0.0 / 0.0 GB");
    }

    #[test]
    fn test_storage_info_format_gb() {
        let info = StorageInfo {
            used: 5_368_709_120,   // 5 GB
            total: 10_737_418_240, // 10 GB
        };
        assert_eq!(info.format(), "5.0 / 10.0 GB");
    }

    #[test]
    fn test_storage_info_format_fractional() {
        let info = StorageInfo {
            used: 1_610_612_736,  // 1.5 GB
            total: 3_221_225_472, // 3 GB
        };
        assert_eq!(info.format(), "1.5 / 3.0 GB");
    }

    // ==================== AppState async tests ====================

    #[tokio::test]
    async fn test_app_state_initial_values() {
        let state = AppState::new();
        assert_eq!(state.get_sync_state().await, SyncState::Starting);
        assert_eq!(state.get_pending_count().await, 0);
        assert!(state.get_current_transfer().await.is_none());
    }

    #[tokio::test]
    async fn test_set_logged_in_false_changes_state() {
        let state = AppState::new();
        state.set_sync_state(SyncState::Synced).await;
        state.set_logged_in(false).await;
        assert_eq!(state.get_sync_state().await, SyncState::NotLoggedIn);
    }

    #[tokio::test]
    async fn test_pending_count_roundtrip() {
        let state = AppState::new();
        state.set_pending_count(42).await;
        assert_eq!(state.get_pending_count().await, 42);
    }

    #[tokio::test]
    async fn test_current_transfer_roundtrip() {
        let state = AppState::new();
        let transfer =
            CurrentTransfer::new(TransferDirection::Upload, "document.pdf".to_string(), 1024);
        state.set_current_transfer(Some(transfer.clone())).await;

        let retrieved = state.get_current_transfer().await;
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.filename, "document.pdf");
        assert_eq!(retrieved.size, 1024);
    }

    #[tokio::test]
    async fn test_current_transfer_clear() {
        let state = AppState::new();
        let transfer = CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 100);
        state.set_current_transfer(Some(transfer)).await;
        state.set_current_transfer(None).await;
        assert!(state.get_current_transfer().await.is_none());
    }
}
