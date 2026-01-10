use crate::error::CredentialError;
use std::path::PathBuf;

/// Credential manager that detects Filen CLI's stored session
pub struct CredentialManager;

impl CredentialManager {
    /// Get the Filen CLI data directory
    fn cli_data_dir() -> Option<PathBuf> {
        // Check for .filen-cli in home directory first (install script location)
        if let Some(home) = dirs::home_dir() {
            let dotdir = home.join(".filen-cli");
            if dotdir.exists() {
                return Some(dotdir);
            }
        }

        // Then check platform-specific locations
        #[cfg(target_os = "macos")]
        {
            if let Some(app_support) = dirs::data_dir() {
                let cli_dir = app_support.join("filen-cli");
                if cli_dir.exists() {
                    return Some(cli_dir);
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Check XDG_CONFIG_HOME or ~/.config
            if let Some(config) = dirs::config_dir() {
                let cli_dir = config.join("filen-cli");
                if cli_dir.exists() {
                    return Some(cli_dir);
                }
            }
        }

        None
    }

    /// Check if the Filen CLI has a stored session
    pub fn cli_session_exists() -> bool {
        if let Some(data_dir) = Self::cli_data_dir() {
            let session_file = data_dir.join(".filen-cli-keep-me-logged-in");
            if session_file.exists() {
                log::info!("Found Filen CLI session at: {:?}", session_file);
                return true;
            }
        }
        log::debug!("No Filen CLI session found");
        false
    }

    /// Check if credentials exist (either CLI session or environment variables)
    pub fn exists() -> bool {
        // First check for CLI stored session
        if Self::cli_session_exists() {
            return true;
        }

        // Fall back to environment variables
        std::env::var("FILEN_EMAIL").is_ok() && std::env::var("FILEN_PASSWORD").is_ok()
    }

    /// Delete stored session (logout from CLI)
    pub fn delete() -> Result<(), CredentialError> {
        if let Some(data_dir) = Self::cli_data_dir() {
            let session_file = data_dir.join(".filen-cli-keep-me-logged-in");
            if session_file.exists() {
                std::fs::remove_file(&session_file)?;
                log::info!("Deleted Filen CLI session file");
            }
        }
        Ok(())
    }
}
