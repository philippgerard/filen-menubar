//! File-based logging for the current app session
//!
//! Logs are written to a single file that is overwritten on each app launch.
//! This keeps log files small and focused on the current session for easier debugging.

use crate::config::LogLevel;
use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

/// Get the platform-specific log directory
pub fn get_log_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .map(|h| h.join("Library/Logs/io.filen.menubar"))
            .unwrap_or_else(|| PathBuf::from("/tmp/filen-menubar/logs"))
    }

    #[cfg(target_os = "linux")]
    {
        dirs::data_local_dir()
            .map(|d| d.join("filen-menubar/logs"))
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .map(|h| h.join(".local/share/filen-menubar/logs"))
                    .unwrap_or_else(|| PathBuf::from("/tmp/filen-menubar/logs"))
            })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        dirs::data_local_dir()
            .map(|d| d.join("filen-menubar/logs"))
            .unwrap_or_else(|| PathBuf::from("./logs"))
    }
}

/// Get the log file path (single file, overwritten each launch)
fn get_log_file_path(log_dir: &Path) -> PathBuf {
    log_dir.join("filen.log")
}

/// Initialize logging
///
/// When `logging_enabled` is true, logs to both file and console.
/// When false, logs only to console (no file created).
///
/// Returns the path to the log file on success (or placeholder if disabled).
pub fn init_logging(
    logging_enabled: bool,
    log_level: Option<LogLevel>,
) -> Result<PathBuf, fern::InitError> {
    // If logging is disabled, only log to console
    if !logging_enabled {
        return init_console_only(log_level);
    }

    let log_dir = get_log_dir();

    // Create log directory if it doesn't exist
    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("Failed to create log directory {:?}: {}", log_dir, e);
        // Fall back to console-only logging
        return init_console_only(log_level);
    }

    let log_file = get_log_file_path(&log_dir);

    // Get log level from config or use default
    let level = log_level.unwrap_or_default().to_level_filter();

    // Also check RUST_LOG environment variable (takes precedence)
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| match s.to_lowercase().as_str() {
            "trace" => Some(log::LevelFilter::Trace),
            "debug" => Some(log::LevelFilter::Debug),
            "info" => Some(log::LevelFilter::Info),
            "warn" => Some(log::LevelFilter::Warn),
            "error" => Some(log::LevelFilter::Error),
            _ => None,
        })
        .unwrap_or(level);

    // Open log file, truncating any existing content (fresh log each launch)
    let file = match fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_file)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open log file {:?}: {}", log_file, e);
            return init_console_only(log_level);
        }
    };

    // Setup fern dispatcher
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(level)
        // Filter out noisy dependencies
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("mio", log::LevelFilter::Warn)
        .level_for("tokio", log::LevelFilter::Warn)
        // Output to both file and stdout
        .chain(file)
        .chain(std::io::stdout())
        .apply()?;

    Ok(log_file)
}

/// Fallback: console-only logging if file logging fails
fn init_console_only(log_level: Option<LogLevel>) -> Result<PathBuf, fern::InitError> {
    let level = log_level.unwrap_or_default().to_level_filter();

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(level)
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("mio", log::LevelFilter::Warn)
        .level_for("tokio", log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .apply()?;

    // Return a placeholder path
    Ok(PathBuf::from("(console only)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_log_dir_not_empty() {
        let log_dir = get_log_dir();
        assert!(!log_dir.as_os_str().is_empty());
    }

    #[test]
    fn test_get_log_file_path_format() {
        let log_dir = PathBuf::from("/tmp/test-logs");
        let log_file = get_log_file_path(&log_dir);

        let filename = log_file.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "filen.log");
    }
}
