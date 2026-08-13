//! Runtime description for the Filen CLI bundled with the application.
//!
//! The backend is an app-private resource, not a command discovered on PATH.
//! Keeping one immutable runtime description ensures version checks, login,
//! session detection, and syncing all address the same executable and data
//! directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

const BUNDLED_CLI_VERSION_MANIFEST: &str = include_str!("../../../third-party/filen-cli/VERSION");

pub fn bundled_cli_version() -> &'static str {
    BUNDLED_CLI_VERSION_MANIFEST.trim()
}

pub fn bundled_cli_version_output() -> String {
    format!("Filen CLI {}", bundled_cli_version())
}

/// The exact Node executable, CLI entrypoint, and writable data directory used
/// by the bundled backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilenCliRuntime {
    command: PathBuf,
    entrypoint: PathBuf,
    data_dir: PathBuf,
}

impl FilenCliRuntime {
    pub fn new(command: PathBuf, entrypoint: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            command,
            entrypoint,
            data_dir,
        }
    }

    pub fn command(&self) -> &Path {
        &self.command
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn entrypoint(&self) -> &Path {
        &self.entrypoint
    }

    /// Arguments shared by every invocation. The patched backend also has its
    /// updater disabled at compile time; `--skip-update` is defense in depth.
    pub fn common_args(&self) -> [OsString; 5] {
        [
            OsString::from("--disable-warning=DEP0169"),
            self.entrypoint.as_os_str().to_owned(),
            OsString::from("--skip-update"),
            OsString::from("--data-dir"),
            self.data_dir.as_os_str().to_owned(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_arguments_keep_paths_with_spaces_as_one_argument() {
        let runtime = FilenCliRuntime::new(
            PathBuf::from("/Applications/Filen Menubar.app/node"),
            PathBuf::from("/Applications/Filen Menubar.app/filen-cli.cjs"),
            PathBuf::from("/tmp/Filen CLI Data"),
        );

        assert_eq!(
            runtime.common_args(),
            [
                OsString::from("--disable-warning=DEP0169"),
                OsString::from("/Applications/Filen Menubar.app/filen-cli.cjs"),
                OsString::from("--skip-update"),
                OsString::from("--data-dir"),
                OsString::from("/tmp/Filen CLI Data"),
            ]
        );
    }

    #[test]
    fn bundled_version_comes_from_the_checked_in_manifest() {
        assert_eq!(bundled_cli_version(), "v0.0.39-menubar.2");
        assert_eq!(bundled_cli_version_output(), "Filen CLI v0.0.39-menubar.2");
    }
}
