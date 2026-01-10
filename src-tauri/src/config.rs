use crate::error::ConfigError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// Locale override (e.g., "en", "de"). If None, uses system locale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Enable file logging. Default: false (disabled)
    #[serde(default)]
    pub logging_enabled: bool,
    /// Log level (trace, debug, info, warn, error). Default: info. Only used when logging_enabled is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    /// Patterns to ignore during sync (e.g., ["Photos", "*.tmp", "node_modules"])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Exclude dot files (files/folders starting with .)
    #[serde(default)]
    pub exclude_dot_files: bool,
}

/// Sync pair configuration for Filen CLI's syncPairs.json format
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPair {
    /// Local absolute path to sync
    pub local: String,
    /// Remote path in Filen Drive (cloud path)
    pub remote: String,
    /// Synchronization mode
    pub sync_mode: String,
    /// Alias name for this sync pair
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// If true, bypass local trash when deleting files
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disable_local_trash: bool,
    /// Patterns to ignore during sync
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// If true, exclude hidden files (starting with dot)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exclude_dot_files: bool,
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
            locale: None,
            logging_enabled: false,
            log_level: None,
            ignore: Vec::new(),
            exclude_dot_files: false,
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

    /// Get the path for the generated syncPairs.json file
    /// Stored in filen-menubar's config directory to avoid conflicts with CLI's own config
    pub fn sync_pairs_path() -> Result<PathBuf, ConfigError> {
        let config_dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
        let app_config_dir = config_dir.join("filen-menubar");
        Ok(app_config_dir.join("syncPairs.json"))
    }

    /// Generate and write the syncPairs.json file for the Filen CLI
    pub fn write_sync_pairs(&self) -> Result<PathBuf, ConfigError> {
        let sync_pair = SyncPair {
            local: self.local_path.to_string_lossy().to_string(),
            remote: self.remote_path.clone(),
            sync_mode: self.sync_mode.clone(),
            alias: Some("filen-menubar".to_string()),
            disable_local_trash: false,
            ignore: self.ignore.clone(),
            exclude_dot_files: self.exclude_dot_files,
        };

        let pairs = vec![sync_pair];
        let path = Self::sync_pairs_path()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(&pairs)?;
        std::fs::write(&path, contents)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Default values tests ====================

    #[test]
    fn test_config_default_sync_mode() {
        let config = Config::default();
        assert_eq!(config.sync_mode, "twoWay");
    }

    #[test]
    fn test_config_default_remote_path() {
        let config = Config::default();
        assert_eq!(config.remote_path, "/");
    }

    #[test]
    fn test_config_default_auto_start() {
        let config = Config::default();
        assert!(config.auto_start);
    }

    #[test]
    fn test_config_default_locale_is_none() {
        let config = Config::default();
        assert!(config.locale.is_none());
    }

    #[test]
    fn test_config_default_logging_enabled_is_false() {
        let config = Config::default();
        assert!(!config.logging_enabled);
    }

    #[test]
    fn test_config_default_log_level_is_none() {
        let config = Config::default();
        assert!(config.log_level.is_none());
    }

    #[test]
    fn test_config_default_local_path_ends_with_filen() {
        let config = Config::default();
        assert!(config.local_path.ends_with("Filen"));
    }

    // ==================== Serialization tests ====================

    #[test]
    fn test_config_serde_roundtrip() {
        let config = Config {
            local_path: PathBuf::from("/home/user/MySyncFolder"),
            remote_path: "/Documents".to_string(),
            sync_mode: "localToCloud".to_string(),
            auto_start: false,
            locale: Some("de".to_string()),
            logging_enabled: true,
            log_level: Some("debug".to_string()),
            ignore: vec!["*.tmp".to_string(), "node_modules".to_string()],
            exclude_dot_files: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.local_path, config.local_path);
        assert_eq!(deserialized.remote_path, config.remote_path);
        assert_eq!(deserialized.sync_mode, config.sync_mode);
        assert_eq!(deserialized.auto_start, config.auto_start);
        assert_eq!(deserialized.locale, config.locale);
        assert_eq!(deserialized.logging_enabled, config.logging_enabled);
        assert_eq!(deserialized.log_level, config.log_level);
        assert_eq!(deserialized.ignore, config.ignore);
        assert_eq!(deserialized.exclude_dot_files, config.exclude_dot_files);
    }

    #[test]
    fn test_config_camel_case_serialization() {
        let config = Config {
            local_path: PathBuf::from("/test"),
            remote_path: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            auto_start: true,
            locale: None,
            logging_enabled: false,
            log_level: None,
            ignore: vec!["Photos".to_string()],
            exclude_dot_files: true,
        };

        let json = serde_json::to_string(&config).unwrap();

        // Should use camelCase in JSON
        assert!(json.contains("localPath"));
        assert!(json.contains("remotePath"));
        assert!(json.contains("syncMode"));
        assert!(json.contains("autoStart"));
        assert!(json.contains("loggingEnabled"));
        assert!(json.contains("excludeDotFiles"));

        // Should NOT contain snake_case
        assert!(!json.contains("local_path"));
        assert!(!json.contains("remote_path"));
        assert!(!json.contains("sync_mode"));
        assert!(!json.contains("auto_start"));
        assert!(!json.contains("logging_enabled"));
        assert!(!json.contains("exclude_dot_files"));
    }

    #[test]
    fn test_config_locale_skipped_when_none() {
        let config = Config {
            local_path: PathBuf::from("/test"),
            remote_path: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            auto_start: true,
            locale: None,
            logging_enabled: false,
            log_level: None,
            ignore: Vec::new(),
            exclude_dot_files: false,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("locale"));
    }

    #[test]
    fn test_config_locale_included_when_some() {
        let config = Config {
            local_path: PathBuf::from("/test"),
            remote_path: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            auto_start: true,
            locale: Some("en".to_string()),
            logging_enabled: false,
            log_level: None,
            ignore: Vec::new(),
            exclude_dot_files: false,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"locale\":\"en\""));
    }

    #[test]
    fn test_config_deserialize_from_camel_case() {
        let json = r#"{
            "localPath": "/Users/test/Filen",
            "remotePath": "/Backup",
            "syncMode": "cloudToLocal",
            "autoStart": false
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.local_path, PathBuf::from("/Users/test/Filen"));
        assert_eq!(config.remote_path, "/Backup");
        assert_eq!(config.sync_mode, "cloudToLocal");
        assert!(!config.auto_start);
        assert!(config.locale.is_none());
    }

    // ==================== File I/O tests with tempfile ====================

    #[test]
    fn test_config_save_and_load_with_tempdir() {
        // Create a temporary directory
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().join("filen-menubar");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        // Create a config with custom values
        let original_config = Config {
            local_path: PathBuf::from("/custom/sync/path"),
            remote_path: "/MyFolder".to_string(),
            sync_mode: "localBackup".to_string(),
            auto_start: false,
            locale: Some("fr".to_string()),
            logging_enabled: true,
            log_level: Some("debug".to_string()),
            ignore: vec!["*.log".to_string()],
            exclude_dot_files: true,
        };

        // Save directly to temp path
        let contents = serde_json::to_string_pretty(&original_config).unwrap();
        std::fs::write(&config_path, contents).unwrap();

        // Load from temp path
        let loaded_contents = std::fs::read_to_string(&config_path).unwrap();
        let loaded_config: Config = serde_json::from_str(&loaded_contents).unwrap();

        assert_eq!(loaded_config.local_path, original_config.local_path);
        assert_eq!(loaded_config.remote_path, original_config.remote_path);
        assert_eq!(loaded_config.sync_mode, original_config.sync_mode);
        assert_eq!(loaded_config.auto_start, original_config.auto_start);
        assert_eq!(loaded_config.locale, original_config.locale);

        // Cleanup is automatic when temp_dir goes out of scope
    }

    #[test]
    fn test_ensure_sync_folder_creates_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_path = temp_dir.path().join("new_sync_folder");

        let config = Config {
            local_path: sync_path.clone(),
            remote_path: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            auto_start: true,
            locale: None,
            logging_enabled: false,
            log_level: None,
            ignore: Vec::new(),
            exclude_dot_files: false,
        };

        // Directory should not exist yet
        assert!(!sync_path.exists());

        // ensure_sync_folder should create it
        let result = config.ensure_sync_folder();
        assert!(result.is_ok());
        assert!(sync_path.exists());
        assert!(sync_path.is_dir());
    }

    #[test]
    fn test_ensure_sync_folder_idempotent() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sync_path = temp_dir.path().join("existing_folder");
        std::fs::create_dir_all(&sync_path).unwrap();

        let config = Config {
            local_path: sync_path.clone(),
            remote_path: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            auto_start: true,
            locale: None,
            logging_enabled: false,
            log_level: None,
            ignore: Vec::new(),
            exclude_dot_files: false,
        };

        // Should succeed even if directory already exists
        let result = config.ensure_sync_folder();
        assert!(result.is_ok());
        assert!(sync_path.exists());
    }

    // ==================== Ignore pattern tests ====================

    #[test]
    fn test_config_default_ignore_is_empty() {
        let config = Config::default();
        assert!(config.ignore.is_empty());
    }

    #[test]
    fn test_config_default_exclude_dot_files_is_false() {
        let config = Config::default();
        assert!(!config.exclude_dot_files);
    }

    #[test]
    fn test_config_ignore_skipped_when_empty() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("ignore"));
    }

    #[test]
    fn test_config_backward_compatibility_without_ignore() {
        // Old config without ignore fields should deserialize with defaults
        let json = r#"{
            "localPath": "/Users/test/Filen",
            "remotePath": "/",
            "syncMode": "twoWay",
            "autoStart": true
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.ignore.is_empty());
        assert!(!config.exclude_dot_files);
    }

    #[test]
    fn test_config_with_ignore_patterns() {
        let json = r#"{
            "localPath": "/Users/test/Filen",
            "remotePath": "/",
            "syncMode": "twoWay",
            "autoStart": true,
            "ignore": ["Photos", "*.tmp", "node_modules"],
            "excludeDotFiles": true
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.ignore.len(), 3);
        assert!(config.ignore.contains(&"Photos".to_string()));
        assert!(config.ignore.contains(&"*.tmp".to_string()));
        assert!(config.ignore.contains(&"node_modules".to_string()));
        assert!(config.exclude_dot_files);
    }

    #[test]
    fn test_sync_pair_serialization() {
        let pair = SyncPair {
            local: "/home/user/Filen".to_string(),
            remote: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            alias: Some("main".to_string()),
            disable_local_trash: false,
            ignore: vec!["*.log".to_string(), "Photos".to_string()],
            exclude_dot_files: true,
        };

        let json = serde_json::to_string_pretty(&[pair]).unwrap();
        assert!(json.contains("\"local\""));
        assert!(json.contains("\"remote\""));
        assert!(json.contains("\"syncMode\""));
        assert!(json.contains("\"alias\""));
        assert!(json.contains("\"ignore\""));
        assert!(json.contains("\"excludeDotFiles\""));
        // disableLocalTrash should be skipped when false
        assert!(!json.contains("disableLocalTrash"));
    }

    #[test]
    fn test_sync_pair_skips_empty_ignore() {
        let pair = SyncPair {
            local: "/home/user/Filen".to_string(),
            remote: "/".to_string(),
            sync_mode: "twoWay".to_string(),
            alias: None,
            disable_local_trash: false,
            ignore: Vec::new(),
            exclude_dot_files: false,
        };

        let json = serde_json::to_string(&pair).unwrap();
        // Empty vectors and false booleans should be skipped
        assert!(!json.contains("ignore"));
        assert!(!json.contains("excludeDotFiles"));
        assert!(!json.contains("disableLocalTrash"));
        assert!(!json.contains("alias"));
    }
}
