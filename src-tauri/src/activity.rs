//! Bounded, persistent history of completed file operations.
//!
//! Activity recording is deliberately independent from [`crate::state::AppState`]:
//! tray state is short-lived and highly reactive, while activity history is a
//! bounded user-facing record that can survive application restarts.

use crate::cli::TransferData;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{watch, Mutex as AsyncMutex};

/// Maximum number of completed activities retained on disk and in memory.
pub const MAX_ACTIVITY_ENTRIES: usize = 500;

const ACTIVITY_SNAPSHOT_VERSION: u32 = 1;
const FIRST_ACTIVITY_ID: u64 = 1;

/// A canonical task-level operation emitted by the Filen CLI.
///
/// Raw transfer-progress operations such as `upload` and `download` are not
/// represented here. The CLI emits task-level events for completed operations;
/// keeping only those avoids recording the same transfer twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityOperation {
    UploadFile,
    DownloadFile,
    CreateLocalDirectory,
    CreateRemoteDirectory,
    DeleteLocalFile,
    DeleteLocalDirectory,
    DeleteRemoteFile,
    DeleteRemoteDirectory,
    RenameLocalFile,
    RenameLocalDirectory,
    RenameRemoteFile,
    RenameRemoteDirectory,
}

impl ActivityOperation {
    fn from_cli_operation(operation: &str) -> Option<Self> {
        match operation {
            "uploadFile" => Some(Self::UploadFile),
            "downloadFile" => Some(Self::DownloadFile),
            "createLocalDirectory" => Some(Self::CreateLocalDirectory),
            "createRemoteDirectory" => Some(Self::CreateRemoteDirectory),
            "deleteLocalFile" => Some(Self::DeleteLocalFile),
            "deleteLocalDirectory" => Some(Self::DeleteLocalDirectory),
            "deleteRemoteFile" => Some(Self::DeleteRemoteFile),
            "deleteRemoteDirectory" => Some(Self::DeleteRemoteDirectory),
            "renameLocalFile" => Some(Self::RenameLocalFile),
            "renameLocalDirectory" => Some(Self::RenameLocalDirectory),
            "renameRemoteFile" => Some(Self::RenameRemoteFile),
            "renameRemoteDirectory" => Some(Self::RenameRemoteDirectory),
            _ => None,
        }
    }
}

/// Terminal outcome reported for a canonical file operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityOutcome {
    Success,
    Failed,
}

impl ActivityOutcome {
    fn from_transfer_type(transfer_type: &str) -> Option<Self> {
        match transfer_type {
            "success" => Some(Self::Success),
            "error" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// One completed task-level operation as observed from the CLI event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// Monotonically increasing identifier local to this history file.
    pub id: u64,
    /// UTC receipt time expressed as Unix epoch milliseconds.
    pub observed_at_ms: i64,
    pub operation: ActivityOperation,
    pub outcome: ActivityOutcome,
    /// Path relative to the configured sync root, exactly as reported by the CLI.
    pub relative_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ActivityError {
    #[error("activity history I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("activity history JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error(
        "unsupported activity history version {found}; this build supports version {supported}"
    )]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("activity history persistence task failed: {0}")]
    PersistenceTask(#[from] tokio::task::JoinError),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedActivitySnapshot {
    version: u32,
    next_id: u64,
    /// Stored oldest first so appending remains natural and deterministic.
    entries: Vec<ActivityEntry>,
}

#[derive(Debug)]
struct ActivityState {
    /// Oldest entry at the front, newest at the back.
    entries: VecDeque<ActivityEntry>,
    next_id: u64,
    revision: u64,
}

impl ActivityState {
    fn empty() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_ACTIVITY_ENTRIES),
            next_id: FIRST_ACTIVITY_ID,
            revision: 0,
        }
    }

    fn from_persisted(snapshot: PersistedActivitySnapshot) -> Self {
        let retained_start = snapshot.entries.len().saturating_sub(MAX_ACTIVITY_ENTRIES);
        let entries: VecDeque<_> = snapshot.entries.into_iter().skip(retained_start).collect();

        let next_after_entries = entries
            .iter()
            .map(|entry| next_id_after(entry.id))
            .max()
            .unwrap_or(FIRST_ACTIVITY_ID);

        Self {
            entries,
            next_id: snapshot
                .next_id
                .max(next_after_entries)
                .max(FIRST_ACTIVITY_ID),
            revision: 0,
        }
    }

    fn persisted_snapshot(&self) -> PersistedActivitySnapshot {
        PersistedActivitySnapshot {
            version: ACTIVITY_SNAPSHOT_VERSION,
            next_id: self.next_id,
            entries: self.entries.iter().cloned().collect(),
        }
    }

    fn bump_revision(&mut self) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        self.revision
    }
}

