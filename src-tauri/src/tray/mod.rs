//! Platform-specific tray implementations
//!
//! - macOS: Uses Tauri's built-in TrayIcon
//! - Linux: Uses ksni for native KDE/freedesktop StatusNotifierItem support
//!
//! This module also provides shared helper functions used by both platform implementations
//! to ensure consistent behavior across platforms.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod linux;

use crate::state::{CurrentTransfer, SyncState};

// ==================== Shared Helper Functions ====================

/// Get the animated dots for loading states.
///
/// Returns ".", "..", or "..." based on the animation frame,
/// creating a simple loading animation effect.
pub fn get_animated_dots(animation_frame: u8) -> &'static str {
    match animation_frame % 3 {
        0 => ".",
        1 => "..",
        _ => "...",
    }
}

/// Get the pending count text based on sync state and count.
///
/// This is the text displayed below the status line in the tray menu,
/// showing either:
/// - "No internet connection" when offline
/// - Animated dots during scanning/starting
/// - File count when syncing ("1 file remaining" or "X files remaining")
/// - "Up to date" when synced
pub fn get_pending_text(sync_state: SyncState, pending_count: u32, animation_frame: u8) -> String {
    // For Offline state, show "No internet connection"
    if sync_state == SyncState::Offline {
        return rust_i18n::t!("menu.no_internet").to_string();
    }

    // During Scanning/Starting, we don't know the pending count yet - show animated dots
    if sync_state == SyncState::Scanning || sync_state == SyncState::Starting {
        return get_animated_dots(animation_frame).to_string();
    }

    if pending_count > 0 {
        if pending_count == 1 {
            rust_i18n::t!("menu.file_remaining").to_string()
        } else {
            rust_i18n::t!("menu.files_remaining", count = pending_count).to_string()
        }
    } else {
        rust_i18n::t!("menu.up_to_date").to_string()
    }
}

/// Tray menu action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenFolder,
    OpenWebUI,
    Login,
    Logout,
    Settings,
    ShowLogs,
    About,
    Quit,
}

/// Platform-agnostic tray interface
pub trait TrayInterface: Send + Sync {
    /// Update the tray icon based on sync state
    /// `animation_frame` is used to animate loading indicators (cycles 0, 1, 2)
    fn update_icon(&self, state: SyncState, animation_frame: u8);

    /// Update the status text in the menu
    fn update_status(&self, text: &str);

    /// Update the pending file count (shown below status when syncing)
    fn update_pending_count(&self, count: u32);

    /// Update the storage info in the menu
    #[allow(dead_code)]
    fn update_storage(&self, text: &str);

    /// Set the login state
    /// - None: Starting/unknown (hide Login/Logout buttons)
    /// - Some(true): Logged in (show Logout button)
    /// - Some(false): Not logged in (show Login button)
    fn set_login_state(&self, state: Option<bool>);

    /// Set whether the user is logged in (affects menu items)
    /// Convenience method that calls set_login_state(Some(logged_in))
    #[allow(dead_code)]
    fn set_logged_in(&self, logged_in: bool) {
        self.set_login_state(Some(logged_in));
    }

    /// Update the current transfer display
    /// - None: No active transfer (hide the menu item)
    /// - Some(transfer): Show current file being transferred with progress
    fn update_current_transfer(&self, transfer: Option<&CurrentTransfer>);
}

#[cfg(target_os = "macos")]
pub use macos::create_tray;

#[cfg(target_os = "linux")]
pub use linux::create_tray;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_animated_dots_cycles() {
        assert_eq!(get_animated_dots(0), ".");
        assert_eq!(get_animated_dots(1), "..");
        assert_eq!(get_animated_dots(2), "...");
        // Should cycle back
        assert_eq!(get_animated_dots(3), ".");
        assert_eq!(get_animated_dots(4), "..");
        assert_eq!(get_animated_dots(5), "...");
    }

    #[test]
    fn test_get_pending_text_offline() {
        rust_i18n::set_locale("en");
        let text = get_pending_text(SyncState::Offline, 5, 0);
        // Should show offline message regardless of pending count
        assert!(!text.contains("5"));
    }

    #[test]
    fn test_get_pending_text_scanning_shows_dots() {
        let text = get_pending_text(SyncState::Scanning, 0, 0);
        assert_eq!(text, ".");
        let text = get_pending_text(SyncState::Scanning, 0, 1);
        assert_eq!(text, "..");
        let text = get_pending_text(SyncState::Scanning, 0, 2);
        assert_eq!(text, "...");
    }

    #[test]
    fn test_get_pending_text_starting_shows_dots() {
        let text = get_pending_text(SyncState::Starting, 0, 0);
        assert_eq!(text, ".");
    }

    #[test]
    fn test_get_pending_text_synced_zero_files() {
        rust_i18n::set_locale("en");
        let text = get_pending_text(SyncState::Synced, 0, 0);
        // Should show "up to date" message
        assert!(!text.contains("remaining"));
    }

    #[test]
    fn test_get_pending_text_syncing_with_files() {
        rust_i18n::set_locale("en");
        let text = get_pending_text(SyncState::Syncing, 5, 0);
        // Should contain some indication of files remaining
        assert!(text.len() > 0);
    }
}
