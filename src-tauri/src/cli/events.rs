//! CLI event types and parsing
//!
//! This module defines the event types emitted by the Filen CLI in `--verbose` mode.
//! Events are JSON-formatted and include sync cycle events, transfer progress,
//! and error notifications.

use serde::Deserialize;

/// Nested data for deltasCount event
#[derive(Debug, Deserialize)]
pub struct DeltasCountData {
    pub count: u32,
}

/// Nested data for transfer event
#[derive(Debug, Deserialize)]
pub struct TransferData {
    /// The operation type: "upload", "download", "createRemoteDirectory", etc.
    #[serde(rename = "of")]
    pub operation: Option<String>,
    /// The status: "queued", "started", "progress", "finished", "success", "error"
    #[serde(rename = "type")]
    pub transfer_type: Option<String>,
    /// The relative path of the file
    #[serde(rename = "relativePath")]
    pub relative_path: Option<String>,
    /// Bytes transferred so far (for progress events)
    pub bytes: Option<u64>,
    /// Total file size in bytes
    pub size: Option<u64>,
}

/// CLI event types emitted in --verbose mode
///
/// These events are parsed from the JSON output of `filen --verbose sync`.
/// The CLI emits various events during sync cycles to indicate progress.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum CliEvent {
    /// A sync cycle has started
    #[serde(rename = "cycleStarted")]
    CycleStarted,

    /// Started scanning local and remote file trees
    #[serde(rename = "cycleGettingTreesStarted")]
    CycleGettingTreesStarted,

    /// Finished scanning file trees
    #[serde(rename = "cycleGettingTreesDone")]
    CycleGettingTreesDone,

    /// Started processing sync tasks (uploads/downloads)
    #[serde(rename = "cycleProcessingTasksStarted")]
    CycleProcessingTasksStarted,

    /// Sync cycle completed successfully
    #[serde(rename = "cycleSuccess")]
    CycleSuccess,

    /// Sync cycle failed with an error
    #[serde(rename = "cycleError")]
    CycleError {
        #[allow(dead_code)]
        error: Option<String>,
    },

    /// Number of deltas (changes) to sync
    #[serde(rename = "deltasCount")]
    DeltasCount { data: DeltasCountData },

    /// File transfer event (upload/download progress)
    #[serde(rename = "transfer")]
    Transfer { data: Option<TransferData> },

    /// File operation completed successfully
    #[serde(rename = "success")]
    Success {
        #[allow(dead_code)]
        path: Option<String>,
    },

    /// Upload progress (legacy event format)
    #[serde(rename = "uploadProgress")]
    UploadProgress {
        #[allow(dead_code)]
        path: Option<String>,
        #[allow(dead_code)]
        progress: Option<f32>,
    },

    /// Download progress (legacy event format)
    #[serde(rename = "downloadProgress")]
    DownloadProgress {
        #[allow(dead_code)]
        path: Option<String>,
        #[allow(dead_code)]
        progress: Option<f32>,
    },

    /// Unknown event type (ignored)
    #[serde(other)]
    Unknown,
}

/// CLI error event structure for stderr parsing
#[derive(Debug, Deserialize)]
pub struct CliErrorEvent {
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    pub error: Option<String>,
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== CliEvent parsing tests ====================

    #[test]
    fn test_parse_cycle_started() {
        let json = r#"{"type":"cycleStarted"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, CliEvent::CycleStarted));
    }

    #[test]
    fn test_parse_cycle_success() {
        let json = r#"{"type":"cycleSuccess"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, CliEvent::CycleSuccess));
    }

    #[test]
    fn test_parse_cycle_processing_tasks_started() {
        let json = r#"{"type":"cycleProcessingTasksStarted"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, CliEvent::CycleProcessingTasksStarted));
    }

    #[test]
    fn test_parse_cycle_getting_trees_started() {
        let json = r#"{"type":"cycleGettingTreesStarted"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, CliEvent::CycleGettingTreesStarted));
    }

    #[test]
    fn test_parse_cycle_getting_trees_done() {
        let json = r#"{"type":"cycleGettingTreesDone"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, CliEvent::CycleGettingTreesDone));
    }

