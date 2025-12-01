//! Platform-specific tray implementations
//!
//! - macOS: Uses Tauri's built-in TrayIcon
//! - Linux: Uses ksni for native KDE/freedesktop StatusNotifierItem support

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod linux;

use crate::state::SyncState;

/// Tray menu action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenFolder,
    SyncNow,
    Login,
    Logout,
    Settings,
    Quit,
}

/// Platform-agnostic tray interface
pub trait TrayInterface: Send + Sync {
    /// Update the tray icon based on sync state
    fn update_icon(&self, state: SyncState);

    /// Update the status text in the menu
    fn update_status(&self, text: &str);

    /// Update the pending file count (shown below status when syncing)
    fn update_pending_count(&self, count: u32);

    /// Update the storage info in the menu
    #[allow(dead_code)]
    fn update_storage(&self, text: &str);

    /// Set whether the user is logged in (affects menu items)
    fn set_logged_in(&self, logged_in: bool);
}

#[cfg(target_os = "macos")]
pub use macos::create_tray;

#[cfg(target_os = "linux")]
pub use linux::create_tray;
