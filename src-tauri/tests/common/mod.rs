//! Common test utilities and fixtures
//!
//! This module provides shared test infrastructure for integration tests.

use filen_menubar_lib::state::{AppState, SyncState};
use std::path::PathBuf;
use tempfile::TempDir;

/// Test fixture for creating a temporary config environment
pub struct TestEnvironment {
    /// Temporary directory for test files
    pub temp_dir: TempDir,
    /// Application state for testing
    pub app_state: AppState,
}

impl TestEnvironment {
    /// Create a new test environment with fresh state
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let app_state = AppState::new();

        Self { temp_dir, app_state }
    }

    /// Get a path within the temporary directory
    pub fn path(&self, name: &str) -> PathBuf {
        self.temp_dir.path().join(name)
    }
}

impl Default for TestEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait for state to change with timeout
pub async fn wait_for_state(
    app_state: &AppState,
    expected: SyncState,
    timeout_ms: u64,
) -> bool {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    while start.elapsed() < timeout {
        if app_state.get_sync_state().await == expected {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_creates_temp_dir() {
        let env = TestEnvironment::new();
        assert!(env.temp_dir.path().exists());
    }

    #[test]
    fn test_environment_path_helper() {
        let env = TestEnvironment::new();
        let path = env.path("test.txt");
        assert!(path.starts_with(env.temp_dir.path()));
        assert!(path.ends_with("test.txt"));
    }

    #[tokio::test]
    async fn test_wait_for_state_immediate() {
        let env = TestEnvironment::new();
        // Initial state is Starting
        let result = wait_for_state(&env.app_state, SyncState::Starting, 100).await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_wait_for_state_timeout() {
        let env = TestEnvironment::new();
        // Synced is not the initial state, should timeout
        let result = wait_for_state(&env.app_state, SyncState::Synced, 50).await;
        assert!(!result);
    }
}
