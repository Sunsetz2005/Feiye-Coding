//! Durable, versioned recovery for Composer drafts and per-session send queues.
//!
//! Attachment paths are inert references here. Callers must provide a verifier
//! for `get` and `put`; this module never opens attachment content, registers a
//! resource handle, or grants a path. Invalid references are filtered on load.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const COMPOSER_RECOVERY_VERSION: u8 = 1;
pub const COMPOSER_DRAFT_KEY: &str = "__draft__";
pub const COMPOSER_RECOVERY_MAX_QUEUE_ITEMS: usize = 20;

const MAX_SESSION_ROWS: usize = 1024;
const MAX_KEY_BYTES: usize = 128;
const MAX_QUEUE_ID_BYTES: usize = 128;
const MAX_DRAFT_BYTES: usize = 64 * 1024;
const MAX_QUEUE_TEXT_BYTES: usize = 64 * 1024;
const MAX_SESSION_TEXT_BYTES: usize = 256 * 1024;
const MAX_ATTACHMENTS_PER_ITEM: usize = 32;
const MAX_ATTACHMENTS_PER_SESSION: usize = 256;
const MAX_ATTACHMENT_PATH_BYTES: usize = 4096;
const MAX_ATTACHMENT_NAME_BYTES: usize = 512;
const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerAttachmentReferenceV1 {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerQueuedSendV1 {
    pub id: String,
    pub stored_display: String,
    pub attachments: Vec<ComposerAttachmentReferenceV1>,
    pub goal_mode: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoveryStateV1 {
    pub draft: String,
    pub attachments: Vec<ComposerAttachmentReferenceV1>,
    pub queue: Vec<ComposerQueuedSendV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoveryGetRequestV1 {
    pub version: u8,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoveryPutRequestV1 {
    pub version: u8,
    pub key: String,
    pub expected_revision: u64,
    pub state: ComposerRecoveryStateV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoveryMigrateRequestV1 {
    pub version: u8,
    pub from_key: String,
    pub to_key: String,
    pub expected_from_revision: u64,
    pub expected_to_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoveryDeleteRequestV1 {
    pub version: u8,
    pub key: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoverySnapshotV1 {
    pub version: u8,
    pub key: String,
    pub revision: u64,
    pub state: ComposerRecoveryStateV1,
    pub filtered_attachment_count: usize,
    pub filtered_queue_item_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoveryRevisionV1 {
    pub version: u8,
    pub key: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposerRecoveryMigrationV1 {
    pub version: u8,
    pub from_key: String,
    pub from_revision: u64,
    pub to_key: String,
    pub to_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComposerRecoveryRowV1 {
    version: u8,
    key: String,
    revision: u64,
    tombstone: bool,
    state: ComposerRecoveryStateV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComposerRecoveryStoreV1 {
    version: u8,
    sessions: Vec<ComposerRecoveryRowV1>,
}

impl Default for ComposerRecoveryStoreV1 {
    fn default() -> Self {
        Self {
            version: COMPOSER_RECOVERY_VERSION,
            sessions: Vec::new(),
        }
    }
}

fn store_path() -> PathBuf {
    crate::paths::app_data_root().join("composer-recovery.v1.json")
}

/// Load one key and revalidate every attachment reference.
///
/// `authorizer` must only verify an existing authority decision. It must not
/// create a grant as a side effect. Rejected paths are not returned.
pub fn get(
    request: ComposerRecoveryGetRequestV1,
    authorizer: impl FnMut(&ComposerAttachmentReferenceV1) -> bool,
) -> Result<ComposerRecoverySnapshotV1, String> {
    get_at(&store_path(), request, authorizer)
}

/// Replace one key using per-key compare-and-set.
///
/// Revision zero means the key has never been written. Tombstones retain their
/// revision, preventing a stale revision-zero writer from reviving a deletion.
pub fn put(
    request: ComposerRecoveryPutRequestV1,
    authorizer: impl FnMut(&ComposerAttachmentReferenceV1) -> bool,
) -> Result<ComposerRecoveryRevisionV1, String> {
    put_at(&store_path(), request, authorizer)
}

/// Atomically move `__draft__` recovery into a materialized session.
/// Both source and destination revisions are checked inside the same lock.
pub fn migrate(
    request: ComposerRecoveryMigrateRequestV1,
) -> Result<ComposerRecoveryMigrationV1, String> {
    migrate_at(&store_path(), request)
}

/// Clear one session while preserving a revision tombstone for stale-writer
/// protection.
pub fn delete(
    request: ComposerRecoveryDeleteRequestV1,
) -> Result<ComposerRecoveryRevisionV1, String> {
    delete_at(&store_path(), request)
}

fn get_at(
    path: &Path,
    request: ComposerRecoveryGetRequestV1,
    mut authorizer: impl FnMut(&ComposerAttachmentReferenceV1) -> bool,
) -> Result<ComposerRecoverySnapshotV1, String> {
    require_version(request.version)?;
    let key = validate_key(&request.key)?;
    let store = read_store_at(path)?;
    let row = store.sessions.iter().find(|row| row.key == key);
    let revision = row.map(|row| row.revision).unwrap_or(0);
    let mut state = row
        .filter(|row| !row.tombstone)
        .map(|row| row.state.clone())
        .unwrap_or_default();
    let (filtered_attachment_count, filtered_queue_item_count) =
        filter_invalid_attachments(&mut state, &mut authorizer);
    Ok(ComposerRecoverySnapshotV1 {
        version: COMPOSER_RECOVERY_VERSION,
        key,
        revision,
        state,
        filtered_attachment_count,
        filtered_queue_item_count,
    })
}

fn put_at(
    path: &Path,
    request: ComposerRecoveryPutRequestV1,
    mut authorizer: impl FnMut(&ComposerAttachmentReferenceV1) -> bool,
) -> Result<ComposerRecoveryRevisionV1, String> {
    require_version(request.version)?;
    let key = validate_key(&request.key)?;
    validate_state(&request.state)?;
    verify_attachments(&request.state, &mut authorizer)?;

    update_store_at(path, |store| {
        let current_revision = revision_for(store, &key);
        require_revision(request.expected_revision, current_revision)?;
        let revision = next_revision(current_revision)?;
        upsert_row(
            store,
            ComposerRecoveryRowV1 {
                version: COMPOSER_RECOVERY_VERSION,
                key: key.clone(),
                revision,
                tombstone: false,
                state: request.state,
            },
        );
        Ok(ComposerRecoveryRevisionV1 {
            version: COMPOSER_RECOVERY_VERSION,
            key,
            revision,
        })
    })
}

fn migrate_at(
    path: &Path,
    request: ComposerRecoveryMigrateRequestV1,
) -> Result<ComposerRecoveryMigrationV1, String> {
    require_version(request.version)?;
    let from_key = validate_key(&request.from_key)?;
    let to_key = validate_key(&request.to_key)?;
    if from_key != COMPOSER_DRAFT_KEY || to_key == COMPOSER_DRAFT_KEY {
        return Err(
            "COMPOSER_RECOVERY_INVALID: migration must move __draft__ to a session key".into(),
        );
    }

    update_store_at(path, |store| {
        let from_row = store
            .sessions
            .iter()
            .find(|row| row.key == from_key)
            .cloned();
        let to_row = store
            .sessions
            .iter()
            .find(|row| row.key == to_key)
            .cloned();
        let from_revision = from_row.as_ref().map(|row| row.revision).unwrap_or(0);
        let to_revision = to_row.as_ref().map(|row| row.revision).unwrap_or(0);
        require_revision(request.expected_from_revision, from_revision)?;
        require_revision(request.expected_to_revision, to_revision)?;

        let Some(from_row) = from_row.filter(|row| !row.tombstone) else {
            return Ok(ComposerRecoveryMigrationV1 {
                version: COMPOSER_RECOVERY_VERSION,
                from_key,
                from_revision,
                to_key,
                to_revision,
            });
        };

        let target_state = to_row
            .filter(|row| !row.tombstone)
            .map(|row| row.state)
            .unwrap_or_default();
        let merged_state = merge_for_migration(target_state, from_row.state)?;
        let next_from_revision = next_revision(from_revision)?;
        let next_to_revision = next_revision(to_revision)?;

        upsert_row(
            store,
            ComposerRecoveryRowV1 {
                version: COMPOSER_RECOVERY_VERSION,
                key: from_key.clone(),
                revision: next_from_revision,
                tombstone: true,
                state: ComposerRecoveryStateV1::default(),
            },
        );
        upsert_row(
            store,
            ComposerRecoveryRowV1 {
                version: COMPOSER_RECOVERY_VERSION,
                key: to_key.clone(),
                revision: next_to_revision,
                tombstone: false,
                state: merged_state,
            },
        );
        Ok(ComposerRecoveryMigrationV1 {
            version: COMPOSER_RECOVERY_VERSION,
            from_key,
            from_revision: next_from_revision,
            to_key,
            to_revision: next_to_revision,
        })
    })
}

fn delete_at(
    path: &Path,
    request: ComposerRecoveryDeleteRequestV1,
) -> Result<ComposerRecoveryRevisionV1, String> {
    require_version(request.version)?;
    let key = validate_key(&request.key)?;
    update_store_at(path, |store| {
        let current_revision = revision_for(store, &key);
        require_revision(request.expected_revision, current_revision)?;
        let revision = next_revision(current_revision)?;
        upsert_row(
            store,
            ComposerRecoveryRowV1 {
                version: COMPOSER_RECOVERY_VERSION,
                key: key.clone(),
                revision,
                tombstone: true,
                state: ComposerRecoveryStateV1::default(),
            },
        );
        Ok(ComposerRecoveryRevisionV1 {
            version: COMPOSER_RECOVERY_VERSION,
            key,
            revision,
        })
    })
}

fn update_store_at<R>(
    path: &Path,
    update: impl FnOnce(&mut ComposerRecoveryStoreV1) -> Result<R, String>,
) -> Result<R, String> {
    crate::store_lock::update_json_locked(path, ComposerRecoveryStoreV1::default, |store| {
        validate_store(store)?;
        let result = update(store)?;
        store.sessions.sort_by(|left, right| left.key.cmp(&right.key));
        validate_store(store)?;
        Ok(result)
    })
}

fn read_store_at(path: &Path) -> Result<ComposerRecoveryStoreV1, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ComposerRecoveryStoreV1::default());
        }
        Err(error) => {
            return Err(format!(
                "COMPOSER_RECOVERY_READ: read {}: {error}",
                path.display()
            ));
        }
    };
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("COMPOSER_RECOVERY_LIMIT: store exceeds maximum bytes".into());
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(ComposerRecoveryStoreV1::default());
    }
    let store: ComposerRecoveryStoreV1 = serde_json::from_slice(&bytes)
        .map_err(|error| format!("COMPOSER_RECOVERY_PARSE: {}: {error}", path.display()))?;
    validate_store(&store)?;
    Ok(store)
}

fn validate_store(store: &ComposerRecoveryStoreV1) -> Result<(), String> {
    require_version(store.version)?;
    if store.sessions.len() > MAX_SESSION_ROWS {
        return Err("COMPOSER_RECOVERY_LIMIT: too many session rows".into());
    }
    let mut keys = HashSet::with_capacity(store.sessions.len());
    for row in &store.sessions {
        require_version(row.version)?;
        validate_key(&row.key)?;
        if row.revision == 0 {
            return Err("COMPOSER_RECOVERY_INVALID: stored revision must be positive".into());
        }
        if !keys.insert(row.key.as_str()) {
            return Err("COMPOSER_RECOVERY_INVALID: duplicate session key".into());
        }
        if row.tombstone {
            if row.state != ComposerRecoveryStateV1::default() {
                return Err("COMPOSER_RECOVERY_INVALID: tombstone contains state".into());
            }
        } else {
            validate_state(&row.state)?;
        }
    }
    let serialized = serde_json::to_vec(store)
        .map_err(|error| format!("COMPOSER_RECOVERY_SERIALIZE: {error}"))?;
    if serialized.len() as u64 > MAX_STORE_BYTES {
        return Err("COMPOSER_RECOVERY_LIMIT: store exceeds maximum bytes".into());
    }
    Ok(())
}

fn validate_state(state: &ComposerRecoveryStateV1) -> Result<(), String> {
    validate_text("draft", &state.draft, MAX_DRAFT_BYTES)?;
    validate_attachment_list(&state.attachments)?;
    if state.queue.len() > COMPOSER_RECOVERY_MAX_QUEUE_ITEMS {
        return Err("COMPOSER_RECOVERY_LIMIT: send queue exceeds maximum items".into());
    }

    let mut queue_ids = HashSet::with_capacity(state.queue.len());
    let mut total_text_bytes = state.draft.len();
    let mut total_attachments = state.attachments.len();
    for item in &state.queue {
        validate_identifier("queue id", &item.id, MAX_QUEUE_ID_BYTES)?;
        if !queue_ids.insert(item.id.as_str()) {
            return Err("COMPOSER_RECOVERY_INVALID: duplicate queue id".into());
        }
        validate_text("queue text", &item.stored_display, MAX_QUEUE_TEXT_BYTES)?;
        validate_attachment_list(&item.attachments)?;
        if item.stored_display.trim().is_empty() && item.attachments.is_empty() {
            return Err("COMPOSER_RECOVERY_INVALID: empty queued send".into());
        }
        if item.created_at < 0 {
            return Err("COMPOSER_RECOVERY_INVALID: negative queue timestamp".into());
        }
        total_text_bytes = total_text_bytes
            .checked_add(item.stored_display.len())
            .ok_or_else(|| "COMPOSER_RECOVERY_LIMIT: text size overflow".to_string())?;
        total_attachments = total_attachments
            .checked_add(item.attachments.len())
            .ok_or_else(|| "COMPOSER_RECOVERY_LIMIT: attachment count overflow".to_string())?;
    }
    if total_text_bytes > MAX_SESSION_TEXT_BYTES {
        return Err("COMPOSER_RECOVERY_LIMIT: session text exceeds maximum bytes".into());
    }
    if total_attachments > MAX_ATTACHMENTS_PER_SESSION {
        return Err("COMPOSER_RECOVERY_LIMIT: session attachments exceed maximum count".into());
    }
    Ok(())
}

fn validate_attachment_list(
    attachments: &[ComposerAttachmentReferenceV1],
) -> Result<(), String> {
    if attachments.len() > MAX_ATTACHMENTS_PER_ITEM {
        return Err("COMPOSER_RECOVERY_LIMIT: too many attachments on one item".into());
    }
    let mut paths = HashSet::with_capacity(attachments.len());
    for attachment in attachments {
        if attachment.path.is_empty()
            || attachment.path.len() > MAX_ATTACHMENT_PATH_BYTES
            || attachment.path.chars().any(char::is_control)
            || !is_absolute_reference(&attachment.path)
        {
            return Err("COMPOSER_RECOVERY_INVALID: invalid attachment path".into());
        }
        if attachment.name.trim().is_empty()
            || attachment.name.len() > MAX_ATTACHMENT_NAME_BYTES
            || attachment.name.chars().any(char::is_control)
        {
            return Err("COMPOSER_RECOVERY_INVALID: invalid attachment name".into());
        }
        if !paths.insert(attachment.path.as_str()) {
            return Err("COMPOSER_RECOVERY_INVALID: duplicate attachment path".into());
        }
    }
    Ok(())
}

fn validate_text(label: &str, text: &str, max_bytes: usize) -> Result<(), String> {
    if text.len() > max_bytes {
        return Err(format!(
            "COMPOSER_RECOVERY_LIMIT: {label} exceeds maximum bytes"
        ));
    }
    if text
        .chars()
        .any(|character| character.is_control() && !character.is_whitespace())
    {
        return Err(format!(
            "COMPOSER_RECOVERY_INVALID: {label} contains control characters"
        ));
    }
    Ok(())
}

fn validate_key(value: &str) -> Result<String, String> {
    validate_identifier("key", value, MAX_KEY_BYTES)
}

fn validate_identifier(label: &str, value: &str, max_bytes: usize) -> Result<String, String> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("COMPOSER_RECOVERY_INVALID: invalid {label}"));
    }
    Ok(value.to_string())
}

fn is_absolute_reference(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

fn require_version(version: u8) -> Result<(), String> {
    if version == COMPOSER_RECOVERY_VERSION {
        Ok(())
    } else {
        Err("COMPOSER_RECOVERY_UNSUPPORTED_VERSION: expected schema v1".into())
    }
}

fn require_revision(expected: u64, current: u64) -> Result<(), String> {
    if expected == current {
        Ok(())
    } else {
        Err(format!(
            "COMPOSER_RECOVERY_STALE: expected revision {expected}, current revision {current}"
        ))
    }
}

fn next_revision(current: u64) -> Result<u64, String> {
    current
        .checked_add(1)
        .ok_or_else(|| "COMPOSER_RECOVERY_LIMIT: revision overflow".to_string())
}

fn revision_for(store: &ComposerRecoveryStoreV1, key: &str) -> u64 {
    store
        .sessions
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.revision)
        .unwrap_or(0)
}

fn upsert_row(store: &mut ComposerRecoveryStoreV1, row: ComposerRecoveryRowV1) {
    if let Some(existing) = store.sessions.iter_mut().find(|item| item.key == row.key) {
        *existing = row;
    } else {
        store.sessions.push(row);
    }
}

fn verify_attachments(
    state: &ComposerRecoveryStateV1,
    authorizer: &mut impl FnMut(&ComposerAttachmentReferenceV1) -> bool,
) -> Result<(), String> {
    for attachment in state
        .attachments
        .iter()
        .chain(state.queue.iter().flat_map(|item| item.attachments.iter()))
    {
        if !authorizer(attachment) {
            return Err(
                "COMPOSER_RECOVERY_ATTACHMENT_DENIED: attachment reference is not authorized"
                    .into(),
            );
        }
    }
    Ok(())
}

fn filter_invalid_attachments(
    state: &mut ComposerRecoveryStateV1,
    authorizer: &mut impl FnMut(&ComposerAttachmentReferenceV1) -> bool,
) -> (usize, usize) {
    let mut filtered_attachments = 0;
    state.attachments.retain(|attachment| {
        let valid = authorizer(attachment);
        if !valid {
            filtered_attachments += 1;
        }
        valid
    });
    for item in &mut state.queue {
        item.attachments.retain(|attachment| {
            let valid = authorizer(attachment);
            if !valid {
                filtered_attachments += 1;
            }
            valid
        });
    }
    let before = state.queue.len();
    state
        .queue
        .retain(|item| !item.stored_display.trim().is_empty() || !item.attachments.is_empty());
    (filtered_attachments, before - state.queue.len())
}

fn merge_for_migration(
    mut target: ComposerRecoveryStateV1,
    source: ComposerRecoveryStateV1,
) -> Result<ComposerRecoveryStateV1, String> {
    let source_has_draft = !source.draft.is_empty() || !source.attachments.is_empty();
    let target_has_draft = !target.draft.is_empty() || !target.attachments.is_empty();
    if source_has_draft {
        if !target_has_draft {
            target.draft = source.draft;
            target.attachments = source.attachments;
        } else if target.draft != source.draft || target.attachments != source.attachments {
            return Err(
                "COMPOSER_RECOVERY_MIGRATION_CONFLICT: destination draft is not empty".into(),
            );
        }
    }

    let mut queue_ids: HashSet<String> =
        target.queue.iter().map(|item| item.id.clone()).collect();
    for item in source.queue {
        if !queue_ids.insert(item.id.clone()) {
            return Err(
                "COMPOSER_RECOVERY_MIGRATION_CONFLICT: duplicate queue id".into(),
            );
        }
        target.queue.push(item);
    }
    validate_state(&target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use uuid::Uuid;

    fn test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sunsetz-composer-recovery-{label}-{}-{}.json",
            std::process::id(),
            Uuid::new_v4()
        ))
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(crate::store_lock::lock_path_for(path));
    }

    fn attachment(path: &str) -> ComposerAttachmentReferenceV1 {
        ComposerAttachmentReferenceV1 {
            path: path.into(),
            name: path.rsplit('/').next().unwrap_or("attachment").into(),
            is_dir: false,
        }
    }

    fn queued(id: &str, text: &str) -> ComposerQueuedSendV1 {
        ComposerQueuedSendV1 {
            id: id.into(),
            stored_display: text.into(),
            attachments: Vec::new(),
            goal_mode: false,
            created_at: 1,
        }
    }

    fn put_request(
        key: &str,
        expected_revision: u64,
        state: ComposerRecoveryStateV1,
    ) -> ComposerRecoveryPutRequestV1 {
        ComposerRecoveryPutRequestV1 {
            version: COMPOSER_RECOVERY_VERSION,
            key: key.into(),
            expected_revision,
            state,
        }
    }

    fn get_request(key: &str) -> ComposerRecoveryGetRequestV1 {
        ComposerRecoveryGetRequestV1 {
            version: COMPOSER_RECOVERY_VERSION,
            key: key.into(),
        }
    }

    #[test]
    fn missing_file_loads_empty_without_creating_storage() {
        let path = test_path("missing");
        let snapshot = get_at(&path, get_request("session-1"), |_| true).unwrap();
        assert_eq!(snapshot.revision, 0);
        assert_eq!(snapshot.state, ComposerRecoveryStateV1::default());
        assert_eq!(snapshot.filtered_attachment_count, 0);
        assert!(!path.exists());
        cleanup(&path);
    }

    #[test]
    fn serde_rejects_unknown_fields_at_store_and_request_boundaries() {
        let path = test_path("unknown-fields");
        fs::write(
            &path,
            r#"{"version":1,"sessions":[],"unexpected":true}"#,
        )
        .unwrap();
        assert!(get_at(&path, get_request("session-1"), |_| true)
            .unwrap_err()
            .contains("COMPOSER_RECOVERY_PARSE"));

        let request = serde_json::json!({
            "version": 1,
            "key": "session-1",
            "expectedRevision": 0,
            "state": { "draft": "", "attachments": [], "queue": [] },
            "unexpected": true
        });
        assert!(serde_json::from_value::<ComposerRecoveryPutRequestV1>(request).is_err());
        cleanup(&path);
    }

    #[test]
    fn put_is_cas_and_stale_write_does_not_replace_state() {
        let path = test_path("cas");
        let initial = ComposerRecoveryStateV1 {
            draft: "first".into(),
            ..ComposerRecoveryStateV1::default()
        };
        let saved = put_at(&path, put_request("session-1", 0, initial), |_| true).unwrap();
        assert_eq!(saved.revision, 1);

        let stale = ComposerRecoveryStateV1 {
            draft: "stale".into(),
            ..ComposerRecoveryStateV1::default()
        };
        assert!(put_at(&path, put_request("session-1", 0, stale), |_| true)
            .unwrap_err()
            .contains("COMPOSER_RECOVERY_STALE"));
        let loaded = get_at(&path, get_request("session-1"), |_| true).unwrap();
        assert_eq!(loaded.revision, 1);
        assert_eq!(loaded.state.draft, "first");
        cleanup(&path);
    }

    #[test]
    fn concurrent_writers_with_same_revision_have_one_winner() {
        let path = test_path("concurrent-cas");
        let barrier = Arc::new(Barrier::new(3));
        let mut threads = Vec::new();
        for draft in ["one", "two"] {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            threads.push(thread::spawn(move || {
                let state = ComposerRecoveryStateV1 {
                    draft: draft.into(),
                    ..ComposerRecoveryStateV1::default()
                };
                barrier.wait();
                put_at(&path, put_request("session-1", 0, state), |_| true)
            }));
        }
        barrier.wait();
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|error| error.contains("COMPOSER_RECOVERY_STALE")))
                .count(),
            1
        );
        assert_eq!(
            get_at(&path, get_request("session-1"), |_| true)
                .unwrap()
                .revision,
            1
        );
        cleanup(&path);
    }

    #[test]
    fn bounds_fail_without_creating_or_advancing_store() {
        let path = test_path("bounds");
        let oversized = ComposerRecoveryStateV1 {
            draft: "x".repeat(MAX_DRAFT_BYTES + 1),
            ..ComposerRecoveryStateV1::default()
        };
        assert!(put_at(&path, put_request("session-1", 0, oversized), |_| true)
            .unwrap_err()
            .contains("COMPOSER_RECOVERY_LIMIT"));
        assert!(!path.exists());

        let overfull_queue = ComposerRecoveryStateV1 {
            queue: (0..=COMPOSER_RECOVERY_MAX_QUEUE_ITEMS)
                .map(|index| queued(&format!("q-{index}"), "body"))
                .collect(),
            ..ComposerRecoveryStateV1::default()
        };
        assert!(put_at(
            &path,
            put_request("session-1", 0, overfull_queue),
            |_| true
        )
        .unwrap_err()
        .contains("COMPOSER_RECOVERY_LIMIT"));
        assert!(!path.exists());
        cleanup(&path);
    }

    #[test]
    fn attachment_authority_is_required_on_put_and_rechecked_on_get() {
        let path = test_path("attachment-authority");
        let mut only_attachment = queued("q-only", "");
        only_attachment.attachments = vec![attachment("/gone-only.txt")];
        let mut text_and_attachment = queued("q-text", "keep text");
        text_and_attachment.attachments = vec![attachment("/gone-text.txt")];
        let state = ComposerRecoveryStateV1 {
            draft: "draft".into(),
            attachments: vec![attachment("/allowed.txt"), attachment("/gone-draft.txt")],
            queue: vec![only_attachment, text_and_attachment],
        };
        put_at(&path, put_request("session-1", 0, state), |_| true).unwrap();

        let loaded = get_at(&path, get_request("session-1"), |reference| {
            reference.path == "/allowed.txt"
        })
        .unwrap();
        assert_eq!(loaded.state.attachments, vec![attachment("/allowed.txt")]);
        assert_eq!(loaded.filtered_attachment_count, 3);
        assert_eq!(loaded.filtered_queue_item_count, 1);
        assert_eq!(loaded.state.queue.len(), 1);
        assert_eq!(loaded.state.queue[0].id, "q-text");
        assert!(loaded.state.queue[0].attachments.is_empty());

        let denied = ComposerRecoveryStateV1 {
            attachments: vec![attachment("/not-authorized.txt")],
            ..ComposerRecoveryStateV1::default()
        };
        assert!(put_at(&path, put_request("session-2", 0, denied), |_| false)
            .unwrap_err()
            .contains("COMPOSER_RECOVERY_ATTACHMENT_DENIED"));
        assert_eq!(
            get_at(&path, get_request("session-2"), |_| true)
                .unwrap()
                .revision,
            0
        );
        cleanup(&path);
    }

    #[test]
    fn draft_migration_is_double_cas_and_appends_queue() {
        let path = test_path("migrate");
        let draft_state = ComposerRecoveryStateV1 {
            draft: "draft body".into(),
            attachments: vec![attachment("/draft.txt")],
            queue: vec![queued("q-draft", "draft follow-up")],
        };
        let target_state = ComposerRecoveryStateV1 {
            queue: vec![queued("q-existing", "existing follow-up")],
            ..ComposerRecoveryStateV1::default()
        };
        put_at(
            &path,
            put_request(COMPOSER_DRAFT_KEY, 0, draft_state),
            |_| true,
        )
        .unwrap();
        put_at(&path, put_request("session-1", 0, target_state), |_| true).unwrap();

        let request = ComposerRecoveryMigrateRequestV1 {
            version: COMPOSER_RECOVERY_VERSION,
            from_key: COMPOSER_DRAFT_KEY.into(),
            to_key: "session-1".into(),
            expected_from_revision: 1,
            expected_to_revision: 1,
        };
        let migrated = migrate_at(&path, request.clone()).unwrap();
        assert_eq!(migrated.from_revision, 2);
        assert_eq!(migrated.to_revision, 2);

        let source = get_at(&path, get_request(COMPOSER_DRAFT_KEY), |_| true).unwrap();
        assert_eq!(source.revision, 2);
        assert_eq!(source.state, ComposerRecoveryStateV1::default());
        let target = get_at(&path, get_request("session-1"), |_| true).unwrap();
        assert_eq!(target.revision, 2);
        assert_eq!(target.state.draft, "draft body");
        assert_eq!(target.state.attachments, vec![attachment("/draft.txt")]);
        assert_eq!(
            target
                .state
                .queue
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["q-existing", "q-draft"]
        );
        assert!(migrate_at(&path, request)
            .unwrap_err()
            .contains("COMPOSER_RECOVERY_STALE"));
        cleanup(&path);
    }

    #[test]
    fn migration_conflict_leaves_both_revisions_unchanged() {
        let path = test_path("migrate-conflict");
        for (key, draft) in [(COMPOSER_DRAFT_KEY, "source"), ("session-1", "target")] {
            let state = ComposerRecoveryStateV1 {
                draft: draft.into(),
                ..ComposerRecoveryStateV1::default()
            };
            put_at(&path, put_request(key, 0, state), |_| true).unwrap();
        }
        let error = migrate_at(
            &path,
            ComposerRecoveryMigrateRequestV1 {
                version: COMPOSER_RECOVERY_VERSION,
                from_key: COMPOSER_DRAFT_KEY.into(),
                to_key: "session-1".into(),
                expected_from_revision: 1,
                expected_to_revision: 1,
            },
        )
        .unwrap_err();
        assert!(error.contains("COMPOSER_RECOVERY_MIGRATION_CONFLICT"));
        assert_eq!(
            get_at(&path, get_request(COMPOSER_DRAFT_KEY), |_| true)
                .unwrap()
                .revision,
            1
        );
        assert_eq!(
            get_at(&path, get_request("session-1"), |_| true)
                .unwrap()
                .revision,
            1
        );
        cleanup(&path);
    }

    #[test]
    fn delete_keeps_tombstone_revision_and_blocks_stale_revival() {
        let path = test_path("delete");
        let state = ComposerRecoveryStateV1 {
            draft: "do not revive".into(),
            ..ComposerRecoveryStateV1::default()
        };
        put_at(&path, put_request("session-1", 0, state.clone()), |_| true).unwrap();
        let deleted = delete_at(
            &path,
            ComposerRecoveryDeleteRequestV1 {
                version: COMPOSER_RECOVERY_VERSION,
                key: "session-1".into(),
                expected_revision: 1,
            },
        )
        .unwrap();
        assert_eq!(deleted.revision, 2);
        let loaded = get_at(&path, get_request("session-1"), |_| true).unwrap();
        assert_eq!(loaded.revision, 2);
        assert_eq!(loaded.state, ComposerRecoveryStateV1::default());
        assert!(put_at(&path, put_request("session-1", 1, state), |_| true)
            .unwrap_err()
            .contains("COMPOSER_RECOVERY_STALE"));
        cleanup(&path);
    }
}
