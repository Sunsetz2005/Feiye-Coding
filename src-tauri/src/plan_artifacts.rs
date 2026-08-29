//! Durable, versioned Plan artifacts with bounded audit history.
//!
//! Live Plan approval remains an ACP interaction. This sidecar records the
//! authored artifact and its long-lived product lifecycle; it is never used to
//! resurrect a dead JSON-RPC request.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PLAN_ARTIFACT_STORE_VERSION: u8 = 1;
const MAX_PLAN_ARTIFACTS: usize = 64;
const MAX_PLAN_REVISIONS: usize = 32;
const MAX_PLAN_TRANSITIONS: usize = 64;
const MAX_PLAN_BODY_CHARS: usize = 64_000;
const MAX_PLAN_ENTRIES_CHARS: usize = 128_000;
const MAX_PLAN_ENTRIES_BYTES: usize = 256 * 1024;
const MAX_PLAN_FEEDBACK_CHARS: usize = 4_000;
const MAX_IDENTIFIER_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanArtifactStatusV1 {
    Proposed,
    Approved,
    RevisionRequested,
    Executing,
    Completed,
    Abandoned,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanArtifactRevisionV1 {
    pub revision: u32,
    pub content_hash: String,
    pub body: Option<String>,
    pub entries: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanArtifactTransitionV1 {
    pub status: PlanArtifactStatusV1,
    pub occurred_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanArtifactV1 {
    pub version: u8,
    pub id: String,
    pub session_id: String,
    pub process_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub status: PlanArtifactStatusV1,
    pub current_revision: u32,
    pub revisions: Vec<PlanArtifactRevisionV1>,
    pub transitions: Vec<PlanArtifactTransitionV1>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimePlanArtifactRecordV1 {
    pub version: u8,
    pub session_id: String,
    pub process_id: String,
    #[serde(default)]
    pub interaction_id: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub entries: Value,
    pub awaiting_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanArtifactStoreV1 {
    version: u8,
    artifacts: Vec<PlanArtifactV1>,
}

impl Default for PlanArtifactStoreV1 {
    fn default() -> Self {
        Self {
            version: PLAN_ARTIFACT_STORE_VERSION,
            artifacts: Vec::new(),
        }
    }
}

fn store_path(session_id: &str) -> Result<PathBuf, String> {
    let session_id = validate_identifier("session id", session_id)?;
    Ok(crate::paths::session_dir(&session_id).join("plan-artifacts.v1.json"))
}

fn validate_identifier(label: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > MAX_IDENTIFIER_CHARS {
        return Err(format!("invalid plan artifact {label}"));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(format!("invalid plan artifact {label}"));
    }
    Ok(value.to_string())
}

fn normalize_optional_identifier(
    label: &str,
    value: Option<String>,
) -> Result<Option<String>, String> {
    value
        .map(|value| validate_identifier(label, &value))
        .transpose()
}

fn bounded_redacted(value: &str, max_chars: usize) -> String {
    redact_inline_credentials(crate::store::redact_text(value).trim_end())
        .chars()
        .take(max_chars)
        .collect()
}

fn redact_inline_credentials(value: &str) -> String {
    let mut output = Vec::new();
    let mut redact_next = false;

    for word in value.split_whitespace() {
        let normalized = word
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .to_ascii_lowercase();

        if redact_next {
            if matches!(normalized.as_str(), "bearer" | "basic" | "digest") {
                output.push(word.to_string());
                continue;
            }
            output.push("[REDACTED]".into());
            redact_next = false;
            continue;
        }

        let lower = word.to_ascii_lowercase();
        let sensitive_prefix = [
            "authorization",
            "api_key",
            "apikey",
            "password",
            "secret",
            "token",
        ]
        .iter()
        .find(|key| lower.starts_with(**key));

        if normalized == "bearer" {
            output.push(word.to_string());
            redact_next = true;
        } else if let Some(prefix) = sensitive_prefix {
            let tail = &word[prefix.len()..];
            if tail == ":" || tail == "=" {
                output.push(word.to_string());
                redact_next = true;
            } else if tail.starts_with(':') || tail.starts_with('=') {
                output.push(format!("{}{}[REDACTED]", &word[..prefix.len()], &tail[..1]));
            } else {
                output.push(word.to_string());
            }
        } else {
            output.push(word.to_string());
        }
    }

    output.join(" ")
}

fn sanitize_entries(value: &Value) -> Value {
    fn take_bounded(value: &str, max_chars: usize, remaining: &mut usize) -> String {
        let take = max_chars.min(*remaining);
        let bounded = bounded_redacted(value, take);
        *remaining = remaining.saturating_sub(bounded.chars().count());
        bounded
    }

    fn walk(value: &Value, depth: usize, remaining: &mut usize) -> Value {
        if depth >= 8 || *remaining == 0 {
            return Value::String("[TRUNCATED]".into());
        }
        *remaining = remaining.saturating_sub(1);
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => {
                *remaining = remaining.saturating_sub(16);
                value.clone()
            }
            Value::String(value) => Value::String(take_bounded(value, 8_000, remaining)),
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .take(128)
                    .map_while(|value| (*remaining > 0).then(|| walk(value, depth + 1, remaining)))
                    .collect(),
            ),
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .take(128)
                    .map_while(|(key, value)| {
                        if *remaining == 0 {
                            return None;
                        }
                        let sensitive = [
                            "authorization",
                            "cookie",
                            "secret",
                            "password",
                            "token",
                            "api_key",
                            "apikey",
                        ]
                        .iter()
                        .any(|needle| key.to_ascii_lowercase().contains(needle));
                        let key = take_bounded(key, 128, remaining);
                        Some((
                            key,
                            if sensitive {
                                Value::String("[REDACTED]".into())
                            } else {
                                walk(value, depth + 1, remaining)
                            },
                        ))
                    })
                    .collect(),
            ),
        }
    }
    let mut remaining = MAX_PLAN_ENTRIES_CHARS;
    walk(value, 0, &mut remaining)
}

fn content_hash(body: Option<&str>, entries: &Value) -> String {
    let canonical = serde_json::json!({ "body": body, "entries": entries });
    hex::encode(Sha256::digest(
        serde_json::to_vec(&canonical).unwrap_or_default(),
    ))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn terminal(status: PlanArtifactStatusV1) -> bool {
    matches!(
        status,
        PlanArtifactStatusV1::Completed
            | PlanArtifactStatusV1::Abandoned
            | PlanArtifactStatusV1::Interrupted
    )
}

fn status_from_entries(entries: &Value, fallback: PlanArtifactStatusV1) -> PlanArtifactStatusV1 {
    let Some(entries) = entries.as_array().filter(|entries| !entries.is_empty()) else {
        return fallback;
    };
    let statuses = entries
        .iter()
        .filter_map(|entry| entry.get("status").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if statuses.len() == entries.len()
        && statuses
            .iter()
            .all(|status| matches!(*status, "completed" | "done"))
    {
        PlanArtifactStatusV1::Completed
    } else if statuses
        .iter()
        .any(|status| matches!(*status, "in_progress" | "executing" | "completed" | "done"))
    {
        PlanArtifactStatusV1::Executing
    } else {
        fallback
    }
}

fn validate_revision(revision: &PlanArtifactRevisionV1) -> Result<(), String> {
    if revision.revision == 0 || !valid_hash(&revision.content_hash) {
        return Err("invalid plan artifact revision".into());
    }
    if revision
        .body
        .as_deref()
        .is_some_and(|body| body.chars().count() > MAX_PLAN_BODY_CHARS)
    {
        return Err("plan artifact body exceeds limit".into());
    }
    if serde_json::to_vec(&revision.entries)
        .map_err(|error| format!("serialize plan artifact entries: {error}"))?
        .len()
        > MAX_PLAN_ENTRIES_BYTES
    {
        return Err("plan artifact entries exceed limit".into());
    }
    if content_hash(revision.body.as_deref(), &revision.entries) != revision.content_hash {
        return Err("plan artifact revision hash mismatch".into());
    }
    Ok(())
}

fn validate_artifact(artifact: &PlanArtifactV1) -> Result<(), String> {
    if artifact.version != PLAN_ARTIFACT_STORE_VERSION {
        return Err("unsupported plan artifact version".into());
    }
    validate_identifier("id", &artifact.id)?;
    validate_identifier("session id", &artifact.session_id)?;
    validate_identifier("process id", &artifact.process_id)?;
    if let Some(value) = artifact.interaction_id.as_deref() {
        validate_identifier("interaction id", value)?;
    }
    if let Some(value) = artifact.tool_call_id.as_deref() {
        validate_identifier("tool call id", value)?;
    }
    if artifact.revisions.is_empty()
        || artifact.revisions.len() > MAX_PLAN_REVISIONS
        || artifact.transitions.is_empty()
        || artifact.transitions.len() > MAX_PLAN_TRANSITIONS
        || artifact.current_revision
            != artifact
                .revisions
                .last()
                .map(|item| item.revision)
                .unwrap_or(0)
    {
        return Err("invalid plan artifact history".into());
    }
    for (index, revision) in artifact.revisions.iter().enumerate() {
        validate_revision(revision)?;
        if revision.revision as usize != index + 1 {
            return Err("plan artifact revisions are not contiguous".into());
        }
    }
    if artifact
        .transitions
        .last()
        .is_none_or(|transition| transition.status != artifact.status)
        || artifact.transitions.iter().any(|transition| {
            transition
                .feedback
                .as_deref()
                .is_some_and(|feedback| feedback.chars().count() > MAX_PLAN_FEEDBACK_CHARS)
        })
    {
        return Err("invalid plan artifact transitions".into());
    }
    Ok(())
}

fn validate_store(store: &PlanArtifactStoreV1) -> Result<(), String> {
    if store.version != PLAN_ARTIFACT_STORE_VERSION {
        return Err("unsupported plan artifact store version".into());
    }
    if store.artifacts.len() > MAX_PLAN_ARTIFACTS {
        return Err("plan artifact store exceeds limit".into());
    }
    let mut ids = HashSet::new();
    for artifact in &store.artifacts {
        validate_artifact(artifact)?;
        if !ids.insert(&artifact.id) {
            return Err("duplicate plan artifact id".into());
        }
    }
    Ok(())
}

fn append_transition(
    artifact: &mut PlanArtifactV1,
    status: PlanArtifactStatusV1,
    feedback: Option<&str>,
    now: DateTime<Utc>,
) {
    if artifact.transitions.last().is_some_and(|event| {
        event.status == status
            && event.feedback.as_deref()
                == feedback.map(str::trim).filter(|value| !value.is_empty())
    }) {
        artifact.status = status;
        artifact.updated_at = now.max(artifact.created_at);
        return;
    }
    if artifact.transitions.len() >= MAX_PLAN_TRANSITIONS {
        let remove = artifact.transitions.len() + 1 - MAX_PLAN_TRANSITIONS;
        artifact.transitions.drain(..remove);
    }
    artifact.transitions.push(PlanArtifactTransitionV1 {
        status,
        occurred_at: now,
        feedback: feedback
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| bounded_redacted(value, MAX_PLAN_FEEDBACK_CHARS)),
    });
    artifact.status = status;
    artifact.updated_at = now.max(artifact.created_at);
}

fn trim_store(store: &mut PlanArtifactStoreV1) -> Result<(), String> {
    if store.artifacts.len() <= MAX_PLAN_ARTIFACTS {
        return Ok(());
    }
    let mut artifacts = std::mem::take(&mut store.artifacts);
    artifacts.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    let (mut kept, mut terminal_rows): (Vec<_>, Vec<_>) = artifacts
        .into_iter()
        .partition(|artifact| !terminal(artifact.status));
    if kept.len() > MAX_PLAN_ARTIFACTS {
        kept.extend(terminal_rows);
        kept.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        store.artifacts = kept;
        return Err("too many active plan artifacts".into());
    }
    let room = MAX_PLAN_ARTIFACTS.saturating_sub(kept.len());
    terminal_rows.truncate(room);
    kept.extend(terminal_rows);
    kept.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    store.artifacts = kept;
    Ok(())
}

fn read_store_at(path: &Path) -> Result<PlanArtifactStoreV1, String> {
    if !path.exists() {
        return Ok(PlanArtifactStoreV1::default());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("read plan artifacts: {error}"))?;
    let store: PlanArtifactStoreV1 =
        serde_json::from_slice(&bytes).map_err(|error| format!("parse plan artifacts: {error}"))?;
    validate_store(&store)?;
    Ok(store)
}

fn update_store_at<R>(
    path: &Path,
    update: impl FnOnce(&mut PlanArtifactStoreV1) -> Result<R, String>,
) -> Result<R, String> {
    crate::store_lock::update_json_locked(path, PlanArtifactStoreV1::default, |store| {
        validate_store(store)?;
        let result = update(store)?;
        trim_store(store)?;
        validate_store(store)?;
        Ok(result)
    })
}

pub fn list(session_id: &str) -> Result<Vec<PlanArtifactV1>, String> {
    list_at(&store_path(session_id)?)
}

fn list_at(path: &Path) -> Result<Vec<PlanArtifactV1>, String> {
    let mut artifacts = read_store_at(path)?.artifacts;
    artifacts.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(artifacts)
}

/// Pick the artifact the workbench should restore for a session.
/// Live `proposed` reviews outrank older completed rows; abandoned rows never display.
pub fn display_artifact(artifacts: &[PlanArtifactV1]) -> Option<&PlanArtifactV1> {
    let mut visible = artifacts
        .iter()
        .filter(|artifact| artifact.status != PlanArtifactStatusV1::Abandoned)
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return None;
    }
    if visible
        .iter()
        .any(|artifact| artifact.status == PlanArtifactStatusV1::Proposed)
    {
        visible.retain(|artifact| artifact.status == PlanArtifactStatusV1::Proposed);
    }
    visible.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    visible.into_iter().next()
}

pub fn record_runtime_plan(request: RuntimePlanArtifactRecordV1) -> Result<PlanArtifactV1, String> {
    let path = store_path(&request.session_id)?;
    record_runtime_plan_at(&path, request, Utc::now())
}

fn record_runtime_plan_at(
    path: &Path,
    request: RuntimePlanArtifactRecordV1,
    now: DateTime<Utc>,
) -> Result<PlanArtifactV1, String> {
    if request.version != PLAN_ARTIFACT_STORE_VERSION {
        return Err("unsupported runtime plan artifact version".into());
    }
    let session_id = validate_identifier("session id", &request.session_id)?;
    let process_id = validate_identifier("process id", &request.process_id)?;
    let interaction_id = normalize_optional_identifier("interaction id", request.interaction_id)?;
    let tool_call_id = normalize_optional_identifier("tool call id", request.tool_call_id)?;
    if request.awaiting_review && interaction_id.is_none() {
        return Err("reviewable plan artifact requires an interaction id".into());
    }
    let body = request
        .body
        .as_deref()
        .map(|body| bounded_redacted(body, MAX_PLAN_BODY_CHARS))
        .filter(|body| !body.trim().is_empty());
    let entries = sanitize_entries(&request.entries);
    let hash = content_hash(body.as_deref(), &entries);

    update_store_at(path, |store| {
        let existing_index = store.artifacts.iter().position(|artifact| {
            interaction_id
                .as_ref()
                .is_some_and(|id| artifact.interaction_id.as_ref() == Some(id))
                || tool_call_id.as_ref().is_some_and(|id| {
                    artifact.tool_call_id.as_ref() == Some(id) && !terminal(artifact.status)
                })
        });
        let index = if let Some(index) = existing_index {
            index
        } else {
            let artifact_id = format!("plan_{}", Uuid::new_v4().simple());
            let revision = PlanArtifactRevisionV1 {
                revision: 1,
                content_hash: hash.clone(),
                body: body.clone(),
                entries: entries.clone(),
                created_at: now,
            };
            let status = if request.awaiting_review {
                PlanArtifactStatusV1::Proposed
            } else {
                status_from_entries(&entries, PlanArtifactStatusV1::Executing)
            };
            store.artifacts.push(PlanArtifactV1 {
                version: PLAN_ARTIFACT_STORE_VERSION,
                id: artifact_id,
                session_id: session_id.clone(),
                process_id: process_id.clone(),
                interaction_id: interaction_id.clone(),
                tool_call_id: tool_call_id.clone(),
                status,
                current_revision: 1,
                revisions: vec![revision],
                transitions: vec![PlanArtifactTransitionV1 {
                    status,
                    occurred_at: now,
                    feedback: None,
                }],
                created_at: now,
                updated_at: now,
            });
            store.artifacts.len() - 1
        };

        let artifact = &mut store.artifacts[index];
        if artifact.process_id != process_id {
            return Err("plan artifact process mismatch".into());
        }
        if terminal(artifact.status) {
            let incoming_status = if request.awaiting_review {
                PlanArtifactStatusV1::Proposed
            } else {
                status_from_entries(&entries, artifact.status)
            };
            let exact_completed_replay = artifact.status == PlanArtifactStatusV1::Completed
                && incoming_status == PlanArtifactStatusV1::Completed
                && artifact
                    .revisions
                    .last()
                    .is_some_and(|revision| revision.content_hash == hash);
            if exact_completed_replay {
                return Ok(artifact.clone());
            }
            return Err("plan artifact is terminal".into());
        }
        if request.awaiting_review
            && artifact.status == PlanArtifactStatusV1::RevisionRequested
            && interaction_id.is_some()
        {
            artifact.interaction_id = interaction_id.clone();
        } else if artifact.interaction_id.is_some()
            && interaction_id.is_some()
            && artifact.interaction_id != interaction_id
        {
            return Err("plan artifact interaction mismatch".into());
        }
        if artifact
            .revisions
            .last()
            .is_none_or(|revision| revision.content_hash != hash)
        {
            if artifact.revisions.len() >= MAX_PLAN_REVISIONS {
                return Err("plan artifact revision limit reached".into());
            }
            let revision = artifact.current_revision.saturating_add(1);
            artifact.revisions.push(PlanArtifactRevisionV1 {
                revision,
                content_hash: hash,
                body,
                entries: entries.clone(),
                created_at: now,
            });
            artifact.current_revision = revision;
        }
        if artifact.interaction_id.is_none() {
            artifact.interaction_id = interaction_id.clone();
        }
        if artifact.tool_call_id.is_none() {
            artifact.tool_call_id = tool_call_id;
        }
        let next_status = if request.awaiting_review {
            PlanArtifactStatusV1::Proposed
        } else {
            status_from_entries(
                &entries,
                if artifact.status == PlanArtifactStatusV1::Approved {
                    PlanArtifactStatusV1::Executing
                } else {
                    artifact.status
                },
            )
        };
        append_transition(artifact, next_status, None, now);
        Ok(artifact.clone())
    })
}

pub fn resolve_plan(
    session_id: &str,
    interaction_id: &str,
    decision: &str,
    feedback: Option<&str>,
) -> Result<PlanArtifactV1, String> {
    let path = store_path(session_id)?;
    resolve_plan_at(&path, interaction_id, decision, feedback, Utc::now())
}

fn resolve_plan_at(
    path: &Path,
    interaction_id: &str,
    decision: &str,
    feedback: Option<&str>,
    now: DateTime<Utc>,
) -> Result<PlanArtifactV1, String> {
    let interaction_id = validate_identifier("interaction id", interaction_id)?;
    let status = match decision.trim() {
        "approved" => PlanArtifactStatusV1::Approved,
        "cancelled" => PlanArtifactStatusV1::RevisionRequested,
        "abandoned" => PlanArtifactStatusV1::Abandoned,
        _ => return Err("unsupported plan artifact decision".into()),
    };
    update_store_at(path, |store| {
        let artifact = store
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.interaction_id.as_deref() == Some(&interaction_id))
            .ok_or_else(|| "plan artifact not found".to_string())?;
        if artifact.status != PlanArtifactStatusV1::Proposed {
            return Err("plan artifact is not awaiting review".into());
        }
        append_transition(artifact, status, feedback, now);
        Ok(artifact.clone())
    })
}

pub fn interrupt_process(
    session_id: &str,
    process_id: &str,
) -> Result<Vec<PlanArtifactV1>, String> {
    let path = store_path(session_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    interrupt_process_at(&path, process_id, Utc::now())
}

fn interrupt_process_at(
    path: &Path,
    process_id: &str,
    now: DateTime<Utc>,
) -> Result<Vec<PlanArtifactV1>, String> {
    let process_id = validate_identifier("process id", process_id)?;
    update_store_at(path, |store| {
        let mut interrupted = Vec::new();
        for artifact in store.artifacts.iter_mut().filter(|artifact| {
            artifact.process_id == process_id
                && matches!(
                    artifact.status,
                    PlanArtifactStatusV1::Proposed
                        | PlanArtifactStatusV1::Approved
                        | PlanArtifactStatusV1::Executing
                        | PlanArtifactStatusV1::RevisionRequested
                )
        }) {
            append_transition(artifact, PlanArtifactStatusV1::Interrupted, None, now);
            interrupted.push(artifact.clone());
        }
        Ok(interrupted)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    struct TestStore {
        root: PathBuf,
        path: PathBuf,
    }

    impl TestStore {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("sunsetz-plan-artifacts-{name}-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self {
                path: root.join("plan-artifacts.v1.json"),
                root,
            }
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn now(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + seconds, 0).unwrap()
    }

    fn record(
        interaction_id: Option<&str>,
        body: &str,
        entries: Value,
        awaiting_review: bool,
    ) -> RuntimePlanArtifactRecordV1 {
        RuntimePlanArtifactRecordV1 {
            version: 1,
            session_id: "session-1".into(),
            process_id: "process-1".into(),
            interaction_id: interaction_id.map(str::to_string),
            tool_call_id: Some("tool-1".into()),
            body: Some(body.into()),
            entries,
            awaiting_review,
        }
    }

    #[test]
    fn plan_lifecycle_persists_revisions_without_reviving_rpc() {
        let store = TestStore::new("lifecycle");
        let proposed = record_runtime_plan_at(
            &store.path,
            record(
                Some("interaction-1"),
                "# Plan\n\nInspect",
                serde_json::json!([{ "content": "Inspect", "status": "pending" }]),
                true,
            ),
            now(0),
        )
        .unwrap();
        assert_eq!(proposed.status, PlanArtifactStatusV1::Proposed);
        assert_eq!(proposed.current_revision, 1);

        let approved =
            resolve_plan_at(&store.path, "interaction-1", "approved", None, now(1)).unwrap();
        assert_eq!(approved.status, PlanArtifactStatusV1::Approved);

        let executing = record_runtime_plan_at(
            &store.path,
            record(
                Some("interaction-1"),
                "# Plan\n\nInspect\nImplement",
                serde_json::json!([
                    { "content": "Inspect", "status": "completed" },
                    { "content": "Implement", "status": "in_progress" }
                ]),
                false,
            ),
            now(2),
        )
        .unwrap();
        assert_eq!(executing.status, PlanArtifactStatusV1::Executing);
        assert_eq!(executing.current_revision, 2);

        let completed = record_runtime_plan_at(
            &store.path,
            record(
                Some("interaction-1"),
                "# Plan\n\nInspect\nImplement",
                serde_json::json!([
                    { "content": "Inspect", "status": "completed" },
                    { "content": "Implement", "status": "completed" }
                ]),
                false,
            ),
            now(3),
        )
        .unwrap();
        assert_eq!(completed.status, PlanArtifactStatusV1::Completed);
        assert_eq!(completed.current_revision, 3);
        assert!(
            resolve_plan_at(&store.path, "interaction-1", "approved", None, now(4))
                .unwrap_err()
                .contains("not awaiting")
        );
        assert_eq!(
            list_at(&store.path).unwrap()[0].status,
            PlanArtifactStatusV1::Completed
        );
    }

    #[test]
    fn revision_request_and_abandon_are_explicit_and_redacted() {
        let store = TestStore::new("review-decisions");
        record_runtime_plan_at(
            &store.path,
            record(Some("interaction-1"), "Plan", serde_json::json!([]), true),
            now(0),
        )
        .unwrap();
        let revised = resolve_plan_at(
            &store.path,
            "interaction-1",
            "cancelled",
            Some("Authorization: Bearer secret-value"),
            now(1),
        )
        .unwrap();
        assert_eq!(revised.status, PlanArtifactStatusV1::RevisionRequested);
        assert!(!revised
            .transitions
            .last()
            .unwrap()
            .feedback
            .as_deref()
            .unwrap()
            .contains("secret-value"));

        let reproposed = record_runtime_plan_at(
            &store.path,
            record(
                Some("interaction-1"),
                "Revised plan",
                serde_json::json!([]),
                true,
            ),
            now(2),
        )
        .unwrap();
        assert_eq!(reproposed.status, PlanArtifactStatusV1::Proposed);
        assert_eq!(reproposed.current_revision, 2);
        let abandoned =
            resolve_plan_at(&store.path, "interaction-1", "abandoned", None, now(3)).unwrap();
        assert_eq!(abandoned.status, PlanArtifactStatusV1::Abandoned);
    }

    #[test]
    fn revised_plan_moves_to_the_new_live_interaction() {
        let store = TestStore::new("new-interaction");
        record_runtime_plan_at(
            &store.path,
            record(Some("interaction-1"), "Plan", serde_json::json!([]), true),
            now(0),
        )
        .unwrap();
        resolve_plan_at(
            &store.path,
            "interaction-1",
            "cancelled",
            Some("Clarify the verification step"),
            now(1),
        )
        .unwrap();

        let revised = record_runtime_plan_at(
            &store.path,
            record(
                Some("interaction-2"),
                "Revised plan",
                serde_json::json!([]),
                true,
            ),
            now(2),
        )
        .unwrap();
        assert_eq!(revised.interaction_id.as_deref(), Some("interaction-2"));
        assert!(resolve_plan_at(&store.path, "interaction-1", "approved", None, now(3),).is_err());
        assert_eq!(
            resolve_plan_at(&store.path, "interaction-2", "approved", None, now(3),)
                .unwrap()
                .status,
            PlanArtifactStatusV1::Approved
        );
    }

    #[test]
    fn completed_runtime_replay_is_idempotent_but_cannot_revive() {
        let store = TestStore::new("terminal-replay");
        let completed_request = record(
            Some("interaction-1"),
            "Plan",
            serde_json::json!([{ "content": "Inspect", "status": "completed" }]),
            false,
        );
        let completed =
            record_runtime_plan_at(&store.path, completed_request.clone(), now(0)).unwrap();
        assert_eq!(completed.status, PlanArtifactStatusV1::Completed);
        let replay = record_runtime_plan_at(&store.path, completed_request, now(1)).unwrap();
        assert_eq!(replay.id, completed.id);
        assert_eq!(replay.revisions.len(), 1);

        let changed = record(
            Some("interaction-1"),
            "Changed after completion",
            serde_json::json!([{ "content": "Inspect again", "status": "pending" }]),
            true,
        );
        assert!(record_runtime_plan_at(&store.path, changed, now(2))
            .unwrap_err()
            .contains("terminal"));
        assert_eq!(
            list_at(&store.path).unwrap()[0].status,
            PlanArtifactStatusV1::Completed
        );
    }

    #[test]
    fn plan_entries_are_globally_bounded_and_sensitive_keys_are_redacted() {
        let store = TestStore::new("bounded-entries");
        let entries = Value::Array(
            (0..256)
                .map(|index| {
                    serde_json::json!({
                        "content": "x".repeat(8_000),
                        "status": "pending",
                        "authorizationToken": format!("secret-{index}"),
                    })
                })
                .collect(),
        );
        let artifact = record_runtime_plan_at(
            &store.path,
            record(Some("interaction-1"), "Plan", entries, true),
            now(0),
        )
        .unwrap();
        let encoded = serde_json::to_vec(&artifact.revisions[0].entries).unwrap();
        assert!(encoded.len() <= MAX_PLAN_ENTRIES_BYTES);
        let text = String::from_utf8(encoded).unwrap();
        assert!(text.contains("[REDACTED]"));
        assert!(!text.contains("secret-1"));
    }

    #[test]
    fn process_exit_interrupts_only_active_artifacts_from_that_process() {
        let store = TestStore::new("interrupt");
        record_runtime_plan_at(
            &store.path,
            record(Some("interaction-1"), "Plan", serde_json::json!([]), true),
            now(0),
        )
        .unwrap();
        let interrupted = interrupt_process_at(&store.path, "process-1", now(1)).unwrap();
        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].status, PlanArtifactStatusV1::Interrupted);
        assert!(interrupt_process_at(&store.path, "process-1", now(2))
            .unwrap()
            .is_empty());
        assert!(
            resolve_plan_at(&store.path, "interaction-1", "approved", None, now(3))
                .unwrap_err()
                .contains("not awaiting")
        );
    }

    #[test]
    fn display_artifact_prefers_live_proposed_over_older_completed() {
        let store = TestStore::new("display");
        record_runtime_plan_at(
            &store.path,
            RuntimePlanArtifactRecordV1 {
                version: 1,
                session_id: "session-1".into(),
                process_id: "process-1".into(),
                interaction_id: Some("interaction-old".into()),
                tool_call_id: Some("tool-old".into()),
                body: Some("Old completed plan".into()),
                entries: serde_json::json!([{ "content": "Inspect", "status": "completed" }]),
                awaiting_review: false,
            },
            now(0),
        )
        .unwrap();
        record_runtime_plan_at(
            &store.path,
            record(
                Some("interaction-new"),
                "Live proposed plan",
                serde_json::json!([]),
                true,
            ),
            now(1),
        )
        .unwrap();
        let artifacts = list_at(&store.path).unwrap();
        let display = display_artifact(&artifacts).unwrap();
        assert_eq!(display.status, PlanArtifactStatusV1::Proposed);
        assert_eq!(display.interaction_id.as_deref(), Some("interaction-new"));
        assert_eq!(
            display.revisions.last().and_then(|row| row.body.as_deref()),
            Some("Live proposed plan")
        );

        resolve_plan_at(&store.path, "interaction-new", "abandoned", None, now(2)).unwrap();
        let after_abandon = list_at(&store.path).unwrap();
        let fallback = display_artifact(&after_abandon).unwrap();
        assert_eq!(fallback.status, PlanArtifactStatusV1::Completed);
        assert_eq!(
            fallback
                .revisions
                .last()
                .and_then(|row| row.body.as_deref()),
            Some("Old completed plan")
        );
    }

    #[test]
    fn duplicate_runtime_update_is_deterministic_and_readable() {
        let store = TestStore::new("duplicate");
        let request = record(
            Some("interaction-1"),
            "Plan",
            serde_json::json!([{ "content": "Inspect", "status": "pending" }]),
            true,
        );
        let first = record_runtime_plan_at(&store.path, request.clone(), now(0)).unwrap();
        let second = record_runtime_plan_at(&store.path, request, now(1)).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.revisions.len(), 1);
        assert_eq!(list_at(&store.path).unwrap().len(), 1);
    }

    #[test]
    fn invalid_ids_versions_and_review_shape_fail_closed() {
        let store = TestStore::new("invalid");
        let mut request = record(None, "Plan", serde_json::json!([]), true);
        assert!(record_runtime_plan_at(&store.path, request.clone(), now(0))
            .unwrap_err()
            .contains("interaction"));
        request.awaiting_review = false;
        request.session_id = "../escape".into();
        assert!(record_runtime_plan_at(&store.path, request.clone(), now(0)).is_err());
        request.session_id = "session-1".into();
        request.version = 2;
        assert!(record_runtime_plan_at(&store.path, request, now(0)).is_err());
    }

    #[test]
    fn unsupported_store_is_never_replaced() {
        let store = TestStore::new("unsupported-store");
        let bytes = br#"{"version":9,"artifacts":[]}"#.to_vec();
        std::fs::write(&store.path, &bytes).unwrap();
        let result = record_runtime_plan_at(
            &store.path,
            record(Some("interaction-1"), "Plan", serde_json::json!([]), true),
            now(0),
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), bytes);
    }
}
