//! Linux tray implementation using ksni for native KDE/freedesktop StatusNotifierItem support

use super::{TrayAction, TrayInterface};
use crate::state::{CurrentTransfer, SyncState};
use ksni::{Tray, TrayMethods};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Shared state for the Linux tray
struct LinuxTrayState {
    sync_state: SyncState,
    status_text: String,
    pending_count: u32,
    /// Login state: None = starting/unknown, Some(true) = logged in, Some(false) = not logged in
    login_state: Option<bool>,
    /// Current transfer display text (None = hidden)
    current_transfer_text: Option<String>,
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
    fn update_icon(&self, state: SyncState) {
        if let Ok(mut s) = self.state.write() {
            s.sync_state = state;
        }
        self.trigger_update();
    }

    fn update_status(&self, text: &str) {
        if let Ok(mut s) = self.state.write() {
            s.status_text = text.to_string();
        }
        self.trigger_update();
    }

    fn update_storage(&self, _text: &str) {
        // Storage not supported by CLI, ignore (matches macOS behavior)
    }

    fn set_login_state(&self, login_state: Option<bool>) {
        if let Ok(mut s) = self.state.write() {
            s.login_state = login_state;
        }
        self.trigger_update();
    }

    fn update_pending_count(&self, count: u32) {
        if let Ok(mut s) = self.state.write() {
            s.pending_count = count;
        }
        self.trigger_update();
    }

    fn update_current_transfer(&self, transfer: Option<&CurrentTransfer>) {
        let new_text = transfer.map(|t| t.display_text(25));
        if let Ok(mut s) = self.state.write() {
            s.current_transfer_text = new_text;
        }
        self.trigger_update();
    }
}

/// The ksni Tray implementation
struct FilenTray {
    state: Arc<RwLock<LinuxTrayState>>,
}

impl Tray for FilenTray {
    fn icon_name(&self) -> String {
        // Use freedesktop icon theme name
        // You can also use icon_pixmap() for embedded icons
        let state = self.state.read().map(|s| s.sync_state).unwrap_or_default();
        match state {
            SyncState::Starting
            | SyncState::Synced
            | SyncState::NotLoggedIn
            | SyncState::Paused => "folder-sync".to_string(),
            SyncState::Syncing => "folder-sync".to_string(),
            SyncState::Error | SyncState::CliNotFound => "dialog-error".to_string(),
        }
    }

    fn title(&self) -> String {
        rust_i18n::t!("tooltip.app_name").to_string()
    }

    fn id(&self) -> String {
        "filen-menubar".to_string()
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
        let login_state = state.as_ref().and_then(|s| s.login_state);
        let current_transfer_text = state.as_ref().and_then(|s| s.current_transfer_text.clone());

        // Pending count text (matches macOS behavior)
        // During Scanning/Starting, we don't know the pending count yet
        let pending_text = if sync_state == SyncState::Scanning || sync_state == SyncState::Starting
        {
            rust_i18n::t!("menu.checking").to_string()
        } else if pending_count > 0 {
            if pending_count == 1 {
                rust_i18n::t!("menu.file_remaining").to_string()
            } else {
                rust_i18n::t!("menu.files_remaining", count = pending_count).to_string()
            }
        } else {
            rust_i18n::t!("menu.up_to_date").to_string()
        };

        let state_clone = self.state.clone();
        let state_clone2 = self.state.clone();
        let state_clone3 = self.state.clone();
        let state_clone4 = self.state.clone();
        let state_clone5 = self.state.clone();
        let state_clone6 = self.state.clone();
        let state_clone7 = self.state.clone();

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
        login_state: None, // Starting state - unknown login status
        current_transfer_text: None,
        action_tx,
    }));

    let tray = FilenTray {
        state: state.clone(),
    };

    // ksni 0.3 API: spawn() is now a method on the Tray trait via TrayMethods
    let handle = tray.spawn().await?;

    Ok(Arc::new(LinuxTray { state, handle }))
}
