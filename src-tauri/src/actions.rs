//! Tray action handlers using the command pattern
//!
//! Each tray menu action is implemented as a separate handler function,
//! making them easier to test and maintain. The handlers are organized
//! by functionality and can be executed independently.

use crate::cli::CliManager;
use crate::config::Config;
use crate::credentials::CredentialManager;
use crate::logging;
use crate::state::{AppState, SyncState};
use crate::tray::TrayInterface;
use std::sync::Arc;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// Context required for executing tray actions
pub struct ActionContext<'a> {
    pub app_state: &'a AppState,
    pub cli_manager: &'a CliManager,
    pub config: &'a Config,
    pub tray: &'a Arc<dyn TrayInterface>,
    pub app_handle: &'a tauri::AppHandle,
}

/// Open the local sync folder in the system file manager
pub fn open_folder(config: &Config) {
    log::info!("Opening sync folder: {:?}", config.local_path);
    if let Err(e) = open::that(&config.local_path) {
        log::error!("Failed to open folder: {}", e);
    }
}

/// Open the Filen web UI in the default browser
pub fn open_web_ui() {
    log::info!("Opening Filen web UI");
    if let Err(e) = open::that("https://app.filen.io") {
        log::error!("Failed to open web UI: {}", e);
    }
}

/// Handle login action - check for CLI session and start sync
pub async fn login(ctx: &ActionContext<'_>) {
    log::info!("Login requested");

    // Check if CLI session exists
    if CredentialManager::exists() {
        log::info!("Found Filen CLI session, starting sync");
        ctx.app_state.set_logged_in(true).await;
        ctx.tray.set_login_state(Some(true));
        ctx.tray.update_status(&SyncState::Syncing.status_text());

        if let Err(e) = ctx.cli_manager.start_sync(ctx.config).await {
            log::error!("Failed to start sync: {}", e);
            ctx.app_state.set_sync_state(SyncState::Error).await;
        }
    } else {
        log::info!("No Filen CLI session found. Please run 'filen' first to authenticate.");
        // Tell the user what to do instead of failing silently
        ctx.app_handle
            .dialog()
            .message(rust_i18n::t!("dialog.login_help_message"))
            .title(rust_i18n::t!("dialog.login_help_title"))
            .kind(MessageDialogKind::Info)
            .blocking_show();
    }
}

/// Handle pause/resume action - pause syncing when active, resume when paused
pub async fn toggle_pause(ctx: &ActionContext<'_>) {
    let current = ctx.app_state.get_sync_state().await;
    if current == SyncState::Paused {
        log::info!("Resume requested");
        ctx.app_state.set_sync_state(SyncState::Scanning).await;
        if let Err(e) = ctx.cli_manager.start_sync(ctx.config).await {
            log::error!("Failed to resume sync: {}", e);
            ctx.app_state.set_sync_state(SyncState::Error).await;
        }
    } else {
        log::info!("Pause requested");
        ctx.cli_manager.stop_sync().await;
        // stop_sync only sets Paused when a process was running; ensure the
        // user's intent is reflected even if the CLI had already died
        // (e.g. pausing while in Error or Offline state stops the auto-retry)
        ctx.app_state.set_sync_state(SyncState::Paused).await;
    }
}

/// Handle logout action - show confirmation dialog and stop sync
pub async fn logout(ctx: &ActionContext<'_>) {
    log::info!("Logout requested");

    let confirmed = ctx
        .app_handle
        .dialog()
        .message(rust_i18n::t!("dialog.logout_message"))
        .title(rust_i18n::t!("dialog.logout_title"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            rust_i18n::t!("dialog.logout_confirm").to_string(),
            rust_i18n::t!("dialog.logout_cancel").to_string(),
        ))
        .blocking_show();

    if confirmed {
        log::info!("Logout confirmed");
        ctx.cli_manager.stop_sync().await;
        let _ = CredentialManager::delete();
        ctx.app_state.set_logged_in(false).await;
        ctx.app_state.set_sync_state(SyncState::NotLoggedIn).await;
        ctx.tray.set_login_state(Some(false));
        ctx.tray
            .update_status(&SyncState::NotLoggedIn.status_text());
    } else {
        log::info!("Logout cancelled");
    }
}

