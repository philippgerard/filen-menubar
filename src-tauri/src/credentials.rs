use crate::error::CredentialError;
use std::path::PathBuf;

/// Credential manager that detects Filen CLI's stored session
#[derive(Debug, Clone)]
pub struct CredentialManager {
    data_dir: PathBuf,
}

impl CredentialManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    /// Resolve the classic CLI's normal data directory. Existing users keep
    /// their login and sync state when moving to the bundled patched backend.
    pub fn default_data_dir() -> Option<PathBuf> {
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
                return Some(app_support.join("filen-cli"));
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Check XDG_CONFIG_HOME or ~/.config
            if let Some(config) = dirs::config_dir() {
                return Some(config.join("filen-cli"));
            }
        }

        None
    }

    /// Ensure the explicit `--data-dir` exists before the CLI tries to persist
    /// login or sync state. The classic CLI only creates its implicit default;
    /// an explicitly supplied path is expected to exist already.
    pub fn ensure_data_dir(&self) -> Result<(), CredentialError> {
        std::fs::create_dir_all(&self.data_dir)?;
        Ok(())
    }

    /// Check if the Filen CLI has a stored session
    pub fn cli_session_exists(&self) -> bool {
        let session_file = self.data_dir.join(".filen-cli-keep-me-logged-in");
        if session_file.exists() {
            log::info!("Found Filen CLI session at: {:?}", session_file);
            return true;
        }
        log::debug!("No Filen CLI session found");
        false
    }

    /// Check if credentials exist (either CLI session or environment variables)
    pub fn exists(&self) -> bool {
        // First check for CLI stored session
        if self.cli_session_exists() {
            return true;
        }

        // Fall back to environment variables
        std::env::var("FILEN_EMAIL").is_ok() && std::env::var("FILEN_PASSWORD").is_ok()
    }

    /// Delete stored session (logout from CLI)
    pub fn delete(&self) -> Result<(), CredentialError> {
        let session_file = self.data_dir.join(".filen-cli-keep-me-logged-in");
        if session_file.exists() {
            std::fs::remove_file(&session_file)?;
            log::info!("Deleted Filen CLI session file");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_detection_and_logout_are_scoped_to_the_injected_data_dir() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let managed_dir = temp.path().join("managed");
        let other_dir = temp.path().join("other");
        std::fs::create_dir_all(&managed_dir).expect("create managed directory");
        std::fs::create_dir_all(&other_dir).expect("create other directory");
        let managed_session = managed_dir.join(".filen-cli-keep-me-logged-in");
        let other_session = other_dir.join(".filen-cli-keep-me-logged-in");
        std::fs::write(&managed_session, b"managed").expect("write managed session");
        std::fs::write(&other_session, b"other").expect("write other session");

        let credentials = CredentialManager::new(managed_dir);
        assert!(credentials.cli_session_exists());

        credentials.delete().expect("delete managed session");
        assert!(!managed_session.exists());
        assert!(other_session.exists());
    }

    #[test]
    fn ensure_data_dir_creates_a_missing_nested_directory() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let data_dir = temp.path().join("missing").join("nested");
        let credentials = CredentialManager::new(data_dir.clone());

        assert!(!data_dir.exists());
        credentials
            .ensure_data_dir()
            .expect("create explicit CLI data directory");
        assert!(data_dir.is_dir());
    }
}
