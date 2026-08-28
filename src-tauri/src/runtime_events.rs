//! Versioned Runtime event envelope emitted alongside legacy `session://*` events.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::{AcpEvent, StreamKind};

const MAX_STRING_CHARS: usize = 16_384;
const MAX_ARRAY_ITEMS: usize = 64;
const MAX_OBJECT_FIELDS: usize = 96;
const MAX_DEPTH: usize = 6;

static SEQUENCES: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
static AUTOMATION_HEARTBEAT_ATTEMPTS: OnceLock<Mutex<HashMap<String, DateTime<Utc>>>> =
    OnceLock::new();
const AUTOMATION_HEARTBEAT_INTERVAL_SECONDS: i64 = 30;
const MAX_AUTOMATION_HEARTBEAT_SESSION_TRACKERS: usize = 256;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEventEnvelopeV1 {
    pub version: u8,
    pub event_id: String,
    pub sequence: u64,
    pub occurred_at: DateTime<Utc>,
    pub session_id: String,
    pub agent_session_id: Option<String>,
    pub process_id: String,
    pub turn_id: Option<String>,
    pub tool_call_id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Value,
}

fn next_sequence(session_id: &str) -> u64 {
    let sequences = SEQUENCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut sequences = sequences.lock().unwrap_or_else(|e| e.into_inner());
    let slot = sequences.entry(session_id.to_string()).or_insert(0);
    *slot = slot.saturating_add(1);
    *slot
}

fn is_automation_progress_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "stream"
            | "tool_call"
            | "plan"
            | "ask_user"
            | "permission"
            | "retry_state"
            | "context_compact"
            | "usage"
    )
}

fn should_record_automation_heartbeat(session_id: &str, now: DateTime<Utc>) -> bool {
    let attempts = AUTOMATION_HEARTBEAT_ATTEMPTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut attempts = attempts.lock().unwrap_or_else(|error| error.into_inner());
    if attempts.get(session_id).is_some_and(|previous| {
        now.signed_duration_since(*previous)
            < ChronoDuration::seconds(AUTOMATION_HEARTBEAT_INTERVAL_SECONDS)
    }) {
        return false;
    }
    if !attempts.contains_key(session_id)
        && attempts.len() >= MAX_AUTOMATION_HEARTBEAT_SESSION_TRACKERS
    {
        if let Some(oldest) = attempts
            .iter()
            .min_by(|left, right| left.1.cmp(right.1))
            .map(|(id, _)| id.clone())
        {
            attempts.remove(&oldest);
        }
    }
    attempts.insert(session_id.to_string(), now);
    true
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "cookie",
        "secret",
        "password",
        "passwd",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn bounded_string(value: &str) -> String {
    if value.chars().count() <= MAX_STRING_CHARS {
        return value.to_string();
    }
    let mut out: String = value.chars().take(MAX_STRING_CHARS).collect();
    out.push('…');
    out
}

pub fn sanitize_payload(value: &Value) -> Value {
    fn walk(value: &Value, depth: usize) -> Value {
        if depth >= MAX_DEPTH {
            return Value::String("[TRUNCATED]".into());
        }
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
            Value::String(text) => Value::String(bounded_string(text)),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .take(MAX_ARRAY_ITEMS)
                    .map(|item| walk(item, depth + 1))
                    .collect(),
            ),
            Value::Object(fields) => {
                let mut out = Map::new();
                for (key, value) in fields.iter().take(MAX_OBJECT_FIELDS) {
                    out.insert(
                        key.clone(),
                        if is_sensitive_key(key) {
                            Value::String("[REDACTED]".into())
                        } else {
                            walk(value, depth + 1)
                        },
                    );
                }
                Value::Object(out)
            }
        }
    }
    walk(value, 0)
}

