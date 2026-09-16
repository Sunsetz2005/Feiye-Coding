//! Versioned, redacted audit snapshots for Runtime reverse-request interactions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::acp_client::AskUserQuestionItem;

const INTERACTION_VERSION: u32 = 1;
const MAX_AUDIT_ROWS: usize = 256;
const MAX_PREVIEW_CHARS: usize = 2_000;
const MAX_PLAN_CHARS: usize = 64_000;
const MAX_OPTIONS_CHARS: usize = 16_000;
const MAX_QUESTIONS: usize = 12;
const MAX_QUESTION_OPTIONS: usize = 24;
const MAX_AUDIT_FILE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InteractionStatusV1 {
    Pending,
    Resolving,
    Resolved,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum InteractionPayloadV1 {
    Permission {
        tool_name: String,
        title: String,
        preview: String,
        scope_key: String,
        options: Value,
        #[serde(default)]
        destructive: bool,
    },
    AskUser {
        questions: Vec<AskUserQuestionItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        partial_answers: Option<Value>,
    },
    Plan {
        entries: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSnapshotV1 {
    pub version: u32,
    pub interaction_id: String,
    pub session_id: String,
    pub process_id: String,
    pub rpc_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub status: InteractionStatusV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub payload: InteractionPayloadV1,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveInteractionRequestV1 {
    pub interaction_id: String,
    pub session_id: String,
    pub decision: String,
    #[serde(default)]
    pub option_id: Option<String>,
    #[serde(default)]
    pub scope_key: Option<String>,
    #[serde(default)]
    pub feedback: Option<String>,
    #[serde(default)]
    pub answers: Option<Value>,
}

impl InteractionSnapshotV1 {
    pub fn new(
        session_id: &str,
        process_id: &str,
        rpc_id: u64,
        tool_call_id: Option<String>,
        payload: InteractionPayloadV1,
    ) -> Self {
        let now = Utc::now();
        Self {
            version: INTERACTION_VERSION,
            interaction_id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            process_id: process_id.to_string(),
            rpc_id,
            tool_call_id,
            status: InteractionStatusV1::Pending,
            created_at: now,
            updated_at: now,
            payload: bound_payload(payload),
        }
    }

    pub fn set_status(&mut self, status: InteractionStatusV1) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    pub fn claim(
        &mut self,
        requested_interaction_id: Option<&str>,
        requested_rpc_id: Option<u64>,
    ) -> Result<(), String> {
        if requested_interaction_id.is_some_and(|id| id != self.interaction_id) {
            return Err("stale interaction id".into());
        }
        if requested_rpc_id.is_some_and(|id| id != self.rpc_id) {
            return Err(format!(
                "stale interaction rpc: expected {}, got {}",
                self.rpc_id,
                requested_rpc_id.unwrap_or_default()
            ));
        }
        if self.status == InteractionStatusV1::Resolving {
            return Err("interaction is already resolving".into());
        }
        if self.status != InteractionStatusV1::Pending {
            return Err("interaction is no longer pending".into());
        }
        self.set_status(InteractionStatusV1::Resolving);
        Ok(())
    }

    pub fn restore_pending(&mut self) -> bool {
        if self.status != InteractionStatusV1::Resolving {
            return false;
        }
        self.set_status(InteractionStatusV1::Pending);
        true
    }

    pub fn resolve(&mut self) -> bool {
        if self.status != InteractionStatusV1::Resolving {
            return false;
        }
        self.set_status(InteractionStatusV1::Resolved);
        true
    }
}

fn truncate_chars(value: String, max: usize) -> String {
    if value.chars().count() <= max {
        value
    } else {
        value.chars().take(max).collect()
    }
}

fn redact_bounded(value: &str, max: usize) -> String {
    truncate_chars(crate::store::redact_text(value), max)
}

fn bounded_json(value: Value, max_chars: usize) -> Value {
    let sanitized = crate::runtime_events::sanitize_payload(&value);
    let serialized = serde_json::to_string(&sanitized).unwrap_or_default();
    if serialized.chars().count() <= max_chars {
        sanitized
    } else {
        serde_json::json!({
            "truncated": true,
            "preview": redact_bounded(&serialized, max_chars),
        })
    }
}

fn bound_questions(questions: Vec<AskUserQuestionItem>, redact: bool) -> Vec<AskUserQuestionItem> {
    questions
        .into_iter()
        .take(MAX_QUESTIONS)
        .map(|mut question| {
            let text = if redact {
                redact_bounded(&question.question, MAX_PREVIEW_CHARS)
            } else {
                truncate_chars(question.question, MAX_PREVIEW_CHARS)
            };
            question.id = truncate_chars(question.id, 160);
            question.question = text;
            question.options = question
                .options
                .into_iter()
                .take(MAX_QUESTION_OPTIONS)
                .map(|mut option| {
                    option.id = truncate_chars(option.id, 160);
                    option.label = if redact {
                        redact_bounded(&option.label, 512)
                    } else {
                        truncate_chars(option.label, 512)
                    };
                    option.description = option.description.map(|description| {
                        if redact {
                            redact_bounded(&description, 1_000)
                        } else {
                            truncate_chars(description, 1_000)
                        }
                    });
                    option
                })
                .collect();
            question
        })
        .collect()
}

fn audit_scope(scope_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(scope_key.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn bound_payload(payload: InteractionPayloadV1) -> InteractionPayloadV1 {
    match payload {
        InteractionPayloadV1::Permission {
            tool_name,
            title,
            preview,
            scope_key,
            options,
            destructive,
        } => InteractionPayloadV1::Permission {
            tool_name,
            title: truncate_chars(title, MAX_PREVIEW_CHARS),
            preview: truncate_chars(preview, MAX_PREVIEW_CHARS),
            scope_key: truncate_chars(scope_key, MAX_PREVIEW_CHARS),
            options: bounded_json(options, MAX_OPTIONS_CHARS),
            destructive,
        },
        InteractionPayloadV1::Plan { entries, body } => InteractionPayloadV1::Plan {
            entries: bounded_json(entries, MAX_PLAN_CHARS),
            body: body.map(|value| truncate_chars(value, MAX_PLAN_CHARS)),
        },
        InteractionPayloadV1::AskUser {
            questions,
            partial_answers,
        } => InteractionPayloadV1::AskUser {
            questions: bound_questions(questions, false),
            partial_answers: partial_answers.map(|value| bounded_json(value, MAX_PLAN_CHARS)),
        },
    }
}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn audit_path(session_id: &str) -> Result<std::path::PathBuf, String> {
    if !valid_session_id(session_id) {
        return Err("invalid interaction session id".into());
    }
    Ok(crate::paths::session_dir(session_id).join("interactions.v1.json"))
}

fn audit_safe_snapshot(snapshot: &InteractionSnapshotV1) -> InteractionSnapshotV1 {
    let mut safe = snapshot.clone();
    safe.payload = match safe.payload {
        InteractionPayloadV1::Permission {
            tool_name,
            title,
            preview,
            scope_key,
            options,
            destructive,
        } => InteractionPayloadV1::Permission {
            tool_name: redact_bounded(&tool_name, 256),
            title: redact_bounded(&title, MAX_PREVIEW_CHARS),
            preview: redact_bounded(&preview, MAX_PREVIEW_CHARS),
            scope_key: audit_scope(&scope_key),
            options: bounded_json(options, MAX_OPTIONS_CHARS),
            destructive,
        },
        InteractionPayloadV1::AskUser { questions, .. } => InteractionPayloadV1::AskUser {
            questions: bound_questions(questions, true),
            // Submitted answers are retry state, not audit data. They stay only
            // in the live interaction and are never written to disk.
            partial_answers: None,
        },
        InteractionPayloadV1::Plan { entries, body } => InteractionPayloadV1::Plan {
            entries: bounded_json(entries, MAX_PLAN_CHARS),
            body: body.map(|value| redact_bounded(&value, MAX_PLAN_CHARS)),
        },
    };
    safe
}

pub fn record(snapshot: &InteractionSnapshotV1) -> Result<(), String> {
    let path = audit_path(&snapshot.session_id)?;
    let snapshot = audit_safe_snapshot(snapshot);
    crate::store_lock::update_json_locked(&path, Vec::<InteractionSnapshotV1>::new, |rows| {
        if let Some(existing) = rows
            .iter_mut()
            .find(|row| row.interaction_id == snapshot.interaction_id)
        {
            *existing = snapshot.clone();
        } else {
            rows.push(snapshot.clone());
        }
        if rows.len() > MAX_AUDIT_ROWS {
            let remove = rows.len() - MAX_AUDIT_ROWS;
            rows.drain(0..remove);
        }
        Ok(())
    })
}

pub fn load(session_id: &str) -> Vec<InteractionSnapshotV1> {
    let Ok(path) = audit_path(session_id) else {
        return Vec::new();
    };
    if path
        .metadata()
        .map(|metadata| metadata.len() > MAX_AUDIT_FILE_BYTES)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_bounds_permission_preview() {
        let snapshot = InteractionSnapshotV1::new(
            "session",
            "process",
            7,
            Some("tool".into()),
            InteractionPayloadV1::Permission {
                tool_name: "shell".into(),
                title: "Run".into(),
                preview: "x".repeat(MAX_PREVIEW_CHARS + 10),
                scope_key: "shell:echo ok".into(),
                options: serde_json::json!([]),
                destructive: false,
            },
        );
        let InteractionPayloadV1::Permission { preview, .. } = snapshot.payload else {
            panic!("permission payload expected");
        };
        assert_eq!(preview.chars().count(), MAX_PREVIEW_CHARS);
    }

    #[test]
    fn claim_rejects_stale_and_duplicate_resolution() {
        let mut snapshot = InteractionSnapshotV1::new(
            "session",
            "process",
            7,
            None,
            InteractionPayloadV1::Plan {
                entries: serde_json::json!([]),
                body: Some("plan".into()),
            },
        );
        assert!(snapshot.claim(Some("stale"), Some(7)).is_err());
        let id = snapshot.interaction_id.clone();
        snapshot.claim(Some(&id), Some(7)).unwrap();
        assert!(snapshot.claim(Some(&id), Some(7)).is_err());
        assert!(snapshot.restore_pending());
        snapshot.claim(Some(&id), Some(7)).unwrap();
        assert!(snapshot.resolve());
        assert!(snapshot.claim(Some(&id), Some(7)).is_err());
    }

    #[test]
    fn audit_copy_hashes_scope_and_drops_partial_answers() {
        let permission = InteractionSnapshotV1::new(
            "session",
            "process",
            1,
            None,
            InteractionPayloadV1::Permission {
                tool_name: "write".into(),
                title: "Write".into(),
                preview: "secret".into(),
                scope_key: "write:/Users/example/private.txt".into(),
                options: serde_json::json!([]),
                destructive: false,
            },
        );
        let safe = audit_safe_snapshot(&permission);
        let InteractionPayloadV1::Permission { scope_key, .. } = safe.payload else {
            panic!("permission payload expected");
        };
        assert!(scope_key.starts_with("sha256:"));
        assert!(!scope_key.contains("/Users"));

        let mut ask = InteractionSnapshotV1::new(
            "session",
            "process",
            2,
            None,
            InteractionPayloadV1::AskUser {
                questions: Vec::new(),
                partial_answers: None,
            },
        );
        if let InteractionPayloadV1::AskUser {
            partial_answers, ..
        } = &mut ask.payload
        {
            *partial_answers = Some(serde_json::json!({"answer": "private"}));
        }
        let safe = audit_safe_snapshot(&ask);
        assert!(matches!(
            safe.payload,
            InteractionPayloadV1::AskUser {
                partial_answers: None,
                ..
            }
        ));
    }

    #[test]
    fn audit_paths_reject_traversal_ids() {
        assert!(audit_path("../../outside").is_err());
        assert!(audit_path("normal-session_1").is_ok());
    }
}
