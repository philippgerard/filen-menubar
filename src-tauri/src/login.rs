//! Interactive Filen CLI authentication hosted by the app.
//!
//! Filen only persists credentials when its interactive login flow is used.
//! This module runs that flow in a pseudo-terminal, feeds credentials through
//! stdin, and reports structured states to the temporary login window. Secrets
//! are never placed in command arguments, environment variables, or logs.

use crate::cli::{find_filen_cli, FilenCliInfo};
use crate::tray::TrayAction;
use portable_pty::{Child, ChildKiller, CommandBuilder, NativePtySystem, PtySize, PtySystem};
use serde::Serialize;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{Manager, State, WebviewWindow};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use zeroize::{Zeroize, Zeroizing};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);
const OUTPUT_BUFFER_LIMIT: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum LoginOutcome {
    NeedsTwoFactor,
    Success,
    InvalidCredentials,
    InvalidTwoFactor,
    KeychainUnavailable,
    MissingFields,
    Busy,
    NoActiveLogin,
    Timeout,
    Cancelled,
    Failed,
}

impl LoginOutcome {
    fn status(&self) -> &'static str {
        match self {
            Self::NeedsTwoFactor => "needsTwoFactor",
            Self::Success => "success",
            Self::InvalidCredentials => "invalidCredentials",
            Self::InvalidTwoFactor => "invalidTwoFactor",
            Self::KeychainUnavailable => "keychainUnavailable",
            Self::MissingFields => "missingFields",
            Self::Busy => "busy",
            Self::NoActiveLogin => "noActiveLogin",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    fn is_terminal(&self) -> bool {
        !matches!(self, Self::NeedsTwoFactor)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    status: &'static str,
}

impl From<LoginOutcome> for LoginResponse {
    fn from(outcome: LoginOutcome) -> Self {
        Self {
            status: outcome.status(),
        }
    }
}

enum LoginInput {
    TwoFactor(Zeroizing<String>),
    Cancel,
}

struct LoginSession {
    input_tx: std_mpsc::Sender<LoginInput>,
    event_rx: AsyncMutex<mpsc::UnboundedReceiver<LoginOutcome>>,
    cancelled: Arc<AtomicBool>,
    killer: Arc<Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>>,
}

impl LoginSession {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.input_tx.send(LoginInput::Cancel);

        if let Ok(mut killer) = self.killer.lock() {
            if let Some(killer) = killer.as_mut() {
                let _ = killer.kill();
            }
        }
    }
}

/// Owns the single interactive login process allowed at a time.
pub struct LoginManager {
    current: Mutex<Option<Arc<LoginSession>>>,
}

impl LoginManager {
    pub fn new() -> Self {
        Self {
            current: Mutex::new(None),
        }
    }

    async fn start(&self, email: String, password: String) -> LoginOutcome {
        let email = Zeroizing::new(email);
        let password = Zeroizing::new(password);

        if email.trim().is_empty() || password.is_empty() {
            return LoginOutcome::MissingFields;
        }

        {
            let current = self.current.lock().unwrap_or_else(|e| e.into_inner());
            if current.is_some() {
                return LoginOutcome::Busy;
            }
        }

        let cli_info = match tokio::task::spawn_blocking(find_filen_cli).await {
            Ok(cli_info) => cli_info,
            Err(error) => {
                log::error!("Failed to discover Filen CLI for login: {error}");
                return LoginOutcome::Failed;
            }
        };

        let (input_tx, input_rx) = std_mpsc::channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let killer = Arc::new(Mutex::new(None));
        let session = Arc::new(LoginSession {
            input_tx,
            event_rx: AsyncMutex::new(event_rx),
            cancelled: cancelled.clone(),
            killer: killer.clone(),
        });

        {
            let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
            if current.is_some() {
                return LoginOutcome::Busy;
            }
            *current = Some(session.clone());
        }

        let worker_session = session.clone();
        let spawn_result = std::thread::Builder::new()
            .name("filen-login".to_string())
            .spawn(move || {
                run_login_process(
                    cli_info, email, password, input_rx, event_tx, cancelled, killer,
                );
            });

        if let Err(error) = spawn_result {
            log::error!("Failed to start Filen login worker: {error}");
            self.clear_if_current(&worker_session);
            return LoginOutcome::Failed;
        }

        self.wait_for_outcome(session).await
    }

