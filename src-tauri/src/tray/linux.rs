//! Linux tray implementation using ksni for native KDE/freedesktop StatusNotifierItem support

use super::{
    get_pending_text, pause_resume_enabled, pause_resume_label, TrayAction, TrayInterface,
};
use crate::state::{CurrentTransfer, SyncState};
use ksni::{Tray, TrayMethods};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Shared state for the Linux tray
struct LinuxTrayState {
    sync_state: SyncState,
    status_text: String,
    pending_count: u32,
    /// Animation frame for loading indicators (0, 1, 2)
    animation_frame: u8,
    /// Login state: None = starting/unknown, Some(true) = logged in, Some(false) = not logged in
    login_state: Option<bool>,
    /// Current transfer display text (None = hidden)
    current_transfer_text: Option<String>,
    /// Pre-formatted time of the last successful sync (None = unknown)
    last_synced_text: Option<String>,
    action_tx: mpsc::UnboundedSender<TrayAction>,
}

/// Linux tray using ksni
pub struct LinuxTray {
    state: Arc<RwLock<LinuxTrayState>>,
    handle: ksni::Handle<FilenTray>,
}

impl LinuxTray {
    /// Trigger a tray update by spawning the async update call
    fn trigger_update(&self) {
        let handle = self.handle.clone();
        // Spawn the async update call - ksni's update() is async and must be awaited
        // for the D-Bus signals to be emitted
        tauri::async_runtime::spawn(async move {
            handle.update(|_| {}).await;
        });
    }
}

impl TrayInterface for LinuxTray {
    fn update_icon(&self, state: SyncState, animation_frame: u8) {
        // Every trigger_update() is a D-Bus round trip, so only fire one when
        // something visible changed. The theme icon ignores the animation
        // frame; on Linux the frame only drives the animated dots in the
        // menu's pending line, which appear during Scanning/Starting.
        let changed = if let Ok(mut s) = self.state.write() {
            let state_changed = s.sync_state != state;
            let frame_changed = s.animation_frame != animation_frame;
            s.sync_state = state;
            s.animation_frame = animation_frame;
            let dots_animating = matches!(state, SyncState::Starting | SyncState::Scanning);
            state_changed || (dots_animating && frame_changed)
        } else {
            false
        };
        if changed {
            self.trigger_update();
        }
    }

