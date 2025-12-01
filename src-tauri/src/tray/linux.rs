//! Linux tray implementation using ksni for native KDE/freedesktop StatusNotifierItem support

use super::{TrayAction, TrayInterface};
use crate::state::SyncState;
use ksni::{Tray, TrayMethods};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Shared state for the Linux tray
struct LinuxTrayState {
    sync_state: SyncState,
    status_text: String,
    storage_text: String,
    pending_count: u32,
    logged_in: bool,
    action_tx: mpsc::UnboundedSender<TrayAction>,
}

/// Linux tray using ksni
pub struct LinuxTray {
    state: Arc<RwLock<LinuxTrayState>>,
    handle: ksni::Handle<FilenTray>,
}

impl TrayInterface for LinuxTray {
    fn update_icon(&self, state: SyncState) {
        if let Ok(mut s) = self.state.write() {
            s.sync_state = state;
        }
        self.handle.update(|_| {});
    }

    fn update_status(&self, text: &str) {
        if let Ok(mut s) = self.state.write() {
            s.status_text = text.to_string();
        }
        self.handle.update(|_| {});
    }

    fn update_storage(&self, text: &str) {
        if let Ok(mut s) = self.state.write() {
            s.storage_text = text.to_string();
        }
        self.handle.update(|_| {});
    }

    fn set_logged_in(&self, logged_in: bool) {
        if let Ok(mut s) = self.state.write() {
            s.logged_in = logged_in;
        }
        self.handle.update(|_| {});
    }

    fn update_pending_count(&self, count: u32) {
        if let Ok(mut s) = self.state.write() {
            s.pending_count = count;
        }
        self.handle.update(|_| {});
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
            SyncState::Synced | SyncState::NotLoggedIn | SyncState::Paused => {
                "folder-sync".to_string()
            }
            SyncState::Syncing => "folder-sync".to_string(),
            SyncState::Error => "dialog-error".to_string(),
        }
    }

    fn title(&self) -> String {
        "Filen Menubar".to_string()
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
        let storage_text = state
            .as_ref()
            .map(|s| s.storage_text.clone())
            .unwrap_or_else(|| "Storage: --".to_string());
        let logged_in = state.as_ref().map(|s| s.logged_in).unwrap_or(false);

        let state_clone = self.state.clone();
        let state_clone2 = self.state.clone();
        let state_clone3 = self.state.clone();
        let state_clone4 = self.state.clone();
        let state_clone5 = self.state.clone();
        let state_clone6 = self.state.clone();

        vec![
            // Status (disabled, just for display)
            StandardItem {
                label: format!("Status: {}", status_text),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            // Open Sync Folder
            StandardItem {
                label: "Open Sync Folder".to_string(),
                enabled: logged_in,
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone.read() {
                        let _ = s.action_tx.send(TrayAction::OpenFolder);
                    }
                }),
                ..Default::default()
            }
            .into(),
            // Sync Now
            StandardItem {
                label: "Sync Now".to_string(),
                enabled: logged_in,
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone2.read() {
                        let _ = s.action_tx.send(TrayAction::SyncNow);
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            // Storage info
            StandardItem {
                label: storage_text,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            // Login/Logout
            if logged_in {
                StandardItem {
                    label: "Logout".to_string(),
                    activate: Box::new(move |_| {
                        if let Ok(s) = state_clone3.read() {
                            let _ = s.action_tx.send(TrayAction::Logout);
                        }
                    }),
                    ..Default::default()
                }
                .into()
            } else {
                StandardItem {
                    label: "Login...".to_string(),
                    activate: Box::new(move |_| {
                        if let Ok(s) = state_clone4.read() {
                            let _ = s.action_tx.send(TrayAction::Login);
                        }
                    }),
                    ..Default::default()
                }
                .into()
            },
            MenuItem::Separator,
            // Settings
            StandardItem {
                label: "Settings...".to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone5.read() {
                        let _ = s.action_tx.send(TrayAction::Settings);
                    }
                }),
                ..Default::default()
            }
            .into(),
            // Quit
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(move |_| {
                    if let Ok(s) = state_clone6.read() {
                        let _ = s.action_tx.send(TrayAction::Quit);
                    }
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Create the tray icon for Linux using ksni
pub async fn create_tray(
    _app: &tauri::AppHandle,
    action_tx: mpsc::UnboundedSender<TrayAction>,
) -> Result<Arc<dyn TrayInterface>, Box<dyn std::error::Error>> {
    let state = Arc::new(RwLock::new(LinuxTrayState {
        sync_state: SyncState::NotLoggedIn,
        status_text: "Not Logged In".to_string(),
        storage_text: "Storage: --".to_string(),
        pending_count: 0,
        logged_in: false,
        action_tx,
    }));

    let tray = FilenTray {
        state: state.clone(),
    };

    // ksni 0.3 API: spawn() is now a method on the Tray trait via TrayMethods
    let handle = tray.spawn().await?;

    Ok(Arc::new(LinuxTray { state, handle }))
}