    async fn submit_two_factor(&self, code: String) -> LoginOutcome {
        let code = Zeroizing::new(code);

        if code.trim().is_empty() {
            return LoginOutcome::MissingFields;
        }

        let session = {
            self.current
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        };
        let Some(session) = session else {
            return LoginOutcome::NoActiveLogin;
        };

        if session.input_tx.send(LoginInput::TwoFactor(code)).is_err() {
            self.clear_if_current(&session);
            return LoginOutcome::Failed;
        }

        self.wait_for_outcome(session).await
    }

    pub fn cancel(&self) {
        let session = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(session) = session {
            session.cancel();
        }
    }

    async fn wait_for_outcome(&self, session: Arc<LoginSession>) -> LoginOutcome {
        let received = {
            let mut event_rx = session.event_rx.lock().await;
            tokio::time::timeout(LOGIN_TIMEOUT, event_rx.recv()).await
        };

        let outcome = match received {
            Ok(Some(outcome)) => outcome,
            Ok(None) => LoginOutcome::Failed,
            Err(_) => {
                session.cancel();
                LoginOutcome::Timeout
            }
        };

        if outcome.is_terminal() {
            self.clear_if_current(&session);
        }

        outcome
    }

    fn clear_if_current(&self, session: &Arc<LoginSession>) {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if current
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, session))
        {
            *current = None;
        }
    }
}

impl Default for LoginManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared command context for the login webview.
#[derive(Clone)]
pub struct LoginCommandContext {
    manager: Arc<LoginManager>,
    action_tx: mpsc::UnboundedSender<TrayAction>,
    keep_alive_after_window_close: Arc<AtomicBool>,
}

impl LoginCommandContext {
    pub fn new(manager: Arc<LoginManager>, action_tx: mpsc::UnboundedSender<TrayAction>) -> Self {
        Self {
            manager,
            action_tx,
            keep_alive_after_window_close: Arc::new(AtomicBool::new(false)),
        }
    }

    fn report(&self, outcome: &LoginOutcome) {
        if outcome == &LoginOutcome::Success {
            let _ = self.action_tx.send(TrayAction::LoginCompleted);
        }
    }

    fn mark_window_closing(&self) {
        self.keep_alive_after_window_close
            .store(true, Ordering::SeqCst);
    }

    fn clear_window_closing(&self) {
        self.keep_alive_after_window_close
            .store(false, Ordering::SeqCst);
    }

