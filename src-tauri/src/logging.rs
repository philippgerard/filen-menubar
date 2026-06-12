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

/// Resolve the effective log level: the RUST_LOG environment variable takes
/// precedence over the configured level
fn resolve_level(log_level: Option<LogLevel>) -> log::LevelFilter {
    use std::str::FromStr;
    std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| LogLevel::from_str(&s).ok())
        .map(LogLevel::to_level_filter)
        .unwrap_or_else(|| log_level.unwrap_or_default().to_level_filter())
}

/// Shared dispatcher setup: log format and noisy-dependency filters
fn base_dispatch(level: log::LevelFilter) -> fern::Dispatch {
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
}

/// Initialize logging
///
/// When `logging_enabled` is true, logs to both file and console.
/// When false, logs only to console (no file created).
/// `RUST_LOG` overrides the configured level in both modes.
///
/// Returns the path to the log file on success (or placeholder if disabled).
pub fn init_logging(
    logging_enabled: bool,
    log_level: Option<LogLevel>,
) -> Result<PathBuf, fern::InitError> {
    let level = resolve_level(log_level);

    // If logging is disabled, only log to console
    if !logging_enabled {
        return init_console_only(level);
    }

    let log_dir = get_log_dir();

    // Create log directory if it doesn't exist
    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("Failed to create log directory {:?}: {}", log_dir, e);
        // Fall back to console-only logging
        return init_console_only(level);
    }

    let log_file = get_log_file_path(&log_dir);

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
            return init_console_only(level);
        }
    };

    base_dispatch(level)
        // Output to both file and stdout
        .chain(file)
        .chain(std::io::stdout())
        .apply()?;

    Ok(log_file)
}

/// Fallback: console-only logging if file logging is disabled or fails
fn init_console_only(level: log::LevelFilter) -> Result<PathBuf, fern::InitError> {
    base_dispatch(level).chain(std::io::stdout()).apply()?;

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
