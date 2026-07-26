mod actions;
mod credentials;
mod logging;
mod login;
mod tray;
mod update;

// Public modules for integration testing
pub mod cli;
pub mod config;
pub mod error;
pub mod state;

use actions::ActionContext;
use cli::CliManager;
use config::Config;
use credentials::CredentialManager;
use login::{LoginCommandContext, LoginManager};
use state::{AppState, StateSnapshot, SyncState};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::{mpsc, watch};
use tray::{TrayAction, TrayInterface};

// Initialize i18n with locale files
rust_i18n::i18n!("locales", fallback = "en");

/// Initialize the locale based on config or system settings
fn init_locale(config: &Config) {
    let locale = if let Some(ref locale) = config.locale {
        locale.clone()
    } else {
        // Detect system locale, default to "en"
        sys_locale::get_locale()
            .map(|l| l.split('-').next().unwrap_or("en").to_string())
            .unwrap_or_else(|| "en".to_string())
    };

    log::info!("Setting locale to: {}", locale);
    rust_i18n::set_locale(&locale);
}

/// Handle tray menu actions by dispatching to the appropriate handler
async fn handle_tray_action(
    action: TrayAction,
    app_state: &AppState,
    cli_manager: &CliManager,
    config: &Config,
    tray: &Arc<dyn TrayInterface>,
    app_handle: &tauri::AppHandle,
    login_manager: &LoginManager,
) {
    // Create action context for handlers that need full context
    let ctx = ActionContext {
        app_state,
        cli_manager,
        config,
        tray,
        app_handle,
    };

    match action {
        TrayAction::OpenFolder => actions::open_folder(config),
        TrayAction::OpenWebUI => actions::open_web_ui(),
        TrayAction::Login => actions::login(&ctx).await,
        TrayAction::LoginCompleted => actions::login_completed(&ctx).await,
        TrayAction::Logout => actions::logout(&ctx).await,
        TrayAction::TogglePause => actions::toggle_pause(&ctx).await,
        TrayAction::Settings => actions::open_settings(),
        TrayAction::ShowLogs => actions::show_logs(),
        TrayAction::About => actions::show_about(app_handle),
        TrayAction::CheckForUpdates => actions::check_for_updates(app_handle).await,
        TrayAction::Quit => {
            login_manager.cancel();
            actions::quit(cli_manager, app_handle).await;
        }
    }
}

/// Format the last-synced timestamp for menu display
fn format_last_synced(snapshot: &StateSnapshot) -> Option<String> {
    snapshot
        .last_synced
        .map(|dt| dt.format("%H:%M").to_string())
}

/// Apply a state snapshot to the tray UI
fn apply_state_to_tray(
    tray: &Arc<dyn TrayInterface>,
    snapshot: &StateSnapshot,
    animation_frame: u8,
) {
    tray.update_status(&snapshot.sync_state.status_text());
    tray.update_last_synced(format_last_synced(snapshot).as_deref());
    tray.update_icon(snapshot.sync_state, animation_frame);
    tray.update_pending_count(snapshot.pending_count);
    tray.update_current_transfer(snapshot.current_transfer.as_ref());
}

/// States whose tray icon is animated; only these need per-frame icon updates
fn is_animated_state(state: SyncState) -> bool {
    matches!(
        state,
        SyncState::Starting | SyncState::Scanning | SyncState::Syncing
    )
}

/// States in which the 500ms timer must run: animated icons plus the
/// offline-retry and error-backoff counters. In all other states the status
/// loop blocks solely on the watch channel, so the app is fully quiescent
/// at rest.
fn needs_timer_tick(state: SyncState) -> bool {
    is_animated_state(state) || state == SyncState::Offline || state == SyncState::Error
}

