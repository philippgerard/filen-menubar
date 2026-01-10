//! Unified error handling for the application
//!
//! This module provides a single `AppError` type that consolidates all error types
//! from different modules, enabling consistent error propagation with the `?` operator.

use thiserror::Error;

/// Unified application error type
///
/// This enum provides a single error type that can represent any error in the application,
/// enabling consistent error handling with the `?` operator throughout the codebase.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// CLI process errors
    #[error("CLI error: {0}")]
    Cli(#[from] CliError),

    /// Credential/authentication errors
    #[error("Credential error: {0}")]
    Credential(#[from] CredentialError),

    /// Generic IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience type alias for Results using AppError
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, AppError>;

/// Configuration-related errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to get config directory")]
    NoConfigDir,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid sync mode: {0}. Valid values: twoWay, localToCloud, cloudToLocal, localBackup, cloudBackup")]
    InvalidSyncMode(String),

    #[error("Invalid log level: {0}. Valid values: trace, debug, info, warn, error")]
    InvalidLogLevel(String),
}

/// CLI process errors
#[derive(Error, Debug)]
pub enum CliError {
    #[error("Failed to spawn CLI process: {0}")]
    Spawn(std::io::Error),

    #[allow(dead_code)]
    #[error("CLI not found. Please install filen-cli")]
    NotFound,

    #[allow(dead_code)]
    #[error("CLI process exited unexpectedly")]
    ProcessExited,

    #[error("Failed to write sync pairs: {0}")]
    SyncPairs(String),
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::Spawn(err)
    }
}

/// Credential/authentication errors
#[derive(Error, Debug)]
pub enum CredentialError {
    #[allow(dead_code)]
    #[error("Credentials not found")]
    NotFound,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::NoConfigDir;
        assert_eq!(err.to_string(), "Failed to get config directory");
    }

    #[test]
    fn test_cli_error_display() {
        let err = CliError::NotFound;
        assert_eq!(err.to_string(), "CLI not found. Please install filen-cli");
    }

    #[test]
    fn test_credential_error_display() {
        let err = CredentialError::NotFound;
        assert_eq!(err.to_string(), "Credentials not found");
    }

    #[test]
    fn test_app_error_from_config_error() {
        let config_err = ConfigError::NoConfigDir;
        let app_err: AppError = config_err.into();
        assert!(matches!(app_err, AppError::Config(_)));
    }

    #[test]
    fn test_app_error_from_cli_error() {
        let cli_err = CliError::NotFound;
        let app_err: AppError = cli_err.into();
        assert!(matches!(app_err, AppError::Cli(_)));
    }

    #[test]
    fn test_app_error_from_credential_error() {
        let cred_err = CredentialError::NotFound;
        let app_err: AppError = cred_err.into();
        assert!(matches!(app_err, AppError::Credential(_)));
    }

    #[test]
    fn test_app_error_display_includes_source() {
        let config_err = ConfigError::NoConfigDir;
        let app_err: AppError = config_err.into();
        assert!(app_err.to_string().contains("Configuration error"));
    }
}