    #[test]
    fn test_parse_cycle_error_with_message() {
        let json = r#"{"type":"cycleError","error":"Something went wrong"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::CycleError { error } => {
                assert_eq!(error, Some("Something went wrong".to_string()));
            }
            _ => panic!("Expected CycleError"),
        }
    }

    #[test]
    fn test_parse_cycle_error_without_message() {
        let json = r#"{"type":"cycleError"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::CycleError { error } => {
                assert!(error.is_none());
            }
            _ => panic!("Expected CycleError"),
        }
    }

    #[test]
    fn test_parse_deltas_count() {
        let json = r#"{"type":"deltasCount","data":{"count":5}}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::DeltasCount { data } => {
                assert_eq!(data.count, 5);
            }
            _ => panic!("Expected DeltasCount"),
        }
    }

    #[test]
    fn test_parse_deltas_count_zero() {
        let json = r#"{"type":"deltasCount","data":{"count":0}}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::DeltasCount { data } => {
                assert_eq!(data.count, 0);
            }
            _ => panic!("Expected DeltasCount"),
        }
    }

    #[test]
    fn test_parse_transfer_upload_progress() {
        let json = r#"{
            "type": "transfer",
            "data": {
                "of": "upload",
                "type": "progress",
                "relativePath": "documents/report.pdf",
                "bytes": 512,
                "size": 1024
            }
        }"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                let data = data.expect("Expected transfer data");
                assert_eq!(data.operation, Some("upload".to_string()));
                assert_eq!(data.transfer_type, Some("progress".to_string()));
                assert_eq!(data.relative_path, Some("documents/report.pdf".to_string()));
                assert_eq!(data.bytes, Some(512));
                assert_eq!(data.size, Some(1024));
            }
            _ => panic!("Expected Transfer"),
        }
    }

    #[test]
    fn test_parse_transfer_download_started() {
        let json = r#"{
            "type": "transfer",
            "data": {
                "of": "download",
                "type": "started",
                "relativePath": "photos/image.jpg",
                "size": 2048
            }
        }"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                let data = data.expect("Expected transfer data");
                assert_eq!(data.operation, Some("download".to_string()));
                assert_eq!(data.transfer_type, Some("started".to_string()));
                assert_eq!(data.relative_path, Some("photos/image.jpg".to_string()));
                assert!(data.bytes.is_none());
                assert_eq!(data.size, Some(2048));
            }
            _ => panic!("Expected Transfer"),
        }
    }

    #[test]
    fn test_parse_transfer_success() {
        let json = r#"{
            "type": "transfer",
            "data": {
                "of": "upload",
                "type": "success",
                "relativePath": "file.txt"
            }
        }"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                let data = data.expect("Expected transfer data");
                assert_eq!(data.transfer_type, Some("success".to_string()));
            }
            _ => panic!("Expected Transfer"),
        }
    }

    #[test]
    fn test_parse_transfer_create_directory() {
        let json = r#"{
            "type": "transfer",
            "data": {
                "of": "createRemoteDirectory",
                "type": "success",
                "relativePath": "new_folder"
            }
        }"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                let data = data.expect("Expected transfer data");
                assert_eq!(data.operation, Some("createRemoteDirectory".to_string()));
            }
            _ => panic!("Expected Transfer"),
        }
    }

    #[test]
    fn test_parse_unknown_event() {
        let json = r#"{"type":"someNewEventType","data":{"foo":"bar"}}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, CliEvent::Unknown));
    }

    #[test]
    fn test_parse_success_event() {
        let json = r#"{"type":"success","path":"/some/path"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Success { path } => {
                assert_eq!(path, Some("/some/path".to_string()));
            }
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_parse_upload_progress() {
        let json = r#"{"type":"uploadProgress","path":"/file.txt","progress":0.75}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::UploadProgress { path, progress } => {
                assert_eq!(path, Some("/file.txt".to_string()));
                assert!((progress.unwrap() - 0.75).abs() < 0.001);
            }
            _ => panic!("Expected UploadProgress"),
        }
    }

    #[test]
    fn test_parse_download_progress() {
        let json = r#"{"type":"downloadProgress","path":"/file.txt","progress":0.5}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::DownloadProgress { path, progress } => {
                assert_eq!(path, Some("/file.txt".to_string()));
                assert!((progress.unwrap() - 0.5).abs() < 0.001);
            }
            _ => panic!("Expected DownloadProgress"),
        }
    }

    // ==================== CliErrorEvent parsing tests ====================

    #[test]
    fn test_parse_error_event_with_error() {
        let json = r#"{"type":"error","error":"Connection failed"}"#;
        let event: CliErrorEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, Some("error".to_string()));
        assert_eq!(event.error, Some("Connection failed".to_string()));
    }

    #[test]
    fn test_parse_error_event_with_message() {
        let json = r#"{"type":"error","message":"Authentication failed"}"#;
        let event: CliErrorEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, Some("error".to_string()));
        assert_eq!(event.message, Some("Authentication failed".to_string()));
    }

    // ==================== Multi-line JSON parsing simulation ====================

    #[test]
    fn test_parse_pretty_printed_json() {
        // Simulates how the CLI outputs multi-line JSON
        let pretty_json = r#"{
    "type": "deltasCount",
    "data": {
        "count": 42
    }
}"#;
        let event: CliEvent = serde_json::from_str(pretty_json).unwrap();
        match event {
            CliEvent::DeltasCount { data } => {
                assert_eq!(data.count, 42);
            }
            _ => panic!("Expected DeltasCount"),
        }
    }

    #[test]
    fn test_parse_nested_transfer_pretty_printed() {
        let pretty_json = r#"{
    "type": "transfer",
    "data": {
        "of": "upload",
        "type": "progress",
        "relativePath": "nested/path/to/file.txt",
        "bytes": 1000,
        "size": 5000
    }
}"#;
        let event: CliEvent = serde_json::from_str(pretty_json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                let data = data.expect("Expected transfer data");
                assert_eq!(data.operation, Some("upload".to_string()));
                assert_eq!(data.bytes, Some(1000));
                assert_eq!(data.size, Some(5000));
            }
            _ => panic!("Expected Transfer"),
        }
    }

    // ==================== Edge cases ====================

    #[test]
    fn test_transfer_with_missing_data() {
        let json = r#"{"type":"transfer"}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                assert!(data.is_none());
            }
            _ => panic!("Expected Transfer"),
        }
    }

    #[test]
    fn test_transfer_with_null_data() {
        let json = r#"{"type":"transfer","data":null}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                assert!(data.is_none());
            }
            _ => panic!("Expected Transfer"),
        }
    }

    #[test]
    fn test_transfer_with_partial_data() {
        let json = r#"{"type":"transfer","data":{"of":"upload"}}"#;
        let event: CliEvent = serde_json::from_str(json).unwrap();
        match event {
            CliEvent::Transfer { data } => {
                let data = data.expect("Expected transfer data");
                assert_eq!(data.operation, Some("upload".to_string()));
                assert!(data.transfer_type.is_none());
                assert!(data.relative_path.is_none());
                assert!(data.bytes.is_none());
                assert!(data.size.is_none());
            }
            _ => panic!("Expected Transfer"),
        }
    }
}
