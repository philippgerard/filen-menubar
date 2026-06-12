use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

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
    /// Scanning local and remote file trees to find changes
    Scanning,
    /// Currently syncing files
    Syncing,
    /// Sync paused
    Paused,
    /// Error occurred during sync
    Error,
    /// Filen CLI not found/not installed
    CliNotFound,
    /// No internet connection
    Offline,
}

/// Error returned when an invalid state transition is attempted
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: SyncState,
    pub to: SyncState,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid state transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for InvalidTransition {}

impl SyncState {
    /// Returns the valid states this state can transition to.
    ///
    /// This defines the state machine transitions:
    /// ```text
    /// Starting → Scanning, Syncing, Synced, NotLoggedIn, CliNotFound
    /// NotLoggedIn → Scanning, Syncing
    /// Scanning → Syncing, Synced, Error, Offline, Paused
    /// Syncing → Synced, Scanning, Error, Offline, Paused
    /// Synced → Scanning, Syncing, NotLoggedIn, Paused
    /// Paused → Scanning, Syncing, NotLoggedIn
    /// Error → Scanning, NotLoggedIn, CliNotFound, Paused
    /// CliNotFound → Starting (retry)
    /// Offline → Scanning, Syncing, Error, Paused (retry when online)
    /// ```
    pub fn valid_transitions(&self) -> &'static [SyncState] {
        match self {
            // Starting can go to scanning (normal start), not logged in, or CLI not found
            SyncState::Starting => &[
                SyncState::Scanning,
                SyncState::Syncing,
                SyncState::Synced,
                SyncState::NotLoggedIn,
                SyncState::CliNotFound,
            ],
            // Not logged in can start syncing after login
            SyncState::NotLoggedIn => &[SyncState::Scanning, SyncState::Syncing],
            // Scanning can find deltas (syncing), complete (synced), fail, or be paused
            SyncState::Scanning => &[
                SyncState::Syncing,
                SyncState::Synced,
                SyncState::Error,
                SyncState::Offline,
                SyncState::Paused,
            ],
            // Syncing can complete, scan again, fail, or be paused
            SyncState::Syncing => &[
                SyncState::Synced,
                SyncState::Scanning,
                SyncState::Error,
                SyncState::Offline,
                SyncState::Paused,
            ],
            // Synced can start new sync cycle, be paused, or logout
            SyncState::Synced => &[
                SyncState::Scanning,
                SyncState::Syncing,
                SyncState::NotLoggedIn,
                SyncState::Paused,
            ],
            // Paused can resume or logout
            SyncState::Paused => &[
                SyncState::Scanning,
                SyncState::Syncing,
                SyncState::NotLoggedIn,
            ],
            // Error can retry, be paused, or logout
            SyncState::Error => &[
                SyncState::Scanning,
                SyncState::NotLoggedIn,
                SyncState::CliNotFound,
                SyncState::Paused,
            ],
            // CLI not found can retry (goes back to starting to recheck)
            SyncState::CliNotFound => &[SyncState::Starting, SyncState::NotLoggedIn],
            // Offline can retry when online (scan for changes) or be paused
            SyncState::Offline => &[
                SyncState::Scanning,
                SyncState::Syncing,
                SyncState::Error,
                SyncState::Paused,
            ],
        }
    }

    /// Check if a transition to the given state is valid
    pub fn can_transition_to(&self, to: SyncState) -> bool {
        // Same state is always "valid" (no-op)
        if *self == to {
            return true;
        }
        self.valid_transitions().contains(&to)
    }

    /// Attempt a state transition, returning an error if invalid.
    ///
    /// Note: Transitions to the same state are always allowed (no-op).
    pub fn try_transition(&self, to: SyncState) -> Result<SyncState, InvalidTransition> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(InvalidTransition { from: *self, to })
        }
    }

    /// Get the display text for the status menu item
    pub fn status_text(&self) -> String {
        match self {
            SyncState::Starting => rust_i18n::t!("status.starting").to_string(),
            SyncState::NotLoggedIn => rust_i18n::t!("status.not_logged_in").to_string(),
            SyncState::Synced => rust_i18n::t!("status.synced").to_string(),
            SyncState::Scanning => rust_i18n::t!("status.scanning").to_string(),
            SyncState::Syncing => rust_i18n::t!("status.syncing").to_string(),
            SyncState::Paused => rust_i18n::t!("status.paused").to_string(),
            SyncState::Error => rust_i18n::t!("status.error").to_string(),
            SyncState::CliNotFound => rust_i18n::t!("status.cli_not_found").to_string(),
            SyncState::Offline => rust_i18n::t!("status.offline").to_string(),
        }
    }

    /// Get the icon name suffix for this state
    #[allow(dead_code)]
    pub fn icon_suffix(&self) -> &'static str {
        match self {
            SyncState::Starting => "idle",
            SyncState::NotLoggedIn => "idle",
            SyncState::Synced => "idle",
            SyncState::Scanning => "syncing",
            SyncState::Syncing => "syncing",
            SyncState::Paused => "idle",
            SyncState::Error => "error",
            SyncState::CliNotFound => "error",
            SyncState::Offline => "idle",
        }
    }

    /// Check if this state represents an active sync operation
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        matches!(self, SyncState::Scanning | SyncState::Syncing)
    }

    /// Check if this state represents an error condition
    #[allow(dead_code)]
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            SyncState::Error | SyncState::CliNotFound | SyncState::Offline
        )
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

    /// Format for display: "↑ filename.pdf" or "↓ filename.pdf"
    /// Note: Percentage removed as Filen CLI doesn't report cumulative progress
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

        format!("{} {}", arrow, filename)
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

