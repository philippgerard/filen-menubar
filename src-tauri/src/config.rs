use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to get config directory")]
    NoConfigDir,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Local path to sync
    pub local_path: PathBuf,
    /// Remote path on Filen (usually "/")
    pub remote_path: String,
    /// Sync mode (twoWay, localToCloud, cloudToLocal, localBackup, cloudBackup)
    pub sync_mode: String,
    /// Auto-start sync on launch
    pub auto_start: bool,
}

impl Default for Config {
    fn default() -> Self {
        let local_path = dirs::home_dir()
            .map(|h| h.join("Filen"))
            .unwrap_or_else(|| PathBuf::from("~/Filen"));

        Self {
            local_path,
            remote_path: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            auto_start: true,
        }
    }
}

impl Config {
    /// Get the config file path
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        let config_dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
        let app_config_dir = config_dir.join("filen-menubar");
        Ok(app_config_dir.join("config.json"))
    }

    /// Load config from disk, or create default if not exists
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path()?;

        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            let config: Config = serde_json::from_str(&contents)?;
            Ok(config)
        } else {
            let config = Config::default();
            config.save()?;
            Ok(config)
        }
    }

    /// Save config to disk
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        Ok(())
    }

    /// Get the sync folder path, creating it if necessary
    pub fn ensure_sync_folder(&self) -> Result<PathBuf, ConfigError> {
        if !self.local_path.exists() {
            std::fs::create_dir_all(&self.local_path)?;
        }
        Ok(self.local_path.clone())
    }
}