/// Stop the CLI process tree before Unix terminates the app.
///
/// Home Manager and other application updaters normally stop GUI apps with
/// SIGTERM. Without an explicit handler, the OS exits this process immediately
/// and does not run Rust destructors, leaving the long-running Filen CLI
/// orphaned. Tray-menu quits already call the same cleanup path in actions.rs.
#[cfg(unix)]
fn install_termination_signal_handler(
    app_handle: tauri::AppHandle,
    cli_manager: Arc<CliManager>,
    login_manager: Arc<LoginManager>,
) {
    tauri::async_runtime::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(error) => {
                log::error!("Failed to install SIGTERM handler: {}", error);
                return;
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(error) => {
                log::error!("Failed to install SIGINT handler: {}", error);
                return;
            }
        };

        tokio::select! {
            _ = sigterm.recv() => log::info!("Received SIGTERM, shutting down"),
            _ = sigint.recv() => log::info!("Received SIGINT, shutting down"),
        }

        login_manager.cancel();
        cli_manager.stop_sync().await;
        app_handle.exit(0);
    });
}

/// Start the status update loop
///
/// This uses a hybrid approach:
/// - Reactive updates via watch channel when state changes
/// - Timer-based updates for icon animation (every 500ms)
/// - Auto-retry logic for offline state (every 30s)
/// - Auto-restart logic for error state with exponential backoff
async fn status_update_loop(
    app_state: AppState,
    // Subscribed by the caller BEFORE this task is spawned, so state changes
    // notified during startup cannot be missed
    mut state_rx: watch::Receiver<StateSnapshot>,
    tray: Arc<dyn TrayInterface>,
    cli_manager: Arc<CliManager>,
    config: Config,
) {
    log::info!("Status update loop started");

    // Animation timer (500ms for smooth icon pulsing)
    let mut animation_interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
    // The timer is gated off while idle; don't fire a burst of catch-up
    // ticks when it is re-enabled after a long pause
    animation_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut animation_frame = 0u8;
    let mut offline_ticks = 0u32;

    // Error restart state with exponential backoff
    // Delays: 5s, 10s, 20s, 40s, 60s (capped)
    let mut error_ticks = 0u32;
    let mut error_restart_delay_secs = 5u32;
    const MAX_ERROR_RESTART_DELAY_SECS: u32 = 60;

    // Track current state for animation updates
    let mut current_snapshot = StateSnapshot::default();

    loop {
        tokio::select! {
            // React to state changes immediately
            result = state_rx.changed() => {
                if result.is_err() {
                    // Channel closed, shouldn't happen but handle gracefully
                    log::error!("State watch channel closed");
                    break;
                }

                current_snapshot = state_rx.borrow().clone();
                log::debug!("Reactive state update: {:?}", current_snapshot.sync_state);
                apply_state_to_tray(&tray, &current_snapshot, animation_frame);

                // Reset counters on state change
                if current_snapshot.sync_state != SyncState::Offline {
                    offline_ticks = 0;
                }
                if current_snapshot.sync_state != SyncState::Error {
                    error_ticks = 0;
                    // Reset backoff only once a sync cycle actually completes.
                    // (Syncing is not enough: a crash-looping CLI can briefly
                    // reach Syncing each round, which would defeat the backoff.)
                    if current_snapshot.sync_state == SyncState::Synced {
                        error_restart_delay_secs = 5;
                    }
                }
            }

            // Animation timer for icon pulsing and auto-retry; disabled
            // entirely while idle (Synced/Paused/NotLoggedIn/CliNotFound)
            _ = animation_interval.tick(), if needs_timer_tick(current_snapshot.sync_state) => {
                animation_frame = (animation_frame + 1) % 4;

                // Update icon animation frame - only animated states need
                // per-frame updates; idle states would just spam the tray
                // (on Linux every update is a D-Bus round trip)
                if is_animated_state(current_snapshot.sync_state) {
                    tray.update_icon(current_snapshot.sync_state, animation_frame);
                }

                // Auto-retry when offline: attempt to reconnect every ~30 seconds
                // (60 ticks at 500ms intervals = 30 seconds)
                if current_snapshot.sync_state == SyncState::Offline {
                    offline_ticks += 1;
                    if offline_ticks >= 60 {
                        offline_ticks = 0;
                        log::info!("Attempting to reconnect after offline state...");
                        app_state.set_sync_state(SyncState::Scanning).await;
                        if let Err(e) = cli_manager.start_sync(&config).await {
                            log::debug!("Reconnect attempt failed: {}", e);
                        }
                    }
                }

                // Auto-restart on error: restart CLI with exponential backoff
                // This handles CLI crashes (e.g., RangeError in Node.js)
                if current_snapshot.sync_state == SyncState::Error {
                    error_ticks += 1;
                    // Convert delay seconds to ticks (2 ticks per second at 500ms intervals)
                    let delay_ticks = error_restart_delay_secs * 2;
                    if error_ticks >= delay_ticks {
                        error_ticks = 0;
                        log::info!(
                            "Attempting to restart sync after CLI error (delay was {}s)...",
                            error_restart_delay_secs
                        );
                        // Increase backoff for the NEXT attempt regardless of
                        // whether the spawn succeeds: a CLI that starts fine
                        // but crashes moments later must also back off,
                        // otherwise it restarts at the minimum delay forever.
                        // Reset happens when a sync cycle completes (Synced).
                        error_restart_delay_secs =
                            (error_restart_delay_secs * 2).min(MAX_ERROR_RESTART_DELAY_SECS);
                        app_state.set_sync_state(SyncState::Scanning).await;
                        if let Err(e) = cli_manager.start_sync(&config).await {
                            log::warn!("Restart attempt failed: {}", e);
                        }
                    }
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load configuration first (needed for log_level)
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            Config::default()
        }
    };

    // Initialize logging (file logging only if enabled in config)
    match logging::init_logging(config.logging_enabled, config.log_level) {
        Ok(log_path) => {
            log::info!("Starting Filen Menubar");
            if config.logging_enabled {
                log::info!("Log file: {:?}", log_path);
            }
            log::info!("Loaded config: {:?}", config);
        }
        Err(e) => {
            eprintln!("Failed to initialize logging: {}", e);
        }
    }

    // Initialize locale
    init_locale(&config);

    // Ensure sync folder exists
    if let Err(e) = config.ensure_sync_folder() {
        log::error!("Failed to create sync folder: {}", e);
    }

    // Create app state
    let app_state = AppState::new();
    // Subscribe immediately, before any task can change state: a watch
    // receiver only sees changes made after subscription, so subscribing
    // inside the (later-spawned) status loop could miss startup transitions
    let state_rx = app_state.subscribe();
    let cli_manager = Arc::new(CliManager::new(app_state.clone()));

    // Create action channel
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<TrayAction>();
    let login_manager = Arc::new(LoginManager::new());
    let login_context = LoginCommandContext::new(login_manager.clone(), action_tx.clone());

    // Store shared state for the action handler
    let config_clone = config.clone();
    let app_state_clone = app_state.clone();
    let cli_manager_clone = cli_manager.clone();

    tauri::Builder::default()
        .manage(login_context.clone())
        .invoke_handler(tauri::generate_handler![
            login::login_copy,
            login::start_login,
            login::submit_two_factor,
            login::cancel_login,
            login::close_login
        ])
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(login::handle_window_event)
        .setup(move |app| {
            // Create the tray icon
            // On Linux with ksni 0.3, the spawn is async, so we need block_on
            #[cfg(target_os = "linux")]
            let tray =
                tauri::async_runtime::block_on(tray::create_tray(app.handle(), action_tx.clone()))
                    .expect("Failed to create tray");
            #[cfg(not(target_os = "linux"))]
            let tray =
                tray::create_tray(app.handle(), action_tx.clone()).expect("Failed to create tray");

            let app_handle = app.handle().clone();
            let config = config_clone.clone();
            let app_state = app_state_clone.clone();
            let cli_manager = cli_manager_clone.clone();
            let login_manager = login_manager.clone();

            #[cfg(unix)]
            install_termination_signal_handler(
                app_handle.clone(),
                cli_manager.clone(),
                login_manager.clone(),
            );

            // Spawn action handler
            let tray_for_handler = tray.clone();
            let app_state_for_handler = app_state.clone();
            let cli_manager_for_handler = cli_manager.clone();
            let config_for_handler = config.clone();
            let app_handle_for_handler = app_handle.clone();
            let login_manager_for_handler = login_manager.clone();

            tauri::async_runtime::spawn(async move {
                while let Some(action) = action_rx.recv().await {
                    handle_tray_action(
                        action,
                        &app_state_for_handler,
                        &cli_manager_for_handler,
                        &config_for_handler,
                        &tray_for_handler,
                        &app_handle_for_handler,
                        &login_manager_for_handler,
                    )
                    .await;
                }
            });

            // Spawn status update loop
            let tray_for_status = tray.clone();
            let app_state_for_status = app_state.clone();
            let cli_manager_for_status = cli_manager.clone();
            let config_for_status = config.clone();

            tauri::async_runtime::spawn(async move {
                status_update_loop(
                    app_state_for_status,
                    state_rx,
                    tray_for_status,
                    cli_manager_for_status,
                    config_for_status,
                )
                .await;
            });

            // Check for existing CLI session and auto-start if configured
            let app_state_for_autostart = app_state.clone();
            let cli_manager_for_autostart = cli_manager.clone();
            let config_for_autostart = config.clone();
            let tray_for_autostart = tray.clone();

            tauri::async_runtime::spawn(async move {
                use tokio::time::{timeout, Duration};

                log::info!("Starting initialization check...");

                // Wrap entire initialization in a timeout to prevent hanging forever
                // Note: CLI retry logic can take up to 14s (0+2+4+8), so allow extra buffer
                let init_result = timeout(Duration::from_secs(30), async {
                    // Check if CLI is available
                    log::info!("Checking CLI availability...");
                    let cli_available = CliManager::is_cli_available().await;
                    log::info!("CLI availability check complete: {}", cli_available);

                    if !cli_available {
                        log::error!(
                            "Filen CLI not found. Please install it with: npm install -g @filen/cli"
                        );
                        return (SyncState::CliNotFound, None);
                    }

                    // Check for stored CLI session (a few quick file stats)
                    log::info!("Checking credentials...");
                    let credentials_exist = CredentialManager::exists();
                    log::info!("Credentials exist: {}", credentials_exist);

                    if credentials_exist {
                        if config_for_autostart.auto_start {
                            log::info!("Found Filen CLI session, will auto-start sync");
                            // Start with Scanning - CLI will update to Syncing when it finds deltas
                            (SyncState::Scanning, Some(true))
                        } else {
                            log::info!("Found Filen CLI session, but auto_start is disabled");
                            (SyncState::Synced, Some(true))
                        }
                    } else {
                        log::info!("No Filen CLI session found");
                        (SyncState::NotLoggedIn, Some(false))
                    }
                })
                .await;

                // Apply the result (or fallback on timeout)
                let (new_state, login_state) = match init_result {
                    Ok(result) => result,
                    Err(_) => {
                        log::error!("Initialization timed out, defaulting to NotLoggedIn");
                        (SyncState::NotLoggedIn, Some(false))
                    }
                };

                log::info!(
                    "Setting state to: {:?}, login_state: {:?}",
                    new_state,
                    login_state
                );

                // Update app state
                app_state_for_autostart.set_sync_state(new_state).await;
                if let Some(logged_in) = login_state {
                    if logged_in {
                        app_state_for_autostart.set_logged_in(true).await;
                    }
                }

                // Update tray
                if let Some(ls) = login_state {
                    tray_for_autostart.set_login_state(Some(ls));
                }
                tray_for_autostart.update_status(&new_state.status_text());
                tray_for_autostart.update_icon(new_state, 0);

                log::info!("Initialization complete, state: {:?}", new_state);

                // Start sync if needed (after state is set)
                if new_state == SyncState::Scanning || new_state == SyncState::Syncing {
                    if let Err(e) = cli_manager_for_autostart
                        .start_sync(&config_for_autostart)
                        .await
                    {
                        log::error!("Failed to auto-start sync: {}", e);
                        app_state_for_autostart
                            .set_sync_state(SyncState::Error)
                            .await;
                        tray_for_autostart.update_status(&SyncState::Error.status_text());
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                let login_context = app_handle.state::<LoginCommandContext>();
                if login_context.should_prevent_exit(code) {
                    log::info!("Keeping tray app alive after closing login window");
                    api.prevent_exit();
                }
            }
        });
}