/// Thread-safe activity history with explicitly triggered persistence.
///
/// Recording and snapshots are synchronous and only hold an in-memory lock.
/// [`flush`](Self::flush) performs serialization and filesystem work away from
/// the async runtime, with concurrent flushes serialized so an older snapshot
/// can never replace a newer one.
#[derive(Clone)]
pub struct ActivityHistory {
    path: Option<PathBuf>,
    state: Arc<RwLock<ActivityState>>,
    revision_tx: watch::Sender<u64>,
    flush_lock: Arc<AsyncMutex<()>>,
}

impl ActivityHistory {
    /// Create a non-persistent history, primarily useful for tests and fallback.
    pub fn in_memory() -> Self {
        Self::from_state(None, ActivityState::empty())
    }

    /// Load history from `path`, falling back to an empty history on any error.
    ///
    /// A corrupt or future-version snapshot must never prevent synchronization
    /// from starting. The error is logged and a later successful flush atomically
    /// replaces the unusable file.
    pub fn load_or_empty(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let state = match load_persisted_state(&path) {
            Ok(Some(state)) => state,
            Ok(None) => ActivityState::empty(),
            Err(error) => {
                log::warn!(
                    "Failed to load activity history from {}: {}. Starting with empty history.",
                    path.display(),
                    error
                );
                ActivityState::empty()
            }
        };

        if path.exists() {
            if let Err(error) = set_owner_only_permissions(&path) {
                log::warn!(
                    "Failed to restrict activity history permissions for {}: {}",
                    path.display(),
                    error
                );
            }
        }

        Self::from_state(Some(path), state)
    }

    fn from_state(path: Option<PathBuf>, state: ActivityState) -> Self {
        let initial_revision = state.revision;
        let (revision_tx, _) = watch::channel(initial_revision);
        Self {
            path,
            state: Arc::new(RwLock::new(state)),
            revision_tx,
            flush_lock: Arc::new(AsyncMutex::new(())),
        }
    }

    /// Record a canonical terminal event with the current UTC receipt time.
    ///
    /// Returns the inserted entry, or `None` for progress events, raw
    /// upload/download events, unknown operations, and incomplete payloads.
    pub fn record_transfer(&self, transfer: &TransferData) -> Option<ActivityEntry> {
        self.record_transfer_at(transfer, utc_now_ms())
    }

    /// Deterministic variant of [`record_transfer`](Self::record_transfer).
    pub fn record_transfer_at(
        &self,
        transfer: &TransferData,
        observed_at_ms: i64,
    ) -> Option<ActivityEntry> {
        let operation = ActivityOperation::from_cli_operation(transfer.operation.as_deref()?)?;
        let outcome = ActivityOutcome::from_transfer_type(transfer.transfer_type.as_deref()?)?;
        let relative_path = transfer.relative_path.clone()?;

        let (entry, revision) = {
            let mut state = self.write_state();
            let id = state.next_id;
            state.next_id = next_id_after(id);

            let entry = ActivityEntry {
                id,
                observed_at_ms,
                operation,
                outcome,
                relative_path,
                size: transfer.size,
            };

            if state.entries.len() == MAX_ACTIVITY_ENTRIES {
                state.entries.pop_front();
            }
            state.entries.push_back(entry.clone());
            let revision = state.bump_revision();
            (entry, revision)
        };

        self.revision_tx.send_replace(revision);
        Some(entry)
    }

