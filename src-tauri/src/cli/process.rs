//! Process abstraction for CLI execution
//!
//! This module provides a trait-based abstraction for spawning and managing
//! CLI processes, enabling dependency injection for testing.
//!
//! ## Usage
//!
//! The `ProcessRunner` trait can be used to inject different implementations:
//! - `TokioProcessRunner`: Real implementation using tokio::process
//! - `MockProcessRunner` (in tests): Simulates process output for testing
//!
//! This is currently set up for future use when refactoring CliManager
//! to accept a generic ProcessRunner.

use async_trait::async_trait;
use std::io;
use std::process::ExitStatus;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// Output from a running process
#[allow(dead_code)]
pub struct ProcessOutput {
    /// Lines from stdout
    pub stdout_lines: Vec<String>,
    /// Lines from stderr
    pub stderr_lines: Vec<String>,
    /// Exit status (if process completed)
    pub status: Option<ExitStatus>,
}

/// Trait for running CLI processes
///
/// This abstraction allows injecting mock implementations for testing
/// without actually spawning real processes.
#[async_trait]
#[allow(dead_code)]
pub trait ProcessRunner: Send + Sync {
    /// Check if a command is available (returns true if it can be executed)
    async fn is_available(&self, command: &str, args: &[&str]) -> bool;

    /// Spawn a process and return a handle for monitoring
    async fn spawn(
        &self,
        command: &str,
        args: &[&str],
        env: Option<(&str, &str)>,
    ) -> io::Result<Box<dyn ProcessHandle>>;
}

/// Handle to a running process
#[async_trait]
#[allow(dead_code)]
pub trait ProcessHandle: Send + Sync {
    /// Read the next line from stdout (returns None on EOF)
    async fn read_stdout_line(&mut self) -> io::Result<Option<String>>;

    /// Read the next line from stderr (returns None on EOF)
    async fn read_stderr_line(&mut self) -> io::Result<Option<String>>;

    /// Kill the process
    async fn kill(&mut self) -> io::Result<()>;

    /// Check if the process is still running
    fn is_running(&self) -> bool;
}

/// Real implementation using tokio::process
#[allow(dead_code)]
pub struct TokioProcessRunner;

impl TokioProcessRunner {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl Default for TokioProcessRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessRunner for TokioProcessRunner {
    async fn is_available(&self, command: &str, args: &[&str]) -> bool {
        let result = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;

        matches!(result, Ok(status) if status.success())
    }

    async fn spawn(
        &self,
        command: &str,
        args: &[&str],
        env: Option<(&str, &str)>,
    ) -> io::Result<Box<dyn ProcessHandle>> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        if let Some((key, value)) = env {
            cmd.env(key, value);
        }

        let child = cmd.spawn()?;
        Ok(Box::new(TokioProcessHandle::new(child)))
    }
}

/// Handle to a real tokio process
#[allow(dead_code)]
pub struct TokioProcessHandle {
    child: Child,
    stdout: Option<tokio::io::Lines<BufReader<tokio::process::ChildStdout>>>,
    stderr: Option<tokio::io::Lines<BufReader<tokio::process::ChildStderr>>>,
    running: bool,
}

impl TokioProcessHandle {
    #[allow(dead_code)]
    fn new(mut child: Child) -> Self {
        let stdout = child.stdout.take().map(|s| BufReader::new(s).lines());
        let stderr = child.stderr.take().map(|s| BufReader::new(s).lines());

        Self {
            child,
            stdout,
            stderr,
            running: true,
        }
    }
}