    /// Consume the one exit request caused by destroying the transient login
    /// window. Programmatic exits (tray Quit, signals, restart) must pass
    /// through so the app can still terminate normally.
    pub fn should_prevent_exit(&self, exit_code: Option<i32>) -> bool {
        let login_window_was_closing = self
            .keep_alive_after_window_close
            .swap(false, Ordering::SeqCst);
        exit_code.is_none() && login_window_was_closing
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCopy {
    locale: String,
    platform: String,
    window_title: String,
    title: String,
    intro: String,
    email_label: String,
    email_placeholder: String,
    password_label: String,
    password_placeholder: String,
    persist_note: String,
    alternative_title: String,
    alternative_note: String,
    submit: String,
    cancel: String,
    authenticating: String,
    two_factor_title: String,
    two_factor_intro: String,
    two_factor_label: String,
    two_factor_placeholder: String,
    verify: String,
    verifying: String,
    success: String,
    error_missing_fields: String,
    error_invalid_credentials: String,
    error_invalid_two_factor: String,
    error_keychain_unavailable: String,
    error_busy: String,
    error_timeout: String,
    error_failed: String,
}

#[tauri::command]
pub fn login_copy() -> LoginCopy {
    LoginCopy {
        locale: rust_i18n::locale().to_string(),
        platform: std::env::consts::OS.to_string(),
        window_title: rust_i18n::t!("login.window_title").to_string(),
        title: rust_i18n::t!("login.title").to_string(),
        intro: rust_i18n::t!("login.intro").to_string(),
        email_label: rust_i18n::t!("login.email_label").to_string(),
        email_placeholder: rust_i18n::t!("login.email_placeholder").to_string(),
        password_label: rust_i18n::t!("login.password_label").to_string(),
        password_placeholder: rust_i18n::t!("login.password_placeholder").to_string(),
        persist_note: rust_i18n::t!("login.persist_note").to_string(),
        alternative_title: rust_i18n::t!("login.alternative_title").to_string(),
        alternative_note: rust_i18n::t!("login.alternative_note").to_string(),
        submit: rust_i18n::t!("login.submit").to_string(),
        cancel: rust_i18n::t!("login.cancel").to_string(),
        authenticating: rust_i18n::t!("login.authenticating").to_string(),
        two_factor_title: rust_i18n::t!("login.two_factor_title").to_string(),
        two_factor_intro: rust_i18n::t!("login.two_factor_intro").to_string(),
        two_factor_label: rust_i18n::t!("login.two_factor_label").to_string(),
        two_factor_placeholder: rust_i18n::t!("login.two_factor_placeholder").to_string(),
        verify: rust_i18n::t!("login.verify").to_string(),
        verifying: rust_i18n::t!("login.verifying").to_string(),
        success: rust_i18n::t!("login.success").to_string(),
        error_missing_fields: rust_i18n::t!("login.error_missing_fields").to_string(),
        error_invalid_credentials: rust_i18n::t!("login.error_invalid_credentials").to_string(),
        error_invalid_two_factor: rust_i18n::t!("login.error_invalid_two_factor").to_string(),
        error_keychain_unavailable: rust_i18n::t!("login.error_keychain_unavailable").to_string(),
        error_busy: rust_i18n::t!("login.error_busy").to_string(),
        error_timeout: rust_i18n::t!("login.error_timeout").to_string(),
        error_failed: rust_i18n::t!("login.error_failed").to_string(),
    }
}

#[tauri::command]
pub async fn start_login(
    email: String,
    password: String,
    context: State<'_, LoginCommandContext>,
) -> Result<LoginResponse, String> {
    let outcome = context.manager.start(email, password).await;
    context.report(&outcome);
    Ok(outcome.into())
}

#[tauri::command]
pub async fn submit_two_factor(
    two_factor_code: String,
    context: State<'_, LoginCommandContext>,
) -> Result<LoginResponse, String> {
    let outcome = context.manager.submit_two_factor(two_factor_code).await;
    context.report(&outcome);
    Ok(outcome.into())
}

#[tauri::command]
pub fn cancel_login(window: WebviewWindow, context: State<'_, LoginCommandContext>) {
    context.manager.cancel();
    close_window_without_exiting_app(&window, &context);
}

#[tauri::command]
pub fn close_login(window: WebviewWindow, context: State<'_, LoginCommandContext>) {
    close_window_without_exiting_app(&window, &context);
}

fn close_window_without_exiting_app(window: &WebviewWindow, context: &LoginCommandContext) {
    context.mark_window_closing();
    if let Err(error) = window.close() {
        context.clear_window_closing();
        log::error!("Failed to close login window: {error}");
    }
}

pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "login" {
        return;
    }

    let context = window.app_handle().state::<LoginCommandContext>();
    if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
        context.mark_window_closing();
    }
    if matches!(
        event,
        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
    ) {
        context.manager.cancel();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverAction {
    SendEmail,
    SendPassword,
    NeedsTwoFactor,
    SendRemember,
    Success,
    InvalidCredentials,
    InvalidTwoFactor,
    KeychainUnavailable,
}

struct LoginPromptDriver {
    email: Option<Zeroizing<String>>,
    password: Option<Zeroizing<String>>,
    output: String,
    sent_email: bool,
    sent_password: bool,
    requested_two_factor: bool,
    sent_remember: bool,
    terminal: bool,
}

impl LoginPromptDriver {
    fn new(email: Zeroizing<String>, password: Zeroizing<String>) -> Self {
        Self {
            email: Some(email),
            password: Some(password),
            output: String::new(),
            sent_email: false,
            sent_password: false,
            requested_two_factor: false,
            sent_remember: false,
            terminal: false,
        }
    }

    fn push(&mut self, chunk: &str) -> Vec<DriverAction> {
        if self.terminal {
            return Vec::new();
        }

        self.output.push_str(chunk);
        if self.output.len() > OUTPUT_BUFFER_LIMIT {
            let target = self.output.len() - OUTPUT_BUFFER_LIMIT;
            let keep_from = self
                .output
                .char_indices()
                .map(|(index, _)| index)
                .find(|index| *index >= target)
                .unwrap_or(self.output.len());
            self.output.drain(..keep_from);
        }

        let mut actions = Vec::new();

        if !self.sent_email && self.output.contains("Email:") {
            self.sent_email = true;
            actions.push(DriverAction::SendEmail);
        }

        if !self.sent_password && self.output.contains("Password:") {
            self.sent_password = true;
            actions.push(DriverAction::SendPassword);
        }

        if self
            .output
            .contains("Invalid Two Factor Authentication code")
            || self.output.contains("Two-factor authentication code wrong")
        {
            self.terminal = true;
            actions.push(DriverAction::InvalidTwoFactor);
            return actions;
        }

        if self.output.contains("Invalid credentials!")
            || self.output.contains("Email or password wrong")
        {
            self.terminal = true;
            actions.push(DriverAction::InvalidCredentials);
            return actions;
        }

        if !self.requested_two_factor
            && (self
                .output
                .contains("Please enter your 2FA code or recovery key:")
                || self.output.contains("Two-factor authentication code:"))
        {
            self.requested_two_factor = true;
            actions.push(DriverAction::NeedsTwoFactor);
        }

        if !self.sent_remember && self.output.contains("Keep me logged in?") {
            self.sent_remember = true;
            actions.push(DriverAction::SendRemember);
        }

        if self
            .output
            .contains("save credentials crypto key in keychain")
            || self
                .output
                .contains("Failed to save credentials in keyring")
        {
            self.terminal = true;
            actions.push(DriverAction::KeychainUnavailable);
            return actions;
        }

        if self
            .output
            .contains("You can delete these credentials using `filen logout`")
            || self.output.contains("Saved credentials")
        {
            self.terminal = true;
            actions.push(DriverAction::Success);
        }

        actions
    }

    fn take_email(&mut self) -> Option<Zeroizing<String>> {
        self.email.take()
    }

    fn take_password(&mut self) -> Option<Zeroizing<String>> {
        self.password.take()
    }
}

fn run_login_process(
    cli_info: FilenCliInfo,
    email: Zeroizing<String>,
    password: Zeroizing<String>,
    input_rx: std_mpsc::Receiver<LoginInput>,
    event_tx: mpsc::UnboundedSender<LoginOutcome>,
    cancelled: Arc<AtomicBool>,
    killer_slot: Arc<Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>>,
) {
    let pty_system = NativePtySystem::default();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 100,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(error) => {
            log::error!("Failed to create pseudo-terminal for Filen login: {error}");
            let _ = event_tx.send(LoginOutcome::Failed);
            return;
        }
    };

    let mut command = CommandBuilder::new(&cli_info.command);
    command.arg("--skip-update");
    command.env("NO_COLOR", "1");
    command.env("TERM", "xterm-256color");
    if let Some(path_env) = cli_info.path_env {
        command.env("PATH", path_env);
    }

    let child = match pair.slave.spawn_command(command) {
        Ok(child) => child,
        Err(error) => {
            log::error!("Failed to start Filen CLI login process: {error}");
            let _ = event_tx.send(LoginOutcome::Failed);
            return;
        }
    };
    drop(pair.slave);

    if let Ok(mut killer) = killer_slot.lock() {
        *killer = Some(child.clone_killer());
    }

    if cancelled.load(Ordering::SeqCst) {
        stop_child(child, &killer_slot);
        let _ = event_tx.send(LoginOutcome::Cancelled);
        return;
    }

    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            log::error!("Failed to read Filen CLI login output: {error}");
            stop_child(child, &killer_slot);
            let _ = event_tx.send(LoginOutcome::Failed);
            return;
        }
    };
    let mut writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(error) => {
            log::error!("Failed to open Filen CLI login input: {error}");
            stop_child(child, &killer_slot);
            let _ = event_tx.send(LoginOutcome::Failed);
            return;
        }
    };

    let mut driver = LoginPromptDriver::new(email, password);
    let mut chunk = [0_u8; 4096];

    loop {
        let bytes_read = match reader.read(&mut chunk) {
            Ok(0) => {
                let outcome = if cancelled.load(Ordering::SeqCst) {
                    LoginOutcome::Cancelled
                } else {
                    LoginOutcome::Failed
                };
                stop_child(child, &killer_slot);
                let _ = event_tx.send(outcome);
                return;
            }
            Ok(bytes_read) => bytes_read,
            Err(error) => {
                if !cancelled.load(Ordering::SeqCst) {
                    log::debug!("Filen login pseudo-terminal closed: {error}");
                }
                let outcome = if cancelled.load(Ordering::SeqCst) {
                    LoginOutcome::Cancelled
                } else {
                    LoginOutcome::Failed
                };
                stop_child(child, &killer_slot);
                let _ = event_tx.send(outcome);
                return;
            }
        };

        let output = String::from_utf8_lossy(&chunk[..bytes_read]);
        for action in driver.push(&output) {
            match action {
                DriverAction::SendEmail => {
                    let Some(mut value) = driver.take_email() else {
                        finish_with(LoginOutcome::Failed, &event_tx, child, &killer_slot);
                        return;
                    };
                    if write_input(&mut writer, &value).is_err() {
                        value.zeroize();
                        finish_with(LoginOutcome::Failed, &event_tx, child, &killer_slot);
                        return;
                    }
                    value.zeroize();
                }
                DriverAction::SendPassword => {
                    let Some(mut value) = driver.take_password() else {
                        finish_with(LoginOutcome::Failed, &event_tx, child, &killer_slot);
                        return;
                    };
                    if write_input(&mut writer, &value).is_err() {
                        value.zeroize();
                        finish_with(LoginOutcome::Failed, &event_tx, child, &killer_slot);
                        return;
                    }
                    value.zeroize();
                }
                DriverAction::NeedsTwoFactor => {
                    let _ = event_tx.send(LoginOutcome::NeedsTwoFactor);
                    match input_rx.recv() {
                        Ok(LoginInput::TwoFactor(mut code)) => {
                            if write_input(&mut writer, &code).is_err() {
                                code.zeroize();
                                finish_with(LoginOutcome::Failed, &event_tx, child, &killer_slot);
                                return;
                            }
                            code.zeroize();
                        }
                        Ok(LoginInput::Cancel) | Err(_) => {
                            cancelled.store(true, Ordering::SeqCst);
                            finish_with(LoginOutcome::Cancelled, &event_tx, child, &killer_slot);
                            return;
                        }
                    }
                }
                DriverAction::SendRemember => {
                    if write_input(&mut writer, "y").is_err() {
                        finish_with(LoginOutcome::Failed, &event_tx, child, &killer_slot);
                        return;
                    }
                }
                DriverAction::Success => {
                    let _ = write_input(&mut writer, "exit");
                    finish_with(LoginOutcome::Success, &event_tx, child, &killer_slot);
                    return;
                }
                DriverAction::InvalidCredentials => {
                    finish_with(
                        LoginOutcome::InvalidCredentials,
                        &event_tx,
                        child,
                        &killer_slot,
                    );
                    return;
                }
                DriverAction::InvalidTwoFactor => {
                    finish_with(
                        LoginOutcome::InvalidTwoFactor,
                        &event_tx,
                        child,
                        &killer_slot,
                    );
                    return;
                }
                DriverAction::KeychainUnavailable => {
                    // Never consent automatically to Filen's plaintext fallback.
                    let _ = write_input(&mut writer, "n");
                    finish_with(
                        LoginOutcome::KeychainUnavailable,
                        &event_tx,
                        child,
                        &killer_slot,
                    );
                    return;
                }
            }
        }
    }
}