    /// Return a point-in-time snapshot ordered newest first for display.
    pub fn snapshot_newest_first(&self) -> Vec<ActivityEntry> {
        self.read_state().entries.iter().rev().cloned().collect()
    }

    /// Subscribe to content revisions for a debounced persistence or UI task.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision_tx.subscribe()
    }

    /// Remove all retained entries in memory.
    ///
    /// Call [`flush`](Self::flush), or use [`clear_and_flush`](Self::clear_and_flush),
    /// to persist the cleared state.
    pub fn clear(&self) {
        let revision = {
            let mut state = self.write_state();
            state.entries.clear();
            state.bump_revision()
        };
        self.revision_tx.send_replace(revision);
    }

    /// Clear the history and atomically persist the empty snapshot.
    ///
    /// Persistent histories are only mutated in memory after the replacement
    /// succeeds. Holding the write lock during the filesystem operation also
    /// prevents a concurrently recorded event from being removed by the clear.
    pub async fn clear_and_flush(&self) -> Result<(), ActivityError> {
        let Some(path) = self.path.clone() else {
            self.clear();
            return Ok(());
        };

        let _flush_guard = self.flush_lock.lock().await;
        let state = Arc::clone(&self.state);
        let revision = tokio::task::spawn_blocking(move || {
            let mut state = state
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let snapshot = PersistedActivitySnapshot {
                version: ACTIVITY_SNAPSHOT_VERSION,
                next_id: state.next_id,
                entries: Vec::new(),
            };
            let contents = serde_json::to_vec_pretty(&snapshot)?;

            atomic_replace(&path, &contents)?;
            state.entries.clear();
            Ok::<_, ActivityError>(state.bump_revision())
        })
        .await??;

        self.revision_tx.send_replace(revision);
        Ok(())
    }

    /// Persist the latest snapshot using an atomic same-directory replacement.
    ///
    /// Filesystem work runs in `spawn_blocking`. A shared async mutex serializes
    /// concurrent calls, and the loop catches revisions recorded during a write
    /// before returning.
    pub async fn flush(&self) -> Result<(), ActivityError> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };

        let _flush_guard = self.flush_lock.lock().await;

        loop {
            let (snapshot, revision) = {
                let state = self.read_state();
                (state.persisted_snapshot(), state.revision)
            };
            let contents = serde_json::to_vec_pretty(&snapshot)?;
            let write_path = path.clone();

            tokio::task::spawn_blocking(move || atomic_replace(&write_path, &contents)).await??;

            if self.read_state().revision == revision {
                return Ok(());
            }
        }
    }

    pub fn len(&self) -> usize {
        self.read_state().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn read_state(&self) -> RwLockReadGuard<'_, ActivityState> {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_state(&self) -> RwLockWriteGuard<'_, ActivityState> {
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for ActivityHistory {
    fn default() -> Self {
        Self::in_memory()
    }
}

/// Platform data-directory location for the persistent activity snapshot.
pub fn default_activity_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|directory| {
        directory
            .join("filen-menubar")
            .join("activity-history.json")
    })
}

fn next_id_after(id: u64) -> u64 {
    if id == u64::MAX {
        FIRST_ACTIVITY_ID
    } else {
        id + 1
    }
}

fn utc_now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

