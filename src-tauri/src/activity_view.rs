//! Commands and lifecycle support for the Recent Activity webview.

use crate::activity::{ActivityEntry, ActivityHistory, ActivityOperation, ActivityOutcome};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, State};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(750);

/// Shared command context for the Recent Activity webview.
#[derive(Clone)]
pub struct ActivityCommandContext {
    history: Arc<ActivityHistory>,
}

impl ActivityCommandContext {
    pub fn new(history: Arc<ActivityHistory>) -> Self {
        Self { history }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityCopy {
    locale: String,
    platform: String,
    window_title: String,
    title: String,
    intro: String,
    search_label: String,
    search_placeholder: String,
    filter_label: String,
    filter_all: String,
    filter_uploads: String,
    filter_downloads: String,
    filter_changes: String,
    filter_errors: String,
    clear_button: String,
    clear_confirm_prompt: String,
    clear_confirm_button: String,
    clear_cancel_button: String,
    loading: String,
    load_failed_title: String,
    load_failed_description: String,
    retry_button: String,
    empty_title: String,
    empty_description: String,
    no_results_title: String,
    no_results_description: String,
    list_label: String,
    category_upload: String,
    category_download: String,
    category_change: String,
    outcome_success: String,
    outcome_failed: String,
    count_one: String,
    count_many: String,
    size_b: String,
    size_kb: String,
    size_mb: String,
    size_gb: String,
    size_tb: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityViewEntry {
    id: u64,
    observed_at_ms: i64,
    category: &'static str,
    outcome: &'static str,
    action: String,
    relative_path: String,
    size: Option<u64>,
}

#[tauri::command]
pub fn activity_copy() -> ActivityCopy {
    ActivityCopy {
        locale: rust_i18n::locale().to_string(),
        platform: std::env::consts::OS.to_string(),
        window_title: rust_i18n::t!("activity.window_title").to_string(),
        title: rust_i18n::t!("activity.title").to_string(),
        intro: rust_i18n::t!("activity.intro").to_string(),
        search_label: rust_i18n::t!("activity.search_label").to_string(),
        search_placeholder: rust_i18n::t!("activity.search_placeholder").to_string(),
        filter_label: rust_i18n::t!("activity.filter_label").to_string(),
        filter_all: rust_i18n::t!("activity.filter_all").to_string(),
        filter_uploads: rust_i18n::t!("activity.filter_uploads").to_string(),
        filter_downloads: rust_i18n::t!("activity.filter_downloads").to_string(),
        filter_changes: rust_i18n::t!("activity.filter_changes").to_string(),
        filter_errors: rust_i18n::t!("activity.filter_errors").to_string(),
        clear_button: rust_i18n::t!("activity.clear_button").to_string(),
        clear_confirm_prompt: rust_i18n::t!("activity.clear_confirm_prompt").to_string(),
        clear_confirm_button: rust_i18n::t!("activity.clear_confirm_button").to_string(),
        clear_cancel_button: rust_i18n::t!("activity.clear_cancel_button").to_string(),
        loading: rust_i18n::t!("activity.loading").to_string(),
        load_failed_title: rust_i18n::t!("activity.load_failed_title").to_string(),
        load_failed_description: rust_i18n::t!("activity.load_failed_description").to_string(),
        retry_button: rust_i18n::t!("activity.retry_button").to_string(),
        empty_title: rust_i18n::t!("activity.empty_title").to_string(),
        empty_description: rust_i18n::t!("activity.empty_description").to_string(),
        no_results_title: rust_i18n::t!("activity.no_results_title").to_string(),
        no_results_description: rust_i18n::t!("activity.no_results_description").to_string(),
        list_label: rust_i18n::t!("activity.list_label").to_string(),
        category_upload: rust_i18n::t!("activity.category_upload").to_string(),
        category_download: rust_i18n::t!("activity.category_download").to_string(),
        category_change: rust_i18n::t!("activity.category_change").to_string(),
        outcome_success: rust_i18n::t!("activity.outcome_success").to_string(),
        outcome_failed: rust_i18n::t!("activity.outcome_failed").to_string(),
        count_one: rust_i18n::t!("activity.count_one", count = "%{count}").to_string(),
        count_many: rust_i18n::t!("activity.count_many", count = "%{count}").to_string(),
        size_b: rust_i18n::t!("activity.size_b").to_string(),
        size_kb: rust_i18n::t!("activity.size_kb").to_string(),
        size_mb: rust_i18n::t!("activity.size_mb").to_string(),
        size_gb: rust_i18n::t!("activity.size_gb").to_string(),
        size_tb: rust_i18n::t!("activity.size_tb").to_string(),
    }
}

#[tauri::command]
pub fn recent_activity(context: State<'_, ActivityCommandContext>) -> Vec<ActivityViewEntry> {
    context
        .history
        .snapshot_newest_first()
        .into_iter()
        .map(ActivityViewEntry::from)
        .collect()
}

#[tauri::command]
pub async fn clear_activity(context: State<'_, ActivityCommandContext>) -> Result<(), String> {
    let history = context.history.clone();
    history.clear_and_flush().await.map_err(|error| {
        log::warn!("Failed to persist cleared activity history: {error}");
        "Could not persist the cleared activity history".to_string()
    })
}

impl From<ActivityEntry> for ActivityViewEntry {
    fn from(entry: ActivityEntry) -> Self {
        let category = category_for(&entry.operation);
        let outcome = match &entry.outcome {
            ActivityOutcome::Success => "success",
            ActivityOutcome::Failed => "failed",
        };
        let action = operation_label(&entry.operation, &entry.outcome);

        Self {
            id: entry.id,
            observed_at_ms: entry.observed_at_ms,
            category,
            outcome,
            action,
            relative_path: entry.relative_path,
            size: entry.size,
        }
    }
}

fn category_for(operation: &ActivityOperation) -> &'static str {
    match operation {
        ActivityOperation::UploadFile => "upload",
        ActivityOperation::DownloadFile => "download",
        ActivityOperation::CreateLocalDirectory
        | ActivityOperation::CreateRemoteDirectory
        | ActivityOperation::DeleteLocalFile
        | ActivityOperation::DeleteLocalDirectory
        | ActivityOperation::DeleteRemoteFile
        | ActivityOperation::DeleteRemoteDirectory
        | ActivityOperation::RenameLocalFile
        | ActivityOperation::RenameLocalDirectory
        | ActivityOperation::RenameRemoteFile
        | ActivityOperation::RenameRemoteDirectory => "change",
    }
}

fn operation_label(operation: &ActivityOperation, outcome: &ActivityOutcome) -> String {
    match (operation, outcome) {
        (ActivityOperation::UploadFile, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.upload_file").to_string()
        }
        (ActivityOperation::UploadFile, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.upload_file_failed").to_string()
        }
        (ActivityOperation::DownloadFile, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.download_file").to_string()
        }
        (ActivityOperation::DownloadFile, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.download_file_failed").to_string()
        }
        (ActivityOperation::CreateRemoteDirectory, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.create_remote_directory").to_string()
        }
        (ActivityOperation::CreateRemoteDirectory, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.create_remote_directory_failed").to_string()
        }
        (ActivityOperation::CreateLocalDirectory, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.create_local_directory").to_string()
        }
        (ActivityOperation::CreateLocalDirectory, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.create_local_directory_failed").to_string()
        }
        (ActivityOperation::DeleteRemoteFile, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.delete_remote_file").to_string()
        }
        (ActivityOperation::DeleteRemoteFile, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.delete_remote_file_failed").to_string()
        }
        (ActivityOperation::DeleteRemoteDirectory, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.delete_remote_directory").to_string()
        }
        (ActivityOperation::DeleteRemoteDirectory, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.delete_remote_directory_failed").to_string()
        }
        (ActivityOperation::DeleteLocalFile, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.delete_local_file").to_string()
        }
        (ActivityOperation::DeleteLocalFile, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.delete_local_file_failed").to_string()
        }
        (ActivityOperation::DeleteLocalDirectory, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.delete_local_directory").to_string()
        }
        (ActivityOperation::DeleteLocalDirectory, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.delete_local_directory_failed").to_string()
        }
        (ActivityOperation::RenameRemoteFile, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.rename_remote_file").to_string()
        }
        (ActivityOperation::RenameRemoteFile, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.rename_remote_file_failed").to_string()
        }
        (ActivityOperation::RenameRemoteDirectory, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.rename_remote_directory").to_string()
        }
        (ActivityOperation::RenameRemoteDirectory, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.rename_remote_directory_failed").to_string()
        }
        (ActivityOperation::RenameLocalFile, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.rename_local_file").to_string()
        }
        (ActivityOperation::RenameLocalFile, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.rename_local_file_failed").to_string()
        }
        (ActivityOperation::RenameLocalDirectory, ActivityOutcome::Success) => {
            rust_i18n::t!("activity.operation.rename_local_directory").to_string()
        }
        (ActivityOperation::RenameLocalDirectory, ActivityOutcome::Failed) => {
            rust_i18n::t!("activity.operation.rename_local_directory_failed").to_string()
        }
    }
}

/// Forward history mutations to the activity window when it exists.
pub async fn forward_activity_updates(
    mut updates: tokio::sync::watch::Receiver<u64>,
    app_handle: tauri::AppHandle,
) {
    while updates.changed().await.is_ok() {
        if app_handle.get_webview_window("activity").is_some() {
            if let Err(error) = app_handle.emit_to("activity", "activity-updated", ()) {
                log::debug!("Failed to notify Recent Activity window: {error}");
            }
        }
    }
}

/// Persist bursts of activity without rewriting the snapshot for every file.
pub async fn persist_activity_updates(
    history: Arc<ActivityHistory>,
    mut updates: tokio::sync::watch::Receiver<u64>,
) {
    while updates.changed().await.is_ok() {
        tokio::time::sleep(SAVE_DEBOUNCE).await;

        while updates.has_changed().unwrap_or(false) {
            let _ = updates.borrow_and_update();
        }

        if let Err(error) = history.flush().await {
            // Activity history is observational. A storage failure must never
            // alter the sync state or stop the Filen subprocess.
            log::warn!("Failed to persist recent activity: {error}");
        }
    }
}

/// Keep the reusable activity window alive when its title-bar close button is used.
pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "activity" {
        return;
    }

    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        if let Err(error) = window.hide() {
            log::error!("Failed to hide Recent Activity window: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorizes_copy_operations_and_other_changes() {
        assert_eq!(category_for(&ActivityOperation::UploadFile), "upload");
        assert_eq!(category_for(&ActivityOperation::DownloadFile), "download");
        assert_eq!(category_for(&ActivityOperation::DeleteRemoteFile), "change");
    }

    #[test]
    fn produces_distinct_success_and_failure_labels() {
        rust_i18n::set_locale("en");
        let success = operation_label(&ActivityOperation::UploadFile, &ActivityOutcome::Success);
        let failed = operation_label(&ActivityOperation::UploadFile, &ActivityOutcome::Failed);

        assert_eq!(success, "Uploaded to cloud");
        assert_eq!(failed, "Upload to cloud failed");
    }
}
