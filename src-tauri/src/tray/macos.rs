//! macOS tray implementation using Tauri's TrayIcon

use super::{TrayAction, TrayInterface};
use crate::state::{CurrentTransfer, SyncState};
use std::sync::{Arc, RwLock};
use tauri::{
    menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle,
};
use tokio::sync::mpsc;

/// Shared state for menu updates
struct MenuState {
    /// Login state: None = starting/unknown, Some(true) = logged in, Some(false) = not logged in
    login_state: Option<bool>,
    status_text: String,
    pending_count: u32,
    /// Current transfer display text (None = hidden)
    current_transfer_text: Option<String>,
}

/// Stored menu item references for in-place updates
struct MenuItems {
    status_item: MenuItem<tauri::Wry>,
    pending_item: MenuItem<tauri::Wry>,
    transfer_item: MenuItem<tauri::Wry>,
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
            state.login_state,
            state.current_transfer_text.as_deref(),
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
            if state.pending_count == 1 {
                rust_i18n::t!("tooltip.syncing_file").to_string()
            } else {
                rust_i18n::t!("tooltip.syncing_files", count = state.pending_count).to_string()
            }
        } else {
            rust_i18n::t!("tooltip.status", status = text).to_string()
        };
        let _ = self.tray.set_tooltip(Some(&tooltip));

        // Update menu item text in-place (doesn't close menu)
        let items = self.menu_items.read().unwrap();
        let _ = items
            .status_item
            .set_text(rust_i18n::t!("menu.status", status = text));
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
            if count == 1 {
                rust_i18n::t!("tooltip.syncing_file").to_string()
            } else {
                rust_i18n::t!("tooltip.syncing_files", count = count).to_string()
            }
        } else {
            rust_i18n::t!("tooltip.status", status = &state.status_text).to_string()
        };
        let _ = self.tray.set_tooltip(Some(&tooltip));

        // Update menu item text in-place (doesn't close menu)
        let items = self.menu_items.read().unwrap();
        let pending_text = if count > 0 {
            if count == 1 {
                rust_i18n::t!("menu.file_remaining").to_string()
            } else {
                rust_i18n::t!("menu.files_remaining", count = count).to_string()
            }
        } else {
            rust_i18n::t!("menu.up_to_date").to_string()
        };
        let _ = items.pending_item.set_text(&pending_text);
    }

    fn update_storage(&self, _text: &str) {
        // Storage not supported by CLI, ignore
    }

    fn set_login_state(&self, login_state: Option<bool>) {
        {
            let mut state = self.state.write().unwrap();
            if state.login_state != login_state {
                state.login_state = login_state;
            } else {
                return; // No change, don't rebuild
            }
        }
        // Login/logout changes menu structure, so we need to rebuild
        self.rebuild_menu();
    }

    fn update_current_transfer(&self, transfer: Option<&CurrentTransfer>) {
        let new_text = transfer.map(|t| t.display_text(25));

        {
            let mut state = self.state.write().unwrap();
            if state.current_transfer_text != new_text {
                state.current_transfer_text = new_text.clone();
            } else {
                return; // No change
            }
        }

        // Update the transfer menu item text in-place
        let items = self.menu_items.read().unwrap();
        match new_text {
            Some(text) => {
                let _ = items.transfer_item.set_text(&text);
                let _ = items.transfer_item.set_enabled(true); // Make visible by enabling
            }
            None => {
                // Hide the item by setting empty text (Tauri doesn't support hiding items)
                let _ = items.transfer_item.set_text("");
                let _ = items.transfer_item.set_enabled(false);
            }
        }
    }
}

/// Build the tray menu with current state
fn build_menu(
    app: &AppHandle,
    status_text: &str,
    pending_count: u32,
    login_state: Option<bool>,
    current_transfer_text: Option<&str>,
) -> Result<(Menu<tauri::Wry>, MenuItems), Box<dyn std::error::Error>> {
    let mut builder = MenuBuilder::new(app);

    // Status (disabled, just for display)
    let status_item =
        MenuItemBuilder::with_id("status", rust_i18n::t!("menu.status", status = status_text))
            .enabled(false)
            .build(app)?;
    builder = builder.item(&status_item);

    // Pending file count (always present)
    let pending_text = if pending_count > 0 {
        if pending_count == 1 {
            rust_i18n::t!("menu.file_remaining").to_string()
        } else {
            rust_i18n::t!("menu.files_remaining", count = pending_count).to_string()
        }
    } else {
        rust_i18n::t!("menu.up_to_date").to_string()
    };
    let pending_item = MenuItemBuilder::with_id("pending_count", &pending_text)
        .enabled(false)
        .build(app)?;
    builder = builder.item(&pending_item);

    // Current transfer (only shown when there's an active transfer)
    let transfer_text = current_transfer_text.unwrap_or("");
    let transfer_item = MenuItemBuilder::with_id("current_transfer", transfer_text)
        .enabled(false)
        .build(app)?;
    // Only add to menu if there's a transfer in progress
    if current_transfer_text.is_some() {
        builder = builder.item(&transfer_item);
    }

    builder = builder.separator();

    // Open Local Folder (enabled only when logged in)
    let open_folder =
        MenuItemBuilder::with_id("open_folder", rust_i18n::t!("menu.open_local_folder"))
            .enabled(login_state == Some(true))
            .build(app)?;
    builder = builder.item(&open_folder);

    // Open Web UI
    let open_web_ui =
        MenuItemBuilder::with_id("open_web_ui", rust_i18n::t!("menu.open_web_ui")).build(app)?;
    builder = builder.item(&open_web_ui);

    builder = builder.separator();

    // Login or Logout based on state (hidden when None/starting)
    match login_state {
        Some(true) => {
            let logout_item =
                MenuItemBuilder::with_id("logout", rust_i18n::t!("menu.logout")).build(app)?;
            builder = builder.item(&logout_item);
        }
        Some(false) => {
            let login_item =
                MenuItemBuilder::with_id("login", rust_i18n::t!("menu.login")).build(app)?;
            builder = builder.item(&login_item);
        }
        None => {
            // Starting state - hide both Login and Logout buttons
        }
    }

    builder = builder.separator();

    // Settings
    let settings_item =
        MenuItemBuilder::with_id("settings", rust_i18n::t!("menu.settings")).build(app)?;
    builder = builder.item(&settings_item);

    // Quit
    let quit_item = MenuItemBuilder::with_id("quit", rust_i18n::t!("menu.quit")).build(app)?;
    builder = builder.item(&quit_item);

    let items = MenuItems {
        status_item,
        pending_item,
        transfer_item,
    };

    Ok((builder.build()?, items))
}

/// Create the tray icon and menu for macOS
pub fn create_tray(
    app: &AppHandle,
    action_tx: mpsc::UnboundedSender<TrayAction>,
) -> Result<Arc<dyn TrayInterface>, Box<dyn std::error::Error>> {
    let initial_status = rust_i18n::t!("status.starting").to_string();
    let initial_state = MenuState {
        login_state: None, // Starting state - unknown login status
        status_text: initial_status.clone(),
        pending_count: 0,
        current_transfer_text: None,
    };

    // Build initial menu
    let (menu, menu_items) = build_menu(
        app,
        &initial_state.status_text,
        initial_state.pending_count,
        initial_state.login_state,
        initial_state.current_transfer_text.as_deref(),
    )?;

    // Create tray icon
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip(&rust_i18n::t!("tooltip.app_name").to_string());

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
