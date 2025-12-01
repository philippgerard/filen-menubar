//! macOS tray implementation using Tauri's TrayIcon

use super::{TrayAction, TrayInterface};
use crate::state::SyncState;
use std::sync::{Arc, RwLock};
use tauri::{
    menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle,
};
use tokio::sync::mpsc;

/// Shared state for menu updates
struct MenuState {
    logged_in: bool,
    status_text: String,
    pending_count: u32,
}

/// Stored menu item references for in-place updates
struct MenuItems {
    status_item: MenuItem<tauri::Wry>,
    pending_item: MenuItem<tauri::Wry>,
}

pub struct MacOsTray {
    app: AppHandle,
    tray: TrayIcon,
    state: Arc<RwLock<MenuState>>,
    menu_items: Arc<RwLock<MenuItems>>,
    #[allow(dead_code)]
    action_tx: mpsc::UnboundedSender<TrayAction>,
}

impl MacOsTray {
    /// Rebuild the menu with current state (only for structural changes like login/logout)
    fn rebuild_menu(&self) {
        let state = self.state.read().unwrap();

        if let Ok((menu, items)) = build_menu(
            &self.app,
            &state.status_text,
            state.pending_count,
            state.logged_in,
        ) {
            let _ = self.tray.set_menu(Some(menu));
            *self.menu_items.write().unwrap() = items;
        }
    }
}

impl TrayInterface for MacOsTray {
    fn update_icon(&self, _state: SyncState) {
        // TODO: Update icon based on state when we have proper icons
    }

    fn update_status(&self, text: &str) {
        {
            let mut state = self.state.write().unwrap();
            if state.status_text != text {
                state.status_text = text.to_string();
            } else {
                return; // No change
            }
        }

        // Update tooltip with current status
        let state = self.state.read().unwrap();
        let tooltip = if state.pending_count > 0 {
            format!("Filen - Syncing {} file{}", state.pending_count, if state.pending_count == 1 { "" } else { "s" })
        } else {
            format!("Filen - {}", text)
        };
        let _ = self.tray.set_tooltip(Some(&tooltip));

        // Update menu item text in-place (doesn't close menu)
        let items = self.menu_items.read().unwrap();
        let _ = items.status_item.set_text(format!("Status: {}", text));
    }

    fn update_pending_count(&self, count: u32) {
        {
            let mut state = self.state.write().unwrap();
            if state.pending_count != count {
                state.pending_count = count;
            } else {
                return; // No change
            }
        }

        // Update tooltip with current status (updates in real-time, even with menu open)
        let state = self.state.read().unwrap();
        let tooltip = if count > 0 {
            format!("Filen - Syncing {} file{}", count, if count == 1 { "" } else { "s" })
        } else {
            format!("Filen - {}", state.status_text)
        };
        let _ = self.tray.set_tooltip(Some(&tooltip));

        // Update menu item text in-place (doesn't close menu)
        let items = self.menu_items.read().unwrap();
        let pending_text = if count > 0 {
            if count == 1 {
                "1 file remaining...".to_string()
            } else {
                format!("{} files remaining...", count)
            }
        } else {
            "Up to date".to_string()
        };
        let _ = items.pending_item.set_text(&pending_text);
    }

    fn update_storage(&self, _text: &str) {
        // Storage not supported by CLI, ignore
    }

    fn set_logged_in(&self, logged_in: bool) {
        {
            let mut state = self.state.write().unwrap();
            if state.logged_in != logged_in {
                state.logged_in = logged_in;
            } else {
                return; // No change, don't rebuild
            }
        }
        // Login/logout changes menu structure, so we need to rebuild
        self.rebuild_menu();
    }
}

/// Build the tray menu with current state
fn build_menu(
    app: &AppHandle,
    status_text: &str,
    pending_count: u32,
    logged_in: bool,
) -> Result<(Menu<tauri::Wry>, MenuItems), Box<dyn std::error::Error>> {
    let mut builder = MenuBuilder::new(app);

    // Status (disabled, just for display)
    let status_item = MenuItemBuilder::with_id("status", format!("Status: {}", status_text))
        .enabled(false)
        .build(app)?;
    builder = builder.item(&status_item);

    // Pending file count (always present)
    let pending_text = if pending_count > 0 {
        if pending_count == 1 {
            "1 file remaining...".to_string()
        } else {
            format!("{} files remaining...", pending_count)
        }
    } else {
        "Up to date".to_string()
    };
    let pending_item = MenuItemBuilder::with_id("pending_count", &pending_text)
        .enabled(false)
        .build(app)?;
    builder = builder.item(&pending_item);

    builder = builder.separator();

    // Open Sync Folder
    let open_folder = MenuItemBuilder::with_id("open_folder", "Open Sync Folder")
        .enabled(logged_in)
        .build(app)?;
    builder = builder.item(&open_folder);

    // Open Web UI
    let open_web_ui = MenuItemBuilder::with_id("open_web_ui", "Open Web UI")
        .build(app)?;
    builder = builder.item(&open_web_ui);

    builder = builder.separator();

    // Login or Logout based on state
    if logged_in {
        let logout_item = MenuItemBuilder::with_id("logout", "Logout").build(app)?;
        builder = builder.item(&logout_item);
    } else {
        let login_item = MenuItemBuilder::with_id("login", "Login...").build(app)?;
        builder = builder.item(&login_item);
    }

    builder = builder.separator();

    // Settings
    let settings_item = MenuItemBuilder::with_id("settings", "Settings...").build(app)?;
    builder = builder.item(&settings_item);

    // Quit
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    builder = builder.item(&quit_item);

    let items = MenuItems {
        status_item,
        pending_item,
    };

    Ok((builder.build()?, items))
}

/// Create the tray icon and menu for macOS
pub fn create_tray(
    app: &AppHandle,
    action_tx: mpsc::UnboundedSender<TrayAction>,
) -> Result<Arc<dyn TrayInterface>, Box<dyn std::error::Error>> {
    let initial_state = MenuState {
        logged_in: false,
        status_text: "Not Logged In".to_string(),
        pending_count: 0,
    };

    // Build initial menu
    let (menu, menu_items) = build_menu(
        app,
        &initial_state.status_text,
        initial_state.pending_count,
        initial_state.logged_in,
    )?;

    // Create tray icon
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Filen Menubar");

    // Try to use the default window icon if available
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    // Enable template icon for macOS dark/light mode support
    builder = builder.icon_as_template(true);

    let tray = builder.build(app)?;

    // Handle menu events
    let action_tx_clone = action_tx.clone();
    app.on_menu_event(move |_app, event| {
        let action = match event.id().as_ref() {
            "open_folder" => Some(TrayAction::OpenFolder),
            "open_web_ui" => Some(TrayAction::OpenWebUI),
            "login" => Some(TrayAction::Login),
            "logout" => Some(TrayAction::Logout),
            "settings" => Some(TrayAction::Settings),
            "quit" => Some(TrayAction::Quit),
            _ => None,
        };

        if let Some(action) = action {
            let _ = action_tx_clone.send(action);
        }
    });

    Ok(Arc::new(MacOsTray {
        app: app.clone(),
        tray,
        state: Arc::new(RwLock::new(initial_state)),
        menu_items: Arc::new(RwLock::new(menu_items)),
        action_tx,
    }))
}