/// Snapshot of state changes for reactive UI updates
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    pub sync_state: SyncState,
    pub pending_count: u32,
    pub current_transfer: Option<CurrentTransfer>,
    /// When the last sync cycle completed successfully
    pub last_synced: Option<DateTime<Local>>,
}

impl Default for StateSnapshot {
    fn default() -> Self {
        Self {
            sync_state: SyncState::Starting,
            pending_count: 0,
            current_transfer: None,
            last_synced: None,
        }
    }
}

/// Application state shared across the app
#[derive(Debug, Clone)]
pub struct AppState {
    inner: Arc<RwLock<AppStateInner>>,
    /// Watch channel sender for notifying subscribers of state changes
    notify_tx: Arc<watch::Sender<StateSnapshot>>,
}

#[derive(Debug)]
struct AppStateInner {
    sync_state: SyncState,
    storage_info: StorageInfo,
    is_logged_in: bool,
    pending_count: u32,
    current_transfer: Option<CurrentTransfer>,
    last_synced: Option<DateTime<Local>>,
}

impl AppState {
    pub fn new() -> Self {
        let (notify_tx, _) = watch::channel(StateSnapshot::default());
        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                sync_state: SyncState::Starting,
                storage_info: StorageInfo::default(),
                is_logged_in: false,
                pending_count: 0,
                current_transfer: None,
                last_synced: None,
            })),
            notify_tx: Arc::new(notify_tx),
        }
    }

    /// Subscribe to state changes.
    ///
    /// Returns a watch receiver that will be notified whenever
    /// sync_state, pending_count, or current_transfer changes.
    /// The receiver can use `changed().await` to wait for updates.
    pub fn subscribe(&self) -> watch::Receiver<StateSnapshot> {
        self.notify_tx.subscribe()
    }

    /// Notify subscribers of a state change
    fn notify(&self, inner: &AppStateInner) {
        let snapshot = StateSnapshot {
            sync_state: inner.sync_state,
            pending_count: inner.pending_count,
            current_transfer: inner.current_transfer.clone(),
            last_synced: inner.last_synced,
        };
        // Ignore send errors (no receivers)
        let _ = self.notify_tx.send(snapshot);
    }

    pub async fn get_sync_state(&self) -> SyncState {
        self.inner.read().await.sync_state
    }

    /// Set the sync state directly (for backwards compatibility).
    ///
    /// Prefer using `transition_to()` for validated state changes with logging.
    pub async fn set_sync_state(&self, state: SyncState) {
        let mut inner = self.inner.write().await;
        if inner.sync_state != state {
            log::debug!("State change: {:?} -> {:?}", inner.sync_state, state);
            inner.sync_state = state;
            self.notify(&inner);
        }
    }

    /// Attempt a validated state transition with logging.
    ///
    /// This method validates that the transition is allowed according to the
    /// state machine rules defined in `SyncState::valid_transitions()`.
    ///
    /// Returns `Ok(new_state)` if the transition was successful, or
    /// `Err(InvalidTransition)` if the transition is not allowed.
    ///
    /// Same-state transitions are always allowed (no-op).
    #[allow(dead_code)]
    pub async fn transition_to(
        &self,
        new_state: SyncState,
    ) -> Result<SyncState, InvalidTransition> {
        let mut inner = self.inner.write().await;
        let current = inner.sync_state;

        // Same state is a no-op
        if current == new_state {
            return Ok(current);
        }

        // Validate the transition
        let validated = current.try_transition(new_state)?;

        log::info!("State transition: {:?} -> {:?}", current, validated);
        inner.sync_state = validated;
        self.notify(&inner);

        Ok(validated)
    }

    /// Transition to a new state, logging a warning if the transition is invalid.
    ///
    /// This is a more permissive version of `transition_to()` that always
    /// allows the transition but logs a warning for invalid ones.
    /// Useful during refactoring when we want to track invalid transitions
    /// without breaking functionality.
    #[allow(dead_code)]
    pub async fn transition_to_unchecked(&self, new_state: SyncState) {
        let mut inner = self.inner.write().await;
        let current = inner.sync_state;

        if current == new_state {
            return;
        }

        if !current.can_transition_to(new_state) {
            log::warn!(
                "Potentially invalid state transition: {:?} -> {:?}",
                current,
                new_state
            );
        } else {
            log::debug!("State transition: {:?} -> {:?}", current, new_state);
        }

        inner.sync_state = new_state;
        self.notify(&inner);
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
            self.notify(&inner);
        }
    }

    pub async fn get_pending_count(&self) -> u32 {
        self.inner.read().await.pending_count
    }

    pub async fn set_pending_count(&self, count: u32) {
        let mut inner = self.inner.write().await;
        if inner.pending_count != count {
            inner.pending_count = count;
            self.notify(&inner);
        }
    }

    #[allow(dead_code)]
    pub async fn get_current_transfer(&self) -> Option<CurrentTransfer> {
        self.inner.read().await.current_transfer.clone()
    }

    pub async fn set_current_transfer(&self, transfer: Option<CurrentTransfer>) {
        let mut inner = self.inner.write().await;
        inner.current_transfer = transfer;
        self.notify(&inner);
    }

    /// Record that a sync cycle just completed successfully
    pub async fn set_last_synced_now(&self) {
        let mut inner = self.inner.write().await;
        inner.last_synced = Some(Local::now());
        self.notify(&inner);
    }

    #[allow(dead_code)]
    pub async fn get_last_synced(&self) -> Option<DateTime<Local>> {
        self.inner.read().await.last_synced
    }

    /// Update progress of the current transfer (bytes transferred)
    #[allow(dead_code)]
    pub async fn update_transfer_progress(&self, bytes: u64) {
        let mut inner = self.inner.write().await;
        if let Some(ref mut transfer) = inner.current_transfer {
            transfer.bytes = bytes;
            self.notify(&inner);
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
    fn test_display_text_no_percentage() {
        // Percentage was removed since Filen CLI doesn't report cumulative progress
        let mut transfer =
            CurrentTransfer::new(TransferDirection::Upload, "file.txt".to_string(), 100);
        transfer.bytes = 45;
        let text = transfer.display_text(50);
        // Should NOT contain percentage
        assert!(!text.contains("%"));
        // Should still contain the filename
        assert!(text.contains("file.txt"));
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
            SyncState::Scanning,
            SyncState::Syncing,
            SyncState::Paused,
            SyncState::Error,
            SyncState::CliNotFound,
            SyncState::Offline,
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
            SyncState::Offline,
        ];
        for state in idle_states {
            assert_eq!(
                state.icon_suffix(),
                "idle",
                "Expected 'idle' for {:?}",
                state
            );
        }

        assert_eq!(SyncState::Scanning.icon_suffix(), "syncing");
        assert_eq!(SyncState::Syncing.icon_suffix(), "syncing");
        assert_eq!(SyncState::Error.icon_suffix(), "error");
        assert_eq!(SyncState::CliNotFound.icon_suffix(), "error");
    }

    // ==================== State Machine tests ====================

    #[test]
    fn test_starting_valid_transitions() {
        let state = SyncState::Starting;
        assert!(state.can_transition_to(SyncState::Scanning));
        assert!(state.can_transition_to(SyncState::NotLoggedIn));
        assert!(state.can_transition_to(SyncState::CliNotFound));
        // Same state is always valid
        assert!(state.can_transition_to(SyncState::Starting));
    }

    #[test]
    fn test_starting_invalid_transition() {
        let state = SyncState::Starting;
        // Cannot go directly to Error or Offline from Starting
        assert!(!state.can_transition_to(SyncState::Error));
        assert!(!state.can_transition_to(SyncState::Offline));
        assert!(!state.can_transition_to(SyncState::Paused));
    }

    #[test]
    fn test_scanning_valid_transitions() {
        let state = SyncState::Scanning;
        assert!(state.can_transition_to(SyncState::Syncing));
        assert!(state.can_transition_to(SyncState::Synced));
        assert!(state.can_transition_to(SyncState::Error));
        assert!(state.can_transition_to(SyncState::Offline));
    }

    #[test]
    fn test_syncing_valid_transitions() {
        let state = SyncState::Syncing;
        assert!(state.can_transition_to(SyncState::Synced));
        assert!(state.can_transition_to(SyncState::Scanning));
        assert!(state.can_transition_to(SyncState::Error));
        assert!(state.can_transition_to(SyncState::Offline));
    }

    #[test]
    fn test_synced_valid_transitions() {
        let state = SyncState::Synced;
        assert!(state.can_transition_to(SyncState::Scanning));
        assert!(state.can_transition_to(SyncState::Syncing));
        assert!(state.can_transition_to(SyncState::NotLoggedIn));
        assert!(state.can_transition_to(SyncState::Paused));
    }

    #[test]
    fn test_offline_can_retry() {
        let state = SyncState::Offline;
        assert!(state.can_transition_to(SyncState::Scanning));
    }

    #[test]
    fn test_try_transition_success() {
        let state = SyncState::Starting;
        let result = state.try_transition(SyncState::Scanning);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), SyncState::Scanning);
    }

    #[test]
    fn test_try_transition_failure() {
        let state = SyncState::Starting;
        let result = state.try_transition(SyncState::Paused);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.from, SyncState::Starting);
        assert_eq!(err.to, SyncState::Paused);
    }

    #[test]
    fn test_try_transition_same_state() {
        let state = SyncState::Syncing;
        let result = state.try_transition(SyncState::Syncing);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), SyncState::Syncing);
    }

    #[test]
    fn test_invalid_transition_display() {
        let err = InvalidTransition {
            from: SyncState::Starting,
            to: SyncState::Paused,
        };
        let display = format!("{}", err);
        assert!(display.contains("Starting"));
        assert!(display.contains("Paused"));
    }

    #[test]
    fn test_is_active() {
        assert!(SyncState::Scanning.is_active());
        assert!(SyncState::Syncing.is_active());
        assert!(!SyncState::Synced.is_active());
        assert!(!SyncState::Starting.is_active());
        assert!(!SyncState::Error.is_active());
    }

    #[test]
    fn test_is_error() {
        assert!(SyncState::Error.is_error());
        assert!(SyncState::CliNotFound.is_error());
        assert!(SyncState::Offline.is_error());
        assert!(!SyncState::Synced.is_error());
        assert!(!SyncState::Syncing.is_error());
    }

    #[test]
    fn test_all_states_have_valid_transitions() {
        // Every state should have at least one valid transition (besides itself)
        let all_states = [
            SyncState::Starting,
            SyncState::NotLoggedIn,
            SyncState::Synced,
            SyncState::Scanning,
            SyncState::Syncing,
            SyncState::Paused,
            SyncState::Error,
            SyncState::CliNotFound,
            SyncState::Offline,
        ];

        for state in all_states {
            let transitions = state.valid_transitions();
            assert!(
                !transitions.is_empty(),
                "{:?} should have at least one valid transition",
                state
            );
        }
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

    #[tokio::test]
    async fn test_transition_to_valid() {
        let state = AppState::new();
        // Starting -> Scanning is valid
        let result = state.transition_to(SyncState::Scanning).await;
        assert!(result.is_ok());
        assert_eq!(state.get_sync_state().await, SyncState::Scanning);
    }

    #[tokio::test]
    async fn test_transition_to_invalid() {
        let state = AppState::new();
        // Starting -> Paused is invalid
        let result = state.transition_to(SyncState::Paused).await;
        assert!(result.is_err());
        // State should remain Starting
        assert_eq!(state.get_sync_state().await, SyncState::Starting);
    }

    #[tokio::test]
    async fn test_transition_to_same_state() {
        let state = AppState::new();
        // Same state transitions are no-ops
        let result = state.transition_to(SyncState::Starting).await;
        assert!(result.is_ok());
        assert_eq!(state.get_sync_state().await, SyncState::Starting);
    }

    #[tokio::test]
    async fn test_transition_to_unchecked_allows_invalid() {
        let state = AppState::new();
        // Invalid transition but should still work (with warning logged)
        state.transition_to_unchecked(SyncState::Paused).await;
        assert_eq!(state.get_sync_state().await, SyncState::Paused);
    }

    // ==================== Watch channel tests ====================

    #[tokio::test]
    async fn test_subscribe_receives_initial_state() {
        let state = AppState::new();
        let rx = state.subscribe();
        let snapshot = rx.borrow();
        assert_eq!(snapshot.sync_state, SyncState::Starting);
        assert_eq!(snapshot.pending_count, 0);
        assert!(snapshot.current_transfer.is_none());
    }

    #[tokio::test]
    async fn test_subscribe_notified_on_sync_state_change() {
        let state = AppState::new();
        let mut rx = state.subscribe();

        // Change state
        state.set_sync_state(SyncState::Scanning).await;

        // Should be notified
        assert!(rx.changed().await.is_ok());
        assert_eq!(rx.borrow().sync_state, SyncState::Scanning);
    }

    #[tokio::test]
    async fn test_subscribe_notified_on_pending_count_change() {
        let state = AppState::new();
        let mut rx = state.subscribe();

        // Change pending count
        state.set_pending_count(5).await;

        // Should be notified
        assert!(rx.changed().await.is_ok());
        assert_eq!(rx.borrow().pending_count, 5);
    }

    #[tokio::test]
    async fn test_subscribe_not_notified_on_same_value() {
        let state = AppState::new();
        let mut rx = state.subscribe();

        // Set same state (Starting -> Starting)
        state.set_sync_state(SyncState::Starting).await;

        // Should NOT be notified (use timeout to verify)
        let result = tokio::time::timeout(std::time::Duration::from_millis(50), rx.changed()).await;

        // Should timeout because no notification was sent
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let state = AppState::new();
        let mut rx1 = state.subscribe();
        let mut rx2 = state.subscribe();

        // Change state
        state.set_sync_state(SyncState::Synced).await;

        // Both should be notified
        assert!(rx1.changed().await.is_ok());
        assert!(rx2.changed().await.is_ok());
        assert_eq!(rx1.borrow().sync_state, SyncState::Synced);
        assert_eq!(rx2.borrow().sync_state, SyncState::Synced);
    }

    #[test]
    fn test_state_snapshot_default() {
        let snapshot = StateSnapshot::default();
        assert_eq!(snapshot.sync_state, SyncState::Starting);
        assert_eq!(snapshot.pending_count, 0);
        assert!(snapshot.current_transfer.is_none());
    }
}