/// Open the configuration file in a text editor
pub fn open_settings() {
    log::info!("Settings requested");

    if let Ok(config_path) = Config::config_path() {
        #[cfg(target_os = "macos")]
        {
            if let Err(e) = std::process::Command::new("open")
                .arg("-a")
                .arg("TextEdit")
                .arg(&config_path)
                .spawn()
            {
                log::error!("Failed to open config in TextEdit: {}", e);
            }
        }

        #[cfg(target_os = "linux")]
        {
            // On Linux, try xdg-open first, fall back to common editors
            let opened = std::process::Command::new("xdg-open")
                .arg(&config_path)
                .spawn()
                .is_ok()
                || std::process::Command::new("gedit")
                    .arg(&config_path)
                    .spawn()
                    .is_ok()
                || std::process::Command::new("kate")
                    .arg(&config_path)
                    .spawn()
                    .is_ok()
                || std::process::Command::new("xed")
                    .arg(&config_path)
                    .spawn()
                    .is_ok()
                || std::process::Command::new("nano")
                    .arg(&config_path)
                    .spawn()
                    .is_ok();

            if !opened {
                log::error!(
                    "Failed to open config file. Please edit manually: {:?}",
                    config_path
                );
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            if let Err(e) = open::that(&config_path) {
                log::error!("Failed to open config: {}", e);
            }
        }
    }
}

/// Open the log directory in the system file manager
pub fn show_logs() {
    log::info!("Show logs requested");
    let log_dir = logging::get_log_dir();

    // Create the directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        log::error!("Failed to create log directory: {}", e);
    }

    if let Err(e) = open::that(&log_dir) {
        log::error!("Failed to open log directory: {}", e);
    }
}

/// Show the About dialog
pub fn show_about(app_handle: &tauri::AppHandle) {
    log::info!("About requested");

    let version = env!("CARGO_PKG_VERSION");
    let about_text = format!(
        "Version {}\n\n\
         A lightweight menubar app for Filen cloud sync.\n\n\
         Author: Philipp Gerard\n\
         License: MIT\n\n\
         https://github.com/philippgerard/filen-menubar",
        version
    );

    app_handle
        .dialog()
        .message(about_text)
        .title("Filen Menubar")
        .kind(MessageDialogKind::Info)
        .blocking_show();
}

/// Handle quit action - stop sync and exit the application
pub async fn quit(cli_manager: &CliManager, app_handle: &tauri::AppHandle) {
    log::info!("Quit requested");
    cli_manager.stop_sync().await;
    app_handle.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Most action handlers involve system interactions (opening files,
    // showing dialogs, etc.) that can't be easily unit tested without mocking.
    // These tests focus on verifying the code structure is correct.

    #[test]
    fn test_action_context_fields() {
        // Compile-time test to ensure ActionContext has the expected fields
        // by referencing each field in a type-checking context
        fn _verify_context_types(
            app_state: &AppState,
            cli_manager: &CliManager,
            config: &Config,
            tray: &Arc<dyn TrayInterface>,
            app_handle: &tauri::AppHandle,
        ) {
            let ctx = ActionContext {
                app_state,
                cli_manager,
                config,
                tray,
                app_handle,
            };
            // Access fields to verify they exist with correct types
            let _: &AppState = ctx.app_state;
            let _: &CliManager = ctx.cli_manager;
            let _: &Config = ctx.config;
            let _: &Arc<dyn TrayInterface> = ctx.tray;
            let _: &tauri::AppHandle = ctx.app_handle;
        }
    }

    #[test]
    fn test_config_path_exists_for_settings() {
        // Verify Config::config_path() returns a result
        // This tests the precondition for open_settings()
        let result = Config::config_path();
        assert!(result.is_ok(), "Config path should be determinable");
    }

    #[test]
    fn test_log_dir_is_valid_path() {
        // Verify logging::get_log_dir() returns a valid path
        // This tests the precondition for show_logs()
        let log_dir = logging::get_log_dir();
        // Path should not be empty
        assert!(!log_dir.as_os_str().is_empty());
    }

    // Note: We don't test open_folder, show_logs, open_web_ui directly
    // because they trigger system dialogs/file managers which would
    // interfere with automated testing.
}