#[async_trait]
impl ProcessHandle for TokioProcessHandle {
    async fn read_stdout_line(&mut self) -> io::Result<Option<String>> {
        if let Some(ref mut lines) = self.stdout {
            match lines.next_line().await {
                Ok(Some(line)) => Ok(Some(line)),
                Ok(None) => {
                    self.running = false;
                    Ok(None)
                }
                Err(e) => {
                    self.running = false;
                    Err(e)
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn read_stderr_line(&mut self) -> io::Result<Option<String>> {
        if let Some(ref mut lines) = self.stderr {
            lines.next_line().await
        } else {
            Ok(None)
        }
    }

    async fn kill(&mut self) -> io::Result<()> {
        self.running = false;
        self.child.kill().await
    }

    fn is_running(&self) -> bool {
        self.running
    }
}

#[cfg(test)]
pub mod mock {
    //! Mock implementations for testing

    use super::*;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Mock process runner for testing
    pub struct MockProcessRunner {
        available: bool,
        stdout_lines: Arc<Mutex<VecDeque<String>>>,
        stderr_lines: Arc<Mutex<VecDeque<String>>>,
    }

    impl MockProcessRunner {
        pub fn new(available: bool) -> Self {
            Self {
                available,
                stdout_lines: Arc::new(Mutex::new(VecDeque::new())),
                stderr_lines: Arc::new(Mutex::new(VecDeque::new())),
            }
        }

        /// Add lines that will be returned from stdout
        pub async fn add_stdout_lines(&self, lines: Vec<String>) {
            let mut stdout = self.stdout_lines.lock().await;
            stdout.extend(lines);
        }

        /// Add lines that will be returned from stderr
        pub async fn add_stderr_lines(&self, lines: Vec<String>) {
            let mut stderr = self.stderr_lines.lock().await;
            stderr.extend(lines);
        }
    }

    #[async_trait]
    impl ProcessRunner for MockProcessRunner {
        async fn is_available(&self, _command: &str, _args: &[&str]) -> bool {
            self.available
        }

        async fn spawn(
            &self,
            _command: &str,
            _args: &[&str],
            _env: Option<(&str, &str)>,
        ) -> io::Result<Box<dyn ProcessHandle>> {
            if !self.available {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Mock: command not available",
                ));
            }

            Ok(Box::new(MockProcessHandle {
                stdout_lines: self.stdout_lines.clone(),
                stderr_lines: self.stderr_lines.clone(),
                running: true,
            }))
        }
    }

    /// Mock process handle for testing
    pub struct MockProcessHandle {
        stdout_lines: Arc<Mutex<VecDeque<String>>>,
        stderr_lines: Arc<Mutex<VecDeque<String>>>,
        running: bool,
    }

    #[async_trait]
    impl ProcessHandle for MockProcessHandle {
        async fn read_stdout_line(&mut self) -> io::Result<Option<String>> {
            let mut lines = self.stdout_lines.lock().await;
            Ok(lines.pop_front())
        }

        async fn read_stderr_line(&mut self) -> io::Result<Option<String>> {
            let mut lines = self.stderr_lines.lock().await;
            Ok(lines.pop_front())
        }

        async fn kill(&mut self) -> io::Result<()> {
            self.running = false;
            Ok(())
        }

        fn is_running(&self) -> bool {
            self.running
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tokio_runner_check_nonexistent_command() {
        let runner = TokioProcessRunner::new();
        let available = runner
            .is_available("nonexistent_command_12345", &["--version"])
            .await;
        assert!(!available);
    }

    #[tokio::test]
    async fn test_tokio_runner_default() {
        let runner = TokioProcessRunner;
        // Just verify it can be constructed
        let _ = runner;
    }

    #[tokio::test]
    async fn test_mock_runner_available() {
        let runner = mock::MockProcessRunner::new(true);
        assert!(runner.is_available("any", &[]).await);
    }

    #[tokio::test]
    async fn test_mock_runner_not_available() {
        let runner = mock::MockProcessRunner::new(false);
        assert!(!runner.is_available("any", &[]).await);
    }

    #[tokio::test]
    async fn test_mock_runner_spawn_when_available() {
        let runner = mock::MockProcessRunner::new(true);
        let result = runner.spawn("test", &[], None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_runner_spawn_when_not_available() {
        let runner = mock::MockProcessRunner::new(false);
        let result = runner.spawn("test", &[], None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_handle_stdout_lines() {
        let runner = mock::MockProcessRunner::new(true);
        runner
            .add_stdout_lines(vec!["line1".to_string(), "line2".to_string()])
            .await;

        let mut handle = runner.spawn("test", &[], None).await.unwrap();

        assert_eq!(
            handle.read_stdout_line().await.unwrap(),
            Some("line1".to_string())
        );
        assert_eq!(
            handle.read_stdout_line().await.unwrap(),
            Some("line2".to_string())
        );
        assert_eq!(handle.read_stdout_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_mock_handle_stderr_lines() {
        let runner = mock::MockProcessRunner::new(true);
        runner.add_stderr_lines(vec!["error1".to_string()]).await;

        let mut handle = runner.spawn("test", &[], None).await.unwrap();

        assert_eq!(
            handle.read_stderr_line().await.unwrap(),
            Some("error1".to_string())
        );
        assert_eq!(handle.read_stderr_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_mock_handle_kill() {
        let runner = mock::MockProcessRunner::new(true);
        let mut handle = runner.spawn("test", &[], None).await.unwrap();

        assert!(handle.is_running());
        handle.kill().await.unwrap();
        assert!(!handle.is_running());
    }
}