    fn update_status(&self, text: &str) {
        let changed = if let Ok(mut s) = self.state.write() {
            if s.status_text != text {
                s.status_text = text.to_string();
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.trigger_update();
        }
    }

    fn update_storage(&self, _text: &str) {
        // Storage not supported by CLI, ignore (matches macOS behavior)
    }

    fn set_login_state(&self, login_state: Option<bool>) {
        let changed = if let Ok(mut s) = self.state.write() {
            if s.login_state != login_state {
                s.login_state = login_state;
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.trigger_update();
        }
    }

    fn update_pending_count(&self, count: u32) {
        let changed = if let Ok(mut s) = self.state.write() {
            if s.pending_count != count {
                s.pending_count = count;
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.trigger_update();
        }
    }

    fn update_current_transfer(&self, transfer: Option<&CurrentTransfer>) {
        let new_text = transfer.map(|t| t.display_text(25));
        let changed = if let Ok(mut s) = self.state.write() {
            if s.current_transfer_text != new_text {
                s.current_transfer_text = new_text;
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.trigger_update();
        }
    }

    fn update_last_synced(&self, time_text: Option<&str>) {
        let new_text = time_text.map(|t| t.to_string());
        let changed = if let Ok(mut s) = self.state.write() {
            if s.last_synced_text != new_text {
                s.last_synced_text = new_text;
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.trigger_update();
        }
    }
}

/// The ksni Tray implementation
struct FilenTray {
    state: Arc<RwLock<LinuxTrayState>>,
}

impl Tray for FilenTray {
    fn icon_name(&self) -> String {
        // Freedesktop theme icon names: these follow the user's icon theme
        // (color, style, light/dark panel) which custom pixmaps cannot do
        let state = self.state.read().map(|s| s.sync_state).unwrap_or_default();
        match state {
            SyncState::Error | SyncState::CliNotFound => "dialog-error".to_string(),
            _ => "folder-sync".to_string(),
        }
    }

    fn title(&self) -> String {
        rust_i18n::t!("tooltip.app_name").to_string()
    }

    fn id(&self) -> String {
        "filen-menubar".to_string()
    }

    fn status(&self) -> ksni::Status {
        // NeedsAttention lets the host highlight the icon on errors and is
        // announced by assistive technology
        let state = self.state.read().map(|s| s.sync_state).unwrap_or_default();
        match state {
            SyncState::Error | SyncState::CliNotFound => ksni::Status::NeedsAttention,
            _ => ksni::Status::Active,
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let (status_text, sync_state, pending_count, animation_frame, last_synced) = self
            .state
            .read()
            .map(|s| {
                (
                    s.status_text.clone(),
                    s.sync_state,
                    s.pending_count,
                    s.animation_frame,
                    s.last_synced_text.clone(),
                )
            })
            .unwrap_or_else(|_| ("Unknown".to_string(), SyncState::Starting, 0, 0, None));

        // Same wording as the macOS tooltip
        let title = if pending_count > 0 {
            if pending_count == 1 {
                rust_i18n::t!("tooltip.syncing_file").to_string()
            } else {
                rust_i18n::t!("tooltip.syncing_files", count = pending_count).to_string()
            }
        } else {
            rust_i18n::t!("tooltip.status", status = &status_text).to_string()
        };

        ksni::ToolTip {
            title,
            description: get_pending_text(
                sync_state,
                pending_count,
                animation_frame,
                last_synced.as_deref(),
            ),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let state = self.state.read().ok();
        let status_text = state
            .as_ref()
            .map(|s| s.status_text.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let sync_state = state.as_ref().map(|s| s.sync_state).unwrap_or_default();
        let pending_count = state.as_ref().map(|s| s.pending_count).unwrap_or(0);
        let animation_frame = state.as_ref().map(|s| s.animation_frame).unwrap_or(0);
        let login_state = state.as_ref().and_then(|s| s.login_state);
        let current_transfer_text = state.as_ref().and_then(|s| s.current_transfer_text.clone());
        let last_synced_text = state.as_ref().and_then(|s| s.last_synced_text.clone());

        // Get pending count text using shared helper function
        let pending_text = get_pending_text(
            sync_state,
            pending_count,
            animation_frame,
            last_synced_text.as_deref(),
        );

        let state_clone = self.state.clone();
        let state_clone2 = self.state.clone();
        let state_clone3 = self.state.clone();
        let state_clone4 = self.state.clone();
        let state_clone5 = self.state.clone();
        let state_clone6 = self.state.clone();
        let state_clone7 = self.state.clone();
        let state_clone8 = self.state.clone();
        let state_clone9 = self.state.clone();
        let state_clone10 = self.state.clone();

        let mut items = vec![
            // Status (disabled, just for display)
            StandardItem {
                label: rust_i18n::t!("menu.status", status = &status_text).to_string(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            // Pending count (disabled, just for display)
            StandardItem {
                label: pending_text,
                enabled: false,
                ..Default::default()
            }
            .into(),
        ];

        // Current transfer (only shown when there's an active transfer)
        if let Some(transfer_text) = current_transfer_text {
            items.push(
                StandardItem {
                    label: transfer_text,
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        items.push(MenuItem::Separator);

        items.extend(vec![
            // Open Local Folder (enabled only when logged in)
            StandardItem {
                label: rust_i18n::t!("menu.open_local_folder").to_string(),
                enabled: login_state == Some(true),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone.read() {
                        let _ = s.action_tx.send(TrayAction::OpenFolder);
                    }
                }),
                ..Default::default()
            }
            .into(),
            // Open Web UI
            StandardItem {
                label: rust_i18n::t!("menu.open_web_ui").to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone2.read() {
                        let _ = s.action_tx.send(TrayAction::OpenWebUI);
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ]);

        // Pause/Resume syncing
        items.push(
            StandardItem {
                label: pause_resume_label(sync_state),
                enabled: pause_resume_enabled(sync_state, login_state),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone9.read() {
                        let _ = s.action_tx.send(TrayAction::TogglePause);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        // Login/Logout based on state (hidden when None/starting)
        match login_state {
            Some(true) => {
                items.push(
                    StandardItem {
                        label: rust_i18n::t!("menu.logout").to_string(),
                        activate: Box::new(move |_| {
                            if let Ok(s) = state_clone3.read() {
                                let _ = s.action_tx.send(TrayAction::Logout);
                            }
                        }),
                        ..Default::default()
                    }
                    .into(),
                );
            }
            Some(false) => {
                items.push(
                    StandardItem {
                        label: rust_i18n::t!("menu.login").to_string(),
                        activate: Box::new(move |_| {
                            if let Ok(s) = state_clone4.read() {
                                let _ = s.action_tx.send(TrayAction::Login);
                            }
                        }),
                        ..Default::default()
                    }
                    .into(),
                );
            }
            None => {
                // Starting state - hide both Login and Logout buttons
                // Suppress unused variable warnings
                let _ = state_clone3;
                let _ = state_clone4;
            }
        }

        items.push(MenuItem::Separator);

        // Settings
        items.push(
            StandardItem {
                label: rust_i18n::t!("menu.settings").to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone5.read() {
                        let _ = s.action_tx.send(TrayAction::Settings);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        // Show Logs
        items.push(
            StandardItem {
                label: rust_i18n::t!("menu.show_logs").to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone6.read() {
                        let _ = s.action_tx.send(TrayAction::ShowLogs);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        // About
        items.push(
            StandardItem {
                label: rust_i18n::t!("menu.about").to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone8.read() {
                        let _ = s.action_tx.send(TrayAction::About);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        // Check for Updates
        items.push(
            StandardItem {
                label: rust_i18n::t!("menu.check_for_updates").to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone10.read() {
                        let _ = s.action_tx.send(TrayAction::CheckForUpdates);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        // Quit
        items.push(
            StandardItem {
                label: rust_i18n::t!("menu.quit").to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone7.read() {
                        let _ = s.action_tx.send(TrayAction::Quit);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

/// Create the tray icon for Linux using ksni
pub async fn create_tray(
    _app: &tauri::AppHandle,
    action_tx: mpsc::UnboundedSender<TrayAction>,
) -> Result<Arc<dyn TrayInterface>, Box<dyn std::error::Error>> {
    let state = Arc::new(RwLock::new(LinuxTrayState {
        sync_state: SyncState::Starting,
        status_text: rust_i18n::t!("status.starting").to_string(),
        pending_count: 0,
        animation_frame: 0,
        login_state: None, // Starting state - unknown login status
        current_transfer_text: None,
        last_synced_text: None,
        action_tx,
    }));

    let tray = FilenTray {
        state: state.clone(),
    };

    // ksni 0.3 API: spawn() is now a method on the Tray trait via TrayMethods
    let handle = tray.spawn().await?;

    Ok(Arc::new(LinuxTray { state, handle }))
}