fn load_persisted_state(path: &Path) -> Result<Option<ActivityState>, ActivityError> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    let snapshot: PersistedActivitySnapshot = serde_json::from_slice(&contents)?;
    if snapshot.version != ACTIVITY_SNAPSHOT_VERSION {
        return Err(ActivityError::UnsupportedVersion {
            found: snapshot.version,
            supported: ACTIVITY_SNAPSHOT_VERSION,
        });
    }

    Ok(Some(ActivityState::from_persisted(snapshot)))
}

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn atomic_replace(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("activity-history.json");
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = parent.join(format!(
        ".{filename}.{}.{}.tmp",
        std::process::id(),
        counter
    ));

    let write_result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options.open(&temp_path)?;
        file.write_all(contents)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);

        fs::rename(&temp_path, path)?;
        set_owner_only_permissions(path)?;

        // Best-effort directory sync makes the rename durable where supported.
        #[cfg(unix)]
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }

        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(
        operation: Option<&str>,
        transfer_type: Option<&str>,
        relative_path: Option<&str>,
        size: Option<u64>,
    ) -> TransferData {
        TransferData {
            operation: operation.map(str::to_string),
            transfer_type: transfer_type.map(str::to_string),
            relative_path: relative_path.map(str::to_string),
            bytes: None,
            size,
        }
    }

    #[test]
    fn maps_every_canonical_operation_and_terminal_outcome() {
        let mappings = [
            ("uploadFile", ActivityOperation::UploadFile),
            ("downloadFile", ActivityOperation::DownloadFile),
            (
                "createLocalDirectory",
                ActivityOperation::CreateLocalDirectory,
            ),
            (
                "createRemoteDirectory",
                ActivityOperation::CreateRemoteDirectory,
            ),
            ("deleteLocalFile", ActivityOperation::DeleteLocalFile),
            (
                "deleteLocalDirectory",
                ActivityOperation::DeleteLocalDirectory,
            ),
            ("deleteRemoteFile", ActivityOperation::DeleteRemoteFile),
            (
                "deleteRemoteDirectory",
                ActivityOperation::DeleteRemoteDirectory,
            ),
            ("renameLocalFile", ActivityOperation::RenameLocalFile),
            (
                "renameLocalDirectory",
                ActivityOperation::RenameLocalDirectory,
            ),
            ("renameRemoteFile", ActivityOperation::RenameRemoteFile),
            (
                "renameRemoteDirectory",
                ActivityOperation::RenameRemoteDirectory,
            ),
        ];
        let outcomes = [
            ("success", ActivityOutcome::Success),
            ("error", ActivityOutcome::Failed),
        ];

        for (operation_name, expected_operation) in mappings {
            for (transfer_type, expected_outcome) in outcomes {
                let history = ActivityHistory::in_memory();
                let event = transfer(
                    Some(operation_name),
                    Some(transfer_type),
                    Some("nested/file.txt"),
                    Some(42),
                );
                let entry = history
                    .record_transfer_at(&event, 1_700_000_000_123)
                    .expect("canonical terminal event should be recorded");

                assert_eq!(entry.id, FIRST_ACTIVITY_ID);
                assert_eq!(entry.observed_at_ms, 1_700_000_000_123);
                assert_eq!(entry.operation, expected_operation);
                assert_eq!(entry.outcome, expected_outcome);
                assert_eq!(entry.relative_path, "nested/file.txt");
                assert_eq!(entry.size, Some(42));
            }
        }
    }

    #[test]
    fn ignores_nonterminal_raw_unknown_and_incomplete_events() {
        let ignored = [
            transfer(Some("upload"), Some("finished"), Some("raw.txt"), None),
            transfer(Some("download"), Some("success"), Some("raw.txt"), None),
            transfer(Some("uploadFile"), Some("finished"), Some("file.txt"), None),
            transfer(Some("uploadFile"), Some("progress"), Some("file.txt"), None),
            transfer(
                Some("futureOperation"),
                Some("success"),
                Some("file.txt"),
                None,
            ),
            transfer(None, Some("success"), Some("file.txt"), None),
            transfer(Some("uploadFile"), None, Some("file.txt"), None),
            transfer(Some("uploadFile"), Some("success"), None, None),
        ];
        let history = ActivityHistory::in_memory();

        for event in ignored {
            assert!(history.record_transfer(&event).is_none());
        }
        assert!(history.is_empty());
    }

    #[test]
    fn repeated_canonical_terminal_events_are_retained_as_distinct_actions() {
        let history = ActivityHistory::in_memory();
        let event = transfer(Some("uploadFile"), Some("success"), Some("same.txt"), None);

        let first = history.record_transfer_at(&event, 100).unwrap();
        let second = history.record_transfer_at(&event, 101).unwrap();

        assert_ne!(first.id, second.id);
        assert_eq!(history.len(), 2);
        assert_eq!(
            history
                .snapshot_newest_first()
                .iter()
                .map(|entry| entry.observed_at_ms)
                .collect::<Vec<_>>(),
            vec![101, 100]
        );
    }

    #[test]
    fn retains_only_newest_five_hundred_in_newest_first_order() {
        let history = ActivityHistory::in_memory();

        for index in 0..505 {
            let path = format!("file-{index}.txt");
            let event = transfer(Some("uploadFile"), Some("success"), Some(&path), None);
            history.record_transfer_at(&event, index).unwrap();
        }

        let entries = history.snapshot_newest_first();
        assert_eq!(entries.len(), MAX_ACTIVITY_ENTRIES);
        assert_eq!(entries.first().unwrap().relative_path, "file-504.txt");
        assert_eq!(entries.first().unwrap().id, 505);
        assert_eq!(entries.last().unwrap().relative_path, "file-5.txt");
        assert_eq!(entries.last().unwrap().id, 6);
    }

    #[test]
    fn preserves_unicode_relative_paths_and_optional_size() {
        let history = ActivityHistory::in_memory();
        let event = transfer(
            Some("downloadFile"),
            Some("success"),
            Some("Fotos/Frühling/東京 🗼.jpg"),
            None,
        );

        let entry = history.record_transfer_at(&event, 7).unwrap();
        assert_eq!(entry.relative_path, "Fotos/Frühling/東京 🗼.jpg");
        assert_eq!(entry.size, None);

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("observedAtMs"));
        assert!(json.contains("relativePath"));
        assert!(json.contains("downloadFile"));
        assert!(!json.contains("\"size\""));
    }

    #[tokio::test]
    async fn clear_updates_snapshot_and_notifies_subscribers() {
        let history = ActivityHistory::in_memory();
        let mut revision_rx = history.subscribe();
        let event = transfer(
            Some("deleteRemoteFile"),
            Some("success"),
            Some("old.txt"),
            None,
        );

        history.record_transfer_at(&event, 8).unwrap();
        revision_rx.changed().await.unwrap();
        let after_record = *revision_rx.borrow_and_update();

        history.clear();
        revision_rx.changed().await.unwrap();
        let after_clear = *revision_rx.borrow_and_update();

        assert!(after_clear > after_record);
        assert!(history.snapshot_newest_first().is_empty());
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn persistent_round_trip_preserves_entries_and_next_id() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("data").join("activity.json");
        let history = ActivityHistory::load_or_empty(path.clone());
        let first = transfer(
            Some("createRemoteDirectory"),
            Some("success"),
            Some("Dokumente"),
            None,
        );
        let second = transfer(
            Some("uploadFile"),
            Some("error"),
            Some("Dokumente/fehlgeschlagen.txt"),
            Some(123),
        );
        history.record_transfer_at(&first, 10).unwrap();
        history.record_transfer_at(&second, 11).unwrap();
        history.flush().await.unwrap();

        let loaded = ActivityHistory::load_or_empty(path);
        let loaded_entries = loaded.snapshot_newest_first();
        assert_eq!(loaded_entries.len(), 2);
        assert_eq!(loaded_entries[0].outcome, ActivityOutcome::Failed);
        assert_eq!(
            loaded_entries[1].operation,
            ActivityOperation::CreateRemoteDirectory
        );

        let third = transfer(
            Some("deleteLocalFile"),
            Some("success"),
            Some("old.txt"),
            None,
        );
        assert_eq!(loaded.record_transfer_at(&third, 12).unwrap().id, 3);
    }

    #[test]
    fn missing_and_corrupt_snapshots_fail_open() {
        let temp_dir = tempfile::tempdir().unwrap();
        let missing = temp_dir.path().join("missing.json");
        assert!(ActivityHistory::load_or_empty(missing).is_empty());

        let corrupt = temp_dir.path().join("corrupt.json");
        fs::write(&corrupt, b"{not valid json").unwrap();
        let history = ActivityHistory::load_or_empty(corrupt.clone());
        assert!(history.is_empty());
        assert_eq!(history.path(), Some(corrupt.as_path()));
    }

    #[tokio::test]
    async fn flush_atomically_replaces_snapshot_without_temp_residue() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("activity.json");
        let history = ActivityHistory::load_or_empty(path.clone());
        let first = transfer(
            Some("renameLocalFile"),
            Some("success"),
            Some("renamed.txt"),
            Some(99),
        );
        history.record_transfer_at(&first, 20).unwrap();
        history.flush().await.unwrap();

        let second = transfer(
            Some("deleteLocalDirectory"),
            Some("error"),
            Some("folder"),
            None,
        );
        history.record_transfer_at(&second, 21).unwrap();
        history.flush().await.unwrap();

        let persisted: PersistedActivitySnapshot =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted.version, ACTIVITY_SNAPSHOT_VERSION);
        assert_eq!(persisted.entries.len(), 2);
        assert_eq!(
            persisted.entries[1].operation,
            ActivityOperation::DeleteLocalDirectory
        );

        let leftovers = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn clear_and_flush_survives_reload() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("activity.json");
        let history = ActivityHistory::load_or_empty(path.clone());
        let event = transfer(
            Some("downloadFile"),
            Some("success"),
            Some("temporary.txt"),
            Some(1),
        );
        history.record_transfer_at(&event, 30).unwrap();
        history.flush().await.unwrap();
        history.clear_and_flush().await.unwrap();

        assert!(ActivityHistory::load_or_empty(path).is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_clear_keeps_entries_and_does_not_notify_subscribers() {
        let history = ActivityHistory::load_or_empty("/dev/null/activity-history.json");
        let mut revision_rx = history.subscribe();
        let event = transfer(
            Some("deleteRemoteFile"),
            Some("success"),
            Some("must-remain.txt"),
            None,
        );
        let recorded = history.record_transfer_at(&event, 31).unwrap();
        revision_rx.changed().await.unwrap();
        revision_rx.borrow_and_update();

        assert!(history.clear_and_flush().await.is_err());
        assert_eq!(history.snapshot_newest_first(), vec![recorded]);
        assert!(!revision_rx.has_changed().unwrap());
    }

    #[test]
    fn load_trims_oversized_snapshot_to_newest_entries() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("activity.json");
        let entries = (1..=510)
            .map(|id| ActivityEntry {
                id,
                observed_at_ms: id as i64,
                operation: ActivityOperation::UploadFile,
                outcome: ActivityOutcome::Success,
                relative_path: format!("file-{id}.txt"),
                size: None,
            })
            .collect();
        let snapshot = PersistedActivitySnapshot {
            version: ACTIVITY_SNAPSHOT_VERSION,
            next_id: 511,
            entries,
        };
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();

        let history = ActivityHistory::load_or_empty(path);
        let retained = history.snapshot_newest_first();
        assert_eq!(retained.len(), MAX_ACTIVITY_ENTRIES);
        assert_eq!(retained.first().unwrap().id, 510);
        assert_eq!(retained.last().unwrap().id, 11);
    }

    #[test]
    fn unsupported_snapshot_version_fails_open() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("activity.json");
        let snapshot = PersistedActivitySnapshot {
            version: ACTIVITY_SNAPSHOT_VERSION + 1,
            next_id: FIRST_ACTIVITY_ID,
            entries: Vec::new(),
        };
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();

        assert!(ActivityHistory::load_or_empty(path).is_empty());
    }
}
