//! Network error detection
//!
//! This module provides utilities for detecting network connectivity issues
//! from error messages. When a network error is detected, the app can show
//! an "Offline" status and automatically retry when connectivity is restored.

/// Patterns that indicate network connectivity issues
const NETWORK_ERROR_PATTERNS: &[&str] = &[
    "enotfound",
    "econnrefused",
    "econnreset",
    "etimedout",
    "ehostunreach",
    "enetunreach",
    "network",
    "offline",
    "internet",
    "dns",
    "socket hang up",
    "connection refused",
    "connection reset",
    "connect etimedout",
    "websocket",
    "socket.filen.io",
    "err_unhandled_error",
    "fetch failed",
    "getaddrinfo",
];

/// Check if an error message indicates a network connectivity issue.
///
/// This function performs case-insensitive pattern matching against
/// common network error messages from Node.js and the Filen CLI.
///
/// # Examples
///
/// ```ignore
/// use filen_menubar_lib::cli::network::is_network_error;
///
/// assert!(is_network_error("getaddrinfo ENOTFOUND api.filen.io"));
/// assert!(is_network_error("connect ECONNREFUSED 127.0.0.1:443"));
/// assert!(!is_network_error("File not found"));
/// ```
pub fn is_network_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    NETWORK_ERROR_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_network_error_enotfound() {
        assert!(is_network_error("getaddrinfo ENOTFOUND api.filen.io"));
        assert!(is_network_error("Error: ENOTFOUND"));
    }

    #[test]
    fn test_is_network_error_connection_errors() {
        assert!(is_network_error("connect ECONNREFUSED 127.0.0.1:443"));
        assert!(is_network_error("read ECONNRESET"));
        assert!(is_network_error("connect ETIMEDOUT 1.2.3.4:443"));
    }

    #[test]
    fn test_is_network_error_host_unreachable() {
        assert!(is_network_error("connect EHOSTUNREACH"));
        assert!(is_network_error("connect ENETUNREACH"));
    }

    #[test]
    fn test_is_network_error_text_patterns() {
        assert!(is_network_error("Network error occurred"));
        assert!(is_network_error("Device is offline"));
        assert!(is_network_error("No internet connection"));
        assert!(is_network_error("DNS lookup failed"));
        assert!(is_network_error("socket hang up"));
        assert!(is_network_error("connection refused"));
        assert!(is_network_error("connection reset by peer"));
    }

    #[test]
    fn test_is_network_error_case_insensitive() {
        assert!(is_network_error("NETWORK ERROR"));
        assert!(is_network_error("Offline Mode"));
        assert!(is_network_error("NO INTERNET"));
    }

    #[test]
    fn test_is_network_error_false_positives() {
        // These should NOT be detected as network errors
        assert!(!is_network_error("File not found"));
        assert!(!is_network_error("Permission denied"));
        assert!(!is_network_error("Invalid JSON"));
        assert!(!is_network_error("Authentication failed"));
    }

    #[test]
    fn test_is_network_error_filen_specific() {
        assert!(is_network_error("socket.filen.io connection failed"));
        assert!(is_network_error("err_unhandled_error: network issue"));
        assert!(is_network_error("fetch failed"));
    }
}
