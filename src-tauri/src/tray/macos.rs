//! macOS tray implementation using Tauri's TrayIcon

use super::{get_pending_text, TrayAction, TrayInterface};
use crate::state::{CurrentTransfer, SyncState};
use std::sync::{Arc, RwLock};
use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle,
};
use tokio::sync::mpsc;

// Embed tray icons at compile time (using @2x Retina versions for crisp display)
const ICON_IDLE: &[u8] = include_bytes!("../../icons/tray/idle@2x.png");
const ICON_ERROR: &[u8] = include_bytes!("../../icons/tray/error@2x.png");
const ICON_SYNCING_0: &[u8] = include_bytes!("../../icons/tray/syncing-0@2x.png");
const ICON_SYNCING_1: &[u8] = include_bytes!("../../icons/tray/syncing-1@2x.png");
const ICON_SYNCING_2: &[u8] = include_bytes!("../../icons/tray/syncing-2@2x.png");
const ICON_SYNCING_3: &[u8] = include_bytes!("../../icons/tray/syncing-3@2x.png");

/// Decode a PNG from bytes into RGBA data.
/// Since we use template mode, macOS will automatically handle dark/light mode.
/// The source PNGs should have black shapes on transparent background.
fn decode_png(png_data: &[u8]) -> Option<Image<'static>> {
    let img = image::load_from_memory(png_data).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(Image::new_owned(rgba.into_raw(), width, height))
}

/// Get the appropriate icon for the given sync state and animation frame
fn get_icon_for_state(state: SyncState, animation_frame: u8) -> Option<Image<'static>> {
    let png_data = match state {
        SyncState::Synced | SyncState::Paused | SyncState::NotLoggedIn | SyncState::Offline => {
            ICON_IDLE
        }
        SyncState::Error | SyncState::CliNotFound => ICON_ERROR,
        SyncState::Starting | SyncState::Scanning | SyncState::Syncing => {
            // Cycle through 4 frames for pulsing animation
            match animation_frame % 4 {
                0 => ICON_SYNCING_0,
                1 => ICON_SYNCING_1,
                2 => ICON_SYNCING_2,
                _ => ICON_SYNCING_3,
            }
        }
    };
    decode_png(png_data)
}

/// Shared state for menu updates
struct MenuState {
    /// Login state: None = starting/unknown, Some(true) = logged in, Some(false) = not logged in
    login_state: Option<bool>,
    status_text: String,
    sync_state: SyncState,
    pending_count: u32,
    /// Animation frame for loading indicators (0, 1, 2)
    animation_frame: u8,
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
            state.sync_state,
            state.pending_count,
            state.animation_frame,
            state.login_state,
            state.current_transfer_text.as_deref(),
        ) {
            let _ = self.tray.set_menu(Some(menu));
            *self.menu_items.write().unwrap() = items;
        }
    }
}

impl TrayInterface for MacOsTray {
    fn update_icon(&self, state: SyncState, animation_frame: u8) {
        let (needs_menu_update, needs_icon_update) = {
            let mut menu_state = self.state.write().unwrap();
            let state_changed = menu_state.sync_state != state;
            let frame_changed = menu_state.animation_frame != animation_frame;

            if state_changed {
                menu_state.sync_state = state;
            }
            if frame_changed {
                menu_state.animation_frame = animation_frame;
            }

            // Determine if we need to update the menu text
            let needs_menu = state_changed
                || (frame_changed
                    && (state == SyncState::Scanning || state == SyncState::Starting));

            // Determine if we need to update the icon
            // For syncing states, update on every frame change for animation
            // For other states, only update when state changes
            let is_animating = state == SyncState::Starting
                || state == SyncState::Scanning
                || state == SyncState::Syncing;
            let needs_icon = state_changed || (is_animating && frame_changed);

            (needs_menu, needs_icon)
        };

        if needs_menu_update {
            // Update pending count display (may have animated dots)
            let state_read = self.state.read().unwrap();
            let pending_text = get_pending_text(
                state_read.sync_state,
                state_read.pending_count,
                animation_frame,
            );
            drop(state_read);

            let items = self.menu_items.read().unwrap();
            let _ = items.pending_item.set_text(&pending_text);
        }

        if needs_icon_update {
            // Update the tray icon based on state and animation frame
            if let Some(icon) = get_icon_for_state(state, animation_frame) {
                let _ = self.tray.set_icon(Some(icon));
                // Re-enable template mode after changing the icon
                // This ensures macOS properly inverts the icon for dark/light mode
                let _ = self.tray.set_icon_as_template(true);
            }
        }
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
        let pending_text = get_pending_text(state.sync_state, count, state.animation_frame);
        drop(state);
        let items = self.menu_items.read().unwrap();
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

        let needs_rebuild = {
            let mut state = self.state.write().unwrap();
            let old_had_transfer = state.current_transfer_text.is_some();
            let new_has_transfer = new_text.is_some();

            if state.current_transfer_text != new_text {
                state.current_transfer_text = new_text.clone();
                // Need to rebuild menu if transfer item visibility changed
                old_had_transfer != new_has_transfer
            } else {
                return; // No change
            }
        };

        if needs_rebuild {
            // Transfer started or stopped - rebuild menu to add/remove the item
            self.rebuild_menu();
        } else if let Some(text) = new_text {
            // Just update the text in-place (progress changed)
            let items = self.menu_items.read().unwrap();
            let _ = items.transfer_item.set_text(&text);
        }
    }
}

/// Build the tray menu with current state
fn build_menu(
    app: &AppHandle,
    status_text: &str,
    sync_state: SyncState,
    pending_count: u32,
    animation_frame: u8,
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
    let pending_text = get_pending_text(sync_state, pending_count, animation_frame);
    let pending_item = MenuItemBuilder::with_id("pending_count", &pending_text)
        .enabled(false)
        .build(app)?;
    builder = builder.item(&pending_item);

    // Current transfer (only added when there's an active transfer)
    // Menu is rebuilt when transfer starts/stops to add/remove this item
    let transfer_text = current_transfer_text.unwrap_or("");
    let transfer_item = MenuItemBuilder::with_id("current_transfer", transfer_text)
        .enabled(false)
        .build(app)?;
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

    // Show Logs
    let show_logs_item =
        MenuItemBuilder::with_id("show_logs", rust_i18n::t!("menu.show_logs")).build(app)?;
    builder = builder.item(&show_logs_item);

    // About
    let about_item = MenuItemBuilder::with_id("about", rust_i18n::t!("menu.about")).build(app)?;
    builder = builder.item(&about_item);

    builder = builder.separator();

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
        sync_state: SyncState::Starting,
        pending_count: 0,
        animation_frame: 0,
        current_transfer_text: None,
    };

    // Build initial menu
    let (menu, menu_items) = build_menu(
        app,
        &initial_state.status_text,
        initial_state.sync_state,
        initial_state.pending_count,
        initial_state.animation_frame,
        initial_state.login_state,
        initial_state.current_transfer_text.as_deref(),
    )?;

    // Create tray icon with our custom monochrome icon
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip(rust_i18n::t!("tooltip.app_name"));

    // Use our custom syncing icon (Starting state) as the initial icon
    if let Some(icon) = get_icon_for_state(SyncState::Starting, 0) {
        builder = builder.icon(icon);
    }

    // Enable template icon for macOS dark/light mode support
    // This makes the icon automatically adapt to light/dark menu bar
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
            "show_logs" => Some(TrayAction::ShowLogs),
            "about" => Some(TrayAction::About),
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
