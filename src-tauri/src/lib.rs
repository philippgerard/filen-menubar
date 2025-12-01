mod cli;
mod config;
mod credentials;
mod state;
mod tray;

use cli::CliManager;
use config::Config;
use credentials::CredentialManager;
use state::{AppState, SyncState};
use std::sync::Arc;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::mpsc;
use tray::{TrayAction, TrayInterface};

/// Handle tray menu actions
async fn handle_tray_action(
    action: TrayAction,
    app_state: &AppState,
    cli_manager: &CliManager,
    config: &Config,
    tray: &Arc<dyn TrayInterface>,
    app_handle: &tauri::AppHandle,
) {
    match action {
        TrayAction::OpenFolder => {
            log::info!("Opening sync folder: {:?}", config.local_path);
            if let Err(e) = open::that(&config.local_path) {
                log::error!("Failed to open folder: {}", e);
            }
        }
        TrayAction::OpenWebUI => {
            log::info!("Opening Filen web UI");
            if let Err(e) = open::that("https://app.filen.io") {
                log::error!("Failed to open web UI: {}", e);
            }
        }
        TrayAction::Login => {
            log::info!("Login requested");

            // Check if CLI session exists
            if CredentialManager::exists() {
                log::info!("Found Filen CLI session, starting sync");
                app_state.set_logged_in(true).await;
                tray.set_logged_in(true);
                tray.update_status(SyncState::Syncing.status_text());

                if let Err(e) = cli_manager.start_sync(config).await {
                    log::error!("Failed to start sync: {}", e);
                    app_state.set_sync_state(SyncState::Error).await;
                }
            } else {
                log::info!("No Filen CLI session found. Please run 'filen login' first to authenticate.");
            }
        }
        TrayAction::Logout => {
            log::info!("Logout requested");

            // Show confirmation dialog
            let app_state = app_state.clone();
            let tray = tray.clone();

            let confirmed = app_handle
                .dialog()
                .message("This will log you out of Filen and stop syncing. You'll need to run 'filen login' in the terminal to log back in.")
                .title("Confirm Logout")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom("Logout".to_string(), "Cancel".to_string()))
                .blocking_show();

            if confirmed {
                log::info!("Logout confirmed");
                cli_manager.stop_sync().await;
                let _ = CredentialManager::delete();
                app_state.set_logged_in(false).await;
                app_state.set_sync_state(SyncState::NotLoggedIn).await;
                tray.set_logged_in(false);
                tray.update_status(SyncState::NotLoggedIn.status_text());
            } else {
                log::info!("Logout cancelled");
            }
        }
        TrayAction::Settings => {
            log::info!("Settings requested");
            // Open config file in default editor
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
        TrayAction::Quit => {
            log::info!("Quit requested");
            cli_manager.stop_sync().await;
            app_handle.exit(0);
        }
    }
}

/// Start the status update loop
async fn status_update_loop(
    app_state: AppState,
    tray: Arc<dyn TrayInterface>,
    _cli_manager: Arc<CliManager>,
) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));

    loop {
        interval.tick().await;

        let sync_state = app_state.get_sync_state().await;
        tray.update_status(sync_state.status_text());
        tray.update_icon(sync_state);

        // Update pending file count
        let pending_count = app_state.get_pending_count().await;
        tray.update_pending_count(pending_count);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting Filen Menubar");

    // Load configuration
    let config = match Config::load() {
        Ok(c) => {
            log::info!("Loaded config: {:?}", c);
            c
        }
        Err(e) => {
            log::error!("Failed to load config: {}", e);
            Config::default()
        }
    };

    // Ensure sync folder exists
    if let Err(e) = config.ensure_sync_folder() {
        log::error!("Failed to create sync folder: {}", e);
    }

    // Create app state
    let app_state = AppState::new();
    let cli_manager = Arc::new(CliManager::new(app_state.clone()));

    // Create action channel
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<TrayAction>();

    // Store shared state for the action handler
    let config_clone = config.clone();
    let app_state_clone = app_state.clone();
    let cli_manager_clone = cli_manager.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            // Create the tray icon
            // On Linux with ksni 0.3, the spawn is async, so we need block_on
            #[cfg(target_os = "linux")]
            let tray = tauri::async_runtime::block_on(tray::create_tray(app.handle(), action_tx.clone()))
                .expect("Failed to create tray");
            #[cfg(not(target_os = "linux"))]
            let tray =
                tray::create_tray(app.handle(), action_tx.clone()).expect("Failed to create tray");

            let app_handle = app.handle().clone();
            let config = config_clone.clone();
            let app_state = app_state_clone.clone();
            let cli_manager = cli_manager_clone.clone();

            // Spawn action handler
            let tray_for_handler = tray.clone();
            let app_state_for_handler = app_state.clone();
            let cli_manager_for_handler = cli_manager.clone();
            let config_for_handler = config.clone();
            let app_handle_for_handler = app_handle.clone();

            tauri::async_runtime::spawn(async move {
                while let Some(action) = action_rx.recv().await {
                    handle_tray_action(
                        action,
                        &app_state_for_handler,
                        &cli_manager_for_handler,
                        &config_for_handler,
                        &tray_for_handler,
                        &app_handle_for_handler,
                    )
                    .await;
                }
            });

            // Spawn status update loop
            let tray_for_status = tray.clone();
            let app_state_for_status = app_state.clone();
            let cli_manager_for_status = cli_manager.clone();

            tauri::async_runtime::spawn(async move {
                status_update_loop(
                    app_state_for_status,
                    tray_for_status,
                    cli_manager_for_status,
                )
                .await;
            });

            // Check for existing CLI session and auto-start if configured
            let app_state_for_autostart = app_state.clone();
            let cli_manager_for_autostart = cli_manager.clone();
            let config_for_autostart = config.clone();
            let tray_for_autostart = tray.clone();

            tauri::async_runtime::spawn(async move {
                // Check if CLI is available
                if !CliManager::is_cli_available().await {
                    log::error!(
                        "Filen CLI not found. Please install it with: npm install -g @filen/cli"
                    );
                    return;
                }

                // Check for stored CLI session
                if CredentialManager::exists() && config_for_autostart.auto_start {
                    log::info!("Found Filen CLI session, auto-starting sync");
                    app_state_for_autostart.set_logged_in(true).await;
                    tray_for_autostart.set_logged_in(true);
                    tray_for_autostart.update_status(SyncState::Syncing.status_text());

                    if let Err(e) = cli_manager_for_autostart
                        .start_sync(&config_for_autostart)
                        .await
                    {
                        log::error!("Failed to auto-start sync: {}", e);
                        app_state_for_autostart
                            .set_sync_state(SyncState::Error)
                            .await;
                    }
                }
            });

            // Hide from dock on macOS
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
