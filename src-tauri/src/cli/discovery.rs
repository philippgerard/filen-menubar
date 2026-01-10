//! CLI binary discovery
//!
//! This module handles finding the Filen CLI binary on the system.
//! GUI apps launched from Finder/desktop don't inherit shell PATH,
//! so we need to search common installation locations.

use std::path::PathBuf;

/// Information about the filen CLI location
pub struct FilenCliInfo {
    /// Path to the filen binary
    pub command: String,
    /// PATH environment variable to use (includes node binary directory)
    pub path_env: Option<String>,
}

/// Find the filen CLI binary by searching common installation paths.
///
/// This is necessary because GUI apps launched from Finder don't inherit shell PATH.
/// Returns both the filen path and the PATH env needed to run it (for node-based installs).
pub fn find_filen_cli() -> FilenCliInfo {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!("Could not determine home directory");
            return FilenCliInfo {
                command: "filen".to_string(),
                path_env: None,
            };
        }
    };

    // Common installation paths to search (with their bin directories for PATH)
    let search_paths: Vec<(PathBuf, Option<PathBuf>)> = vec![
        // Standard system paths - node should be in system PATH
        (
            PathBuf::from("/usr/local/bin/filen"),
            Some(PathBuf::from("/usr/local/bin")),
        ),
        (
            PathBuf::from("/opt/homebrew/bin/filen"),
            Some(PathBuf::from("/opt/homebrew/bin")),
        ),
        // Official Filen CLI installer path (curl -sL https://filen.io/cli.sh | bash)
        (
            home.join(".filen-cli/bin/filen"),
            Some(home.join(".filen-cli/bin")),
        ),
        // User local bin
        (home.join(".local/bin/filen"), Some(home.join(".local/bin"))),
        // npm global installs
        (home.join(".npm/bin/filen"), Some(home.join(".npm/bin"))),
        (
            home.join(".npm-global/bin/filen"),
            Some(home.join(".npm-global/bin")),
        ),
    ];

    // Check standard paths first
    for (filen_path, bin_dir) in &search_paths {
        if filen_path.exists() {
            log::info!("Found filen CLI at: {:?}", filen_path);
            let path_env = bin_dir.as_ref().map(|d| build_path_env(d));
            return FilenCliInfo {
                command: filen_path.to_string_lossy().to_string(),
                path_env,
            };
        }
    }

    // Search fnm (Fast Node Manager) installations
    let fnm_base = home.join(".local/share/fnm/node-versions");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("installation/bin");
                let filen_path = bin_dir.join("filen");
                if filen_path.exists() {
                    let path_env = build_path_env(&bin_dir);
                    log::info!("Found filen CLI in fnm at: {:?}", filen_path);
                    return FilenCliInfo {
                        command: filen_path.to_string_lossy().to_string(),
                        path_env: Some(path_env),
                    };
                }
            }
        }
    }

    // Search nvm (Node Version Manager) installations
    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("bin");
                let filen_path = bin_dir.join("filen");
                if filen_path.exists() {
                    log::info!("Found filen CLI in nvm at: {:?}", filen_path);
                    return FilenCliInfo {
                        command: filen_path.to_string_lossy().to_string(),
                        path_env: Some(build_path_env(&bin_dir)),
                    };
                }
            }
        }
    }

    // Search volta installations
    let volta_bin = home.join(".volta/bin");
    let volta_filen = volta_bin.join("filen");
    if volta_filen.exists() {
        log::info!("Found filen CLI in volta at: {:?}", volta_filen);
        return FilenCliInfo {
            command: volta_filen.to_string_lossy().to_string(),
            path_env: Some(build_path_env(&volta_bin)),
        };
    }

    // Fallback to just "filen" (will use PATH if available)
    log::warn!("filen CLI not found in common paths, falling back to PATH lookup");
    FilenCliInfo {
        command: "filen".to_string(),
        path_env: None,
    }
}

/// Find node binary in common version manager locations
fn find_node_bin_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    // Check fnm (Fast Node Manager)
    let fnm_base = home.join(".local/share/fnm/node-versions");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("installation/bin");
                if bin_dir.join("node").exists() {
                    log::debug!("Found node in fnm at: {:?}", bin_dir);
                    return Some(bin_dir);
                }
            }
        }
    }

    // Check nvm (Node Version Manager)
    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let bin_dir = entry.path().join("bin");
                if bin_dir.join("node").exists() {
                    log::debug!("Found node in nvm at: {:?}", bin_dir);
                    return Some(bin_dir);
                }
            }
        }
    }

    // Check volta
    let volta_bin = home.join(".volta/bin");
    if volta_bin.join("node").exists() {
        log::debug!("Found node in volta at: {:?}", volta_bin);
        return Some(volta_bin);
    }

    None
}

/// Build a PATH environment variable that includes the given bin directory
/// along with essential system paths and node binary location
fn build_path_env(bin_dir: &std::path::Path) -> String {
    let system_paths = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

    // Check if bin_dir already contains node
    if bin_dir.join("node").exists() {
        return format!("{}:{}", bin_dir.display(), system_paths);
    }

    // Try to find node in version managers
    if let Some(node_bin_dir) = find_node_bin_dir() {
        return format!(
            "{}:{}:{}",
            bin_dir.display(),
            node_bin_dir.display(),
            system_paths
        );
    }

    format!("{}:{}", bin_dir.display(), system_paths)
}