fn event_parts(event: &AcpEvent) -> (&'static str, Option<String>, Value) {
    match event {
        AcpEvent::State {
            backend,
            agent_session_id,
            model_id,
        } => (
            "state",
            None,
            json!({
                "backend": backend,
                "agentSessionId": agent_session_id,
                "modelId": model_id,
            }),
        ),
        AcpEvent::Stream {
            kind,
            text,
            message_id,
            done,
        } => (
            "stream",
            None,
            json!({
                "kind": match kind {
                    StreamKind::Assistant => "assistant",
                    StreamKind::Thought => "thought",
                },
                "text": text,
                "messageId": message_id,
                "done": done,
            }),
        ),
        AcpEvent::ToolCall {
            tool_call_id,
            title,
            kind,
            status,
            raw,
        } => (
            "tool_call",
            Some(tool_call_id.clone()),
            json!({ "title": title, "kind": kind, "status": status, "raw": raw }),
        ),
        AcpEvent::Plan {
            entries,
            body,
            rpc_id,
            tool_call_id,
        } => (
            "plan",
            tool_call_id.clone(),
            json!({ "entries": entries, "body": body, "rpcId": rpc_id }),
        ),
        AcpEvent::AskUserQuestion {
            rpc_id,
            tool_call_id,
            questions,
            raw,
        } => (
            "ask_user",
            tool_call_id.clone(),
            json!({ "rpcId": rpc_id, "questionCount": questions.len(), "raw": raw }),
        ),
        AcpEvent::PermissionRequest {
            rpc_id,
            tool_call_id,
            tool_name,
            title,
            options,
            raw,
        } => (
            "permission",
            Some(tool_call_id.clone()),
            json!({
                "rpcId": rpc_id,
                "toolName": tool_name,
                "title": title,
                "options": options,
                "raw": raw,
            }),
        ),
        AcpEvent::PromptComplete { stop_reason } => (
            "prompt_complete",
            None,
            json!({ "stopReason": stop_reason }),
        ),
        AcpEvent::RetryState {
            attempt,
            max_retries,
            reason,
            status,
        } => (
            "retry_state",
            None,
            json!({
                "attempt": attempt,
                "maxRetries": max_retries,
                "reason": reason,
                "status": status,
            }),
        ),
        AcpEvent::ContextCompact {
            trigger,
            tokens_before,
            tokens_after,
            summary_preview,
            note,
        } => (
            "context_compact",
            None,
            json!({
                "trigger": trigger,
                "tokensBefore": tokens_before,
                "tokensAfter": tokens_after,
                "summaryPreview": summary_preview,
                "note": note,
            }),
        ),
        AcpEvent::Usage {
            input_tokens,
            output_tokens,
            cached_read_tokens,
            reasoning_tokens,
            model_calls,
            model_id,
        } => (
            "usage",
            None,
            json!({
                "inputTokens": input_tokens,
                "outputTokens": output_tokens,
                "cachedReadTokens": cached_read_tokens,
                "reasoningTokens": reasoning_tokens,
                "modelCalls": model_calls,
                "modelId": model_id,
            }),
        ),
        AcpEvent::Error { error } => (
            "error",
            None,
            json!({ "code": error.code.as_str(), "message": error.message }),
        ),
        AcpEvent::Stderr { line } => ("stderr", None, json!({ "line": line })),
        AcpEvent::ProcessExited { code } => ("process_exited", None, json!({ "code": code })),
        AcpEvent::Unknown { method, payload } => (
            "unknown",
            None,
            json!({ "method": bounded_string(method), "payload": payload }),
        ),
    }
}

pub fn emit(
    app: &AppHandle,
    session_id: &str,
    agent_session_id: Option<String>,
    process_id: &str,
    turn_id: Option<String>,
    event: &AcpEvent,
) {
    let (event_type, tool_call_id, payload) = event_parts(event);
    let occurred_at = Utc::now();
    let envelope = RuntimeEventEnvelopeV1 {
        version: 1,
        event_id: Uuid::new_v4().to_string(),
        sequence: next_sequence(session_id),
        occurred_at,
        session_id: session_id.to_string(),
        agent_session_id,
        process_id: process_id.to_string(),
        turn_id,
        tool_call_id,
        event_type: event_type.to_string(),
        payload: sanitize_payload(&payload),
    };
    let _ = app.emit("session://runtime_event_v1", envelope.clone());
    if envelope.event_type == "process_exited" {
        if let Err(error) = crate::automation_scheduler::record_process_termination_for_session_v1(
            session_id,
            process_id,
            envelope.sequence,
            crate::automation_scheduler::AutomationTerminationProofKindV1::ProcessExit,
        ) {
            tracing::warn!(
                session_id,
                process_id,
                sequence = envelope.sequence,
                "automation Runtime termination proof rejected: {error}"
            );
        }
    }
    if is_automation_progress_event(&envelope.event_type)
        && should_record_automation_heartbeat(session_id, occurred_at)
    {
        if let Err(error) = crate::automation_scheduler::record_runtime_heartbeat_for_session_v1(
            session_id,
            envelope.sequence,
            process_id,
        ) {
            tracing::warn!(
                session_id,
                sequence = envelope.sequence,
                "automation Runtime heartbeat rejected: {error}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_redacts_secrets_and_bounds_strings() {
        let long = "x".repeat(MAX_STRING_CHARS + 10);
        let value = sanitize_payload(&json!({
            "authorization": "Bearer secret",
            "nested": { "apiKey": "secret", "safe": long },
        }));
        assert_eq!(value["authorization"], "[REDACTED]");
        assert_eq!(value["nested"]["apiKey"], "[REDACTED]");
        assert!(value["nested"]["safe"].as_str().unwrap().ends_with('…'));
    }

    #[test]
    fn sequence_is_monotonic_per_session() {
        let session = Uuid::new_v4().to_string();
        assert_eq!(next_sequence(&session), 1);
        assert_eq!(next_sequence(&session), 2);
        let other = Uuid::new_v4().to_string();
        assert_eq!(next_sequence(&other), 1);
    }

    #[test]
    fn automation_heartbeat_accepts_only_progress_and_is_rate_limited_per_session() {
        assert!(is_automation_progress_event("stream"));
        assert!(is_automation_progress_event("tool_call"));
        assert!(!is_automation_progress_event("stderr"));
        assert!(!is_automation_progress_event("process_exited"));

        let session = Uuid::new_v4().to_string();
        let other = Uuid::new_v4().to_string();
        let now = Utc::now();
        assert!(should_record_automation_heartbeat(&session, now));
        assert!(!should_record_automation_heartbeat(
            &session,
            now + ChronoDuration::seconds(AUTOMATION_HEARTBEAT_INTERVAL_SECONDS - 1),
        ));
        assert!(should_record_automation_heartbeat(
            &session,
            now + ChronoDuration::seconds(AUTOMATION_HEARTBEAT_INTERVAL_SECONDS),
        ));
        assert!(should_record_automation_heartbeat(&other, now));
    }
}
