//! Integration tests for CLI event parsing
//!
//! These tests verify that JSON events from the Filen CLI
//! are correctly parsed into CliEvent variants.

use filen_menubar_lib::cli::{CliErrorEvent, CliEvent};

#[test]
fn test_parse_real_cli_cycle_events() {
    // These are actual events captured from the Filen CLI
    let events = vec![
        r#"{"type":"cycleStarted"}"#,
        r#"{"type":"cycleGettingTreesStarted"}"#,
        r#"{"type":"cycleGettingTreesDone"}"#,
        r#"{"type":"cycleProcessingTasksStarted"}"#,
        r#"{"type":"deltasCount","data":{"count":0}}"#,
        r#"{"type":"cycleSuccess"}"#,
    ];

    for event_str in events {
        let result: Result<CliEvent, _> = serde_json::from_str(event_str);
        assert!(result.is_ok(), "Failed to parse event: {}", event_str);
    }
}

#[test]
fn test_parse_transfer_events() {
    // Matches actual Filen CLI output format:
    // - "of" field indicates upload/download
    // - "type" field indicates the status (started/progress/finished/success)
    let upload_event = r#"{
        "type": "transfer",
        "data": {
            "of": "upload",
            "type": "started",
            "relativePath": "Documents/test.pdf",
            "size": 1048576
        }
    }"#;

    let result: Result<CliEvent, _> = serde_json::from_str(upload_event);
    assert!(result.is_ok());

    match result.unwrap() {
        CliEvent::Transfer { data } => {
            assert!(data.is_some());
            let data = data.unwrap();
            // operation is "upload" or "download" (from "of" field)
            assert_eq!(data.operation.as_deref(), Some("upload"));
            // transfer_type is the status (from inner "type" field)
            assert_eq!(data.transfer_type.as_deref(), Some("started"));
            assert_eq!(data.size, Some(1048576));
        }
        _ => panic!("Expected Transfer event"),
    }
}

#[test]
fn test_parse_error_event() {
    let error_event = r#"{
        "type": "error",
        "error": "Connection refused"
    }"#;

    let result: Result<CliErrorEvent, _> = serde_json::from_str(error_event);
    assert!(result.is_ok());

    let event = result.unwrap();
    assert_eq!(event.event_type.as_deref(), Some("error"));
    assert_eq!(event.error.as_deref(), Some("Connection refused"));
}

#[test]
fn test_parse_deltas_with_count() {
    let deltas_event = r#"{"type":"deltasCount","data":{"count":15}}"#;

    let result: Result<CliEvent, _> = serde_json::from_str(deltas_event);
    assert!(result.is_ok());

    match result.unwrap() {
        CliEvent::DeltasCount { data } => {
            assert_eq!(data.count, 15);
        }
        _ => panic!("Expected DeltasCount event"),
    }
}

#[test]
fn test_network_error_detection() {
    use filen_menubar_lib::cli::network::is_network_error;

    // Common network errors from Filen CLI
    let network_errors = vec![
        "getaddrinfo ENOTFOUND api.filen.io",
        "connect ECONNREFUSED 127.0.0.1:443",
        "connect ETIMEDOUT 1.2.3.4:443",
        "socket.filen.io connection failed",
        "fetch failed",
    ];

    for error in network_errors {
        assert!(
            is_network_error(error),
            "Should detect as network error: {}",
            error
        );
    }

    // Non-network errors
    let other_errors = vec!["File not found", "Permission denied", "Invalid credentials"];

    for error in other_errors {
        assert!(
            !is_network_error(error),
            "Should NOT detect as network error: {}",
            error
        );
    }
}

#[test]
fn test_parse_multiline_json() {
    // CLI sometimes outputs pretty-printed JSON
    let multiline_event = r#"{
        "type": "cycleSuccess"
    }"#;

    let result: Result<CliEvent, _> = serde_json::from_str(multiline_event);
    assert!(result.is_ok());

    match result.unwrap() {
        CliEvent::CycleSuccess => {}
        _ => panic!("Expected CycleSuccess event"),
    }
}