fn write_input(writer: &mut Box<dyn Write + Send>, value: &str) -> std::io::Result<()> {
    writer.write_all(value.as_bytes())?;
    writer.write_all(b"\r")?;
    writer.flush()
}

fn finish_with(
    outcome: LoginOutcome,
    event_tx: &mpsc::UnboundedSender<LoginOutcome>,
    child: Box<dyn Child + Send + Sync>,
    killer_slot: &Arc<Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>>,
) {
    stop_child(child, killer_slot);
    let _ = event_tx.send(outcome);
}

fn stop_child(
    mut child: Box<dyn Child + Send + Sync>,
    killer_slot: &Arc<Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>>,
) {
    let _ = child.kill();
    if let Ok(mut killer) = killer_slot.lock() {
        *killer = None;
    }

    // Waiting before the PTY handles are dropped can deadlock on macOS while
    // the child is in kernel exit. Reap it independently after this worker
    // unwinds and releases its reader/writer handles.
    let _ = std::thread::Builder::new()
        .name("filen-login-reaper".to_string())
        .spawn(move || {
            let _ = child.wait();
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver() -> LoginPromptDriver {
        LoginPromptDriver::new(
            Zeroizing::new("person@example.com".to_string()),
            Zeroizing::new("secret".to_string()),
        )
    }

    #[test]
    fn login_copy_reports_the_compile_target_platform() {
        assert_eq!(login_copy().platform, std::env::consts::OS);
    }

    #[test]
    fn login_copy_explains_the_manual_cli_alternative() {
        let copy = login_copy();

        assert!(!copy.alternative_title.is_empty());
        assert!(copy.alternative_note.contains("filen"));
        assert!(copy.alternative_note.contains("filen-cli"));
        assert!(copy.alternative_note.contains('y'));
    }

    #[test]
    fn login_window_close_prevents_exactly_one_implicit_exit() {
        let manager = Arc::new(LoginManager::new());
        let (action_tx, _action_rx) = mpsc::unbounded_channel();
        let context = LoginCommandContext::new(manager, action_tx);

        assert!(!context.should_prevent_exit(None));

        context.mark_window_closing();
        assert!(context.should_prevent_exit(None));
        assert!(!context.should_prevent_exit(None));
    }

    #[test]
    fn explicit_exit_is_not_prevented_after_login_window_close() {
        let manager = Arc::new(LoginManager::new());
        let (action_tx, _action_rx) = mpsc::unbounded_channel();
        let context = LoginCommandContext::new(manager, action_tx);

        context.mark_window_closing();
        assert!(!context.should_prevent_exit(Some(0)));
        assert!(!context.should_prevent_exit(None));
    }

    #[test]
    fn legacy_login_prompt_sequence_is_recognized_across_chunks() {
        let mut driver = driver();

        assert_eq!(
            driver.push("Please enter your Filen credentials:\r\nEma"),
            Vec::<DriverAction>::new()
        );
        assert_eq!(driver.push("il: "), vec![DriverAction::SendEmail]);
        assert_eq!(driver.push("Password: "), vec![DriverAction::SendPassword]);
        assert_eq!(
            driver.push("Keep me logged in? (y/N) "),
            vec![DriverAction::SendRemember]
        );
        assert_eq!(
            driver.push("You can delete these credentials using `filen logout`"),
            vec![DriverAction::Success]
        );
    }

    #[test]
    fn legacy_two_factor_flow_is_recognized() {
        let mut driver = driver();
        let _ = driver.push("Email: ");
        let _ = driver.push("Password: ");

        assert_eq!(
            driver.push("Please enter your 2FA code or recovery key: "),
            vec![DriverAction::NeedsTwoFactor]
        );
        assert_eq!(
            driver.push("Invalid Two Factor Authentication code!"),
            vec![DriverAction::InvalidTwoFactor]
        );
    }

    #[test]
    fn rust_cli_prompt_sequence_is_recognized() {
        let mut driver = driver();

        assert_eq!(driver.push("Email: "), vec![DriverAction::SendEmail]);
        assert_eq!(driver.push("Password: "), vec![DriverAction::SendPassword]);
        assert_eq!(
            driver.push("Two-factor authentication code: "),
            vec![DriverAction::NeedsTwoFactor]
        );
        assert_eq!(
            driver.push("Keep me logged in? [Y/n] "),
            vec![DriverAction::SendRemember]
        );
        assert_eq!(
            driver.push("Saved credentials"),
            vec![DriverAction::Success]
        );
    }

    #[test]
    fn invalid_credentials_are_terminal() {
        let mut driver = driver();
        let _ = driver.push("Email: ");
        let _ = driver.push("Password: ");

        assert_eq!(
            driver.push("Invalid credentials!"),
            vec![DriverAction::InvalidCredentials]
        );
        assert!(driver.push("Email: ").is_empty());
    }

    #[test]
    fn keychain_failure_does_not_continue_to_plaintext_fallback() {
        let mut driver = driver();
        let _ = driver.push("Keep me logged in?");

        assert_eq!(
            driver.push("Failed to save credentials in keyring"),
            vec![DriverAction::KeychainUnavailable]
        );
    }

    #[test]
    fn output_buffer_truncation_preserves_utf8_boundaries() {
        let mut driver = driver();
        let unicode_output = "é".repeat(OUTPUT_BUFFER_LIMIT);

        assert!(driver.push(&unicode_output).is_empty());
        assert!(driver.output.is_char_boundary(0));
        assert!(driver.output.len() <= OUTPUT_BUFFER_LIMIT);
    }

    #[cfg(unix)]
    fn fake_cli(script: &str) -> (tempfile::TempDir, FilenCliInfo) {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().expect("create fake CLI directory");
        let path = temp_dir.path().join("filen");
        std::fs::write(&path, script).expect("write fake CLI");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("make fake CLI executable");

        (
            temp_dir,
            FilenCliInfo {
                command: path.to_string_lossy().to_string(),
                path_env: None,
            },
        )
    }

    #[cfg(unix)]
    fn run_fake_cli(
        cli_info: FilenCliInfo,
    ) -> (
        std_mpsc::Sender<LoginInput>,
        mpsc::UnboundedReceiver<LoginOutcome>,
        std::thread::JoinHandle<()>,
    ) {
        let (input_tx, input_rx) = std_mpsc::channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = std::thread::spawn(move || {
            run_login_process(
                cli_info,
                Zeroizing::new("person@example.com".to_string()),
                Zeroizing::new("secret".to_string()),
                input_rx,
                event_tx,
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(None)),
            )
        });
        (input_tx, event_rx, handle)
    }

    #[cfg(unix)]
    #[test]
    fn pty_runner_completes_legacy_login_without_exposing_secrets_as_arguments() {
        let (_temp_dir, cli_info) = fake_cli(
            r#"#!/bin/sh
printf 'Please enter your Filen credentials:\nEmail: '
IFS= read -r email
printf 'Password: '
IFS= read -r password
if [ "$email" != "person@example.com" ] || [ "$password" != "secret" ]; then
  printf 'Invalid credentials!\n'
  exit 1
fi
printf 'Keep me logged in? (y/N) '
IFS= read -r keep
if [ "$keep" != "y" ]; then
  exit 1
fi
printf 'You can delete these credentials using `filen logout`\n'
IFS= read -r command
"#,
        );
        let (_input_tx, mut event_rx, handle) = run_fake_cli(cli_info);

        assert_eq!(
            event_rx.blocking_recv(),
            Some(LoginOutcome::Success),
            "the fake interactive CLI should reach persisted-session success"
        );
        handle.join().expect("login worker should stop");
    }

    #[cfg(unix)]
    #[test]
    fn pty_runner_pauses_for_two_factor_input_and_resumes() {
        let (_temp_dir, cli_info) = fake_cli(
            r#"#!/bin/sh
printf 'Email: '
IFS= read -r email
printf 'Password: '
IFS= read -r password
printf 'Please enter your 2FA code or recovery key: '
IFS= read -r code
if [ "$code" != "123456" ]; then
  printf 'Invalid Two Factor Authentication code!\n'
  exit 1
fi
printf 'Keep me logged in? (y/N) '
IFS= read -r keep
printf 'You can delete these credentials using `filen logout`\n'
IFS= read -r command
"#,
        );
        let (input_tx, mut event_rx, handle) = run_fake_cli(cli_info);

        assert_eq!(event_rx.blocking_recv(), Some(LoginOutcome::NeedsTwoFactor));
        input_tx
            .send(LoginInput::TwoFactor(Zeroizing::new("123456".to_string())))
            .expect("send fake two-factor code");
        assert_eq!(event_rx.blocking_recv(), Some(LoginOutcome::Success));
        handle.join().expect("login worker should stop");
    }
}
