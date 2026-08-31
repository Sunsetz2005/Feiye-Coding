//! Host session manager: Sunsetz agent kernel by default.
//! Legacy `grok agent stdio` ACP is used only when `runtimeBackend=grok_acp`.
//! Mock only if SUNSETZ_ACP=mock.
//!
//! Process policy (I01–I03):
//! - One ACP process per live/parked App session (up to `maxConcurrentAgents`, default 3).
//! - Switching chats parks a Ready process instead of killing it (when under the cap).
//! - Idle processes are soft-recycled after `agentIdleMinutes` (default 30); session meta stays.
//!
//! Streaming performance (I04 / I06):
//! - Mid-stream journal upserts are throttled (≥500ms or paragraph / force).
//! - Pure stream silence past `streamStallSeconds` emits `session://stream_stall`.

mod subagents;

pub use subagents::SubagentView;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::{
    parse_ask_user_question_params, should_abort_provider_retry, AcpClient, AcpEvent,
    AskUserOutcome, AskUserQuestionItem, PermissionOutcome, StreamKind, TransportWriteAck,
    HOST_PROVIDER_MAX_RETRIES,
};
use crate::agent_loop;
use crate::cli_probe;
use crate::error::{AgentError, AgentErrorCode};
use crate::interactions::{InteractionPayloadV1, InteractionSnapshotV1, InteractionStatusV1};
use crate::journal_throttle::{is_paragraph_break, JournalWriteThrottle};
use crate::mock_acp::{self, MockConnectMode, MockStreamHandle, StreamChunk};
use crate::permission::{
    extract_path_target, extract_shell_command, may_auto_allow, may_auto_deny,
    permission_scope_key, pick_option_id, PermissionPolicy, SessionAllowCache,
};
use crate::process_limits::{
    can_spawn_process, is_idle_expired, normalize_idle_minutes, normalize_max_concurrent,
    process_limit_message,
};
use crate::session_fsm::{SessionFsm, SessionState};
use crate::store::{self, ChatMessageStored, MessageAttachmentStored, SessionMeta};
use crate::stream_stall::{
    normalize_stream_stall_seconds, should_emit_stall, stream_stall_message,
};
use crate::turn_complete::{
    is_successful_prompt_complete, is_terminal_tool_status, should_defer_prompt_complete,
};

/// Strip bulky MCP/RPC dumps so chat errors stay human-readable.
/// Full stderr is still logged via `tracing` on the ACP client side.
fn sanitize_error_detail(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return s;
    }
    // Drop `; stderr: …` / `stderr: …` tails from format_exit_detail legacy messages.
    if let Some(idx) = s.find("; stderr:") {
        s.truncate(idx);
    } else if let Some(idx) = s.find("stderr:") {
        s.truncate(idx);
    }
    // Strip ANSI SGR if any leaked through.
    let mut cleaned = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        cleaned.push(bytes[i] as char);
        i += 1;
    }
    let s = cleaned.trim().to_string();
    // Compact known host timeouts to a short stable tag (UI maps via code + this).
    let lower = s.to_lowercase();
    if lower.contains("rpc timeout") && lower.contains("session/prompt") {
        return "turn_timeout".into();
    }
    if lower.contains("rpc channel closed") {
        return "agent_disconnected".into();
    }
    // Cap leftover technical lines.
    if s.len() > 160 {
        let mut end = 160;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        return format!("{}…", &s[..end]);
    }
    s
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub session_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub state: SessionState,
    pub last_error: Option<AgentError>,
    pub streaming_message_id: Option<String>,
    pub backend: String,
    pub model_id: Option<String>,
    pub project_path: Option<String>,
    pub title: String,
    pub context_usage: Option<store::SessionTokenUsage>,
    pub sandbox: crate::runtime_compat::SandboxApplicationV1,
    /// Live plus background sessions that are connecting, streaming, or waiting
    /// on permission. The focused `session_id` is still the live slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub busy_session_ids: Vec<String>,
}

/// Versioned send result for Host-owned, visible Memory injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSendResultV2 {
    pub version: u8,
    pub snapshot: SessionSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_injection: Option<crate::memory_injection::MemoryInjectionRecordV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_disclosure: Option<crate::memory_injection::MemoryInjectionDisclosureV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_uses: Vec<crate::skill_feedback::SkillUseRecordV1>,
}

/// One user-prompt checkpoint for the rewind timeline UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindPointDto {
    pub prompt_index: u32,
    pub message_id: Option<String>,
    pub preview: String,
}

/// Result of `session_rewind_execute` — local journal is source of truth for UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindExecuteResult {
    pub snapshot: SessionSnapshot,
    /// False when agent rewind extension failed / unsupported / disconnected.
    pub agent_ok: bool,
    pub agent_error: Option<String>,
    pub local_ok: bool,
    pub kept_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPermissionRequest {
    pub interaction_id: String,
    pub rpc_id: u64,
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub title: String,
    pub preview: String,
    pub scope_key: String,
    pub options: serde_json::Value,
}

/// Stable UI/query payload for a recoverable `_x.ai/ask_user_question`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiAskUserRequest {
    pub interaction_id: String,
    pub rpc_id: u64,
    pub session_id: String,
    pub tool_call_id: Option<String>,
    pub questions: Vec<AskUserQuestionItem>,
    /// Answers already submitted by the UI when the Runtime write failed.
    /// This lets a reloaded WebView restore the draft instead of asking the
    /// user to type it again. It remains in memory only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_answers: Option<serde_json::Value>,
    /// Original Runtime params for forward-compatible clients. Existing UIs
    /// can continue to consume only the normalized fields above.
    pub raw: serde_json::Value,
}

/// Full in-memory reverse-request. This is deliberately not part of the disk
/// schema: it is meaningful only while its owning ACP connection is alive.
#[derive(Debug)]
struct PendingAskUser {
    interaction: InteractionSnapshotV1,
    rpc_id: u64,
    tool_call_id: Option<String>,
    activity_id: String,
    questions: Vec<AskUserQuestionItem>,
    partial_answers: Option<serde_json::Value>,
    raw: serde_json::Value,
    /// Prevent two UI retries from writing two replies concurrently.
    resolving: bool,
    /// In-process Sunsetz kernel wait. ACP asks leave this empty.
    host_reply: Option<tokio::sync::oneshot::Sender<AskUserOutcome>>,
}

impl PendingAskUser {
    fn ui_payload(&self, session_id: &str) -> UiAskUserRequest {
        UiAskUserRequest {
            interaction_id: self.interaction.interaction_id.clone(),
            rpc_id: self.rpc_id,
            session_id: session_id.to_string(),
            tool_call_id: self.tool_call_id.clone(),
            questions: self.questions.clone(),
            partial_answers: self.partial_answers.clone(),
            raw: self.raw.clone(),
        }
    }
}

struct PendingPermission {
    interaction: InteractionSnapshotV1,
    host_reply: Option<tokio::sync::oneshot::Sender<PermissionOutcome>>,
}

impl PendingPermission {
    fn ui_payload(&self) -> UiPermissionRequest {
        let InteractionPayloadV1::Permission {
            tool_name,
            title,
            preview,
            scope_key,
            options,
        } = &self.interaction.payload
        else {
            unreachable!("pending permission payload kind")
        };
        UiPermissionRequest {
            interaction_id: self.interaction.interaction_id.clone(),
            rpc_id: self.interaction.rpc_id,
            session_id: self.interaction.session_id.clone(),
            tool_call_id: self.interaction.tool_call_id.clone().unwrap_or_default(),
            tool_name: tool_name.clone(),
            title: title.clone(),
            preview: preview.clone(),
            scope_key: scope_key.clone(),
            options: options.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct PendingPlan {
    interaction: InteractionSnapshotV1,
}

fn ask_user_activity_id(tool_call_id: Option<&str>, rpc_id: u64) -> String {
    tool_call_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("ask-user-{rpc_id}"))
}

fn ask_user_activity_title(
    status: &str,
    question_count: usize,
    answered_count: Option<usize>,
) -> String {
    let question_label = if question_count == 1 {
        "question"
    } else {
        "questions"
    };
    match status {
        "completed" => {
            let answered = answered_count.unwrap_or(0).min(question_count);
            let skipped = question_count.saturating_sub(answered);
            format!(
                "Asked {question_count} {question_label} · {answered} answered · {skipped} skipped"
            )
        }
        "cancelled" => {
            format!("Questionnaire cancelled · {question_count} {question_label}")
        }
        "failed" | "error" => {
            format!("Questionnaire interrupted · {question_count} {question_label}")
        }
        _ => format!("Asking {question_count} {question_label}"),
    }
}

fn answered_question_count(answers: &serde_json::Value, question_count: usize) -> usize {
    answers
        .as_object()
        .map(|values| {
            values
                .keys()
                .filter(|key| !key.trim().is_empty())
                .count()
                .min(question_count)
        })
        .unwrap_or(0)
}

fn claim_pending_ask(
    pending: &mut PendingAskUser,
    requested_interaction_id: Option<&str>,
    requested_rpc_id: Option<u64>,
) -> Result<u64, String> {
    if pending.resolving {
        return Err("ask_user_question is already resolving".into());
    }
    pending
        .interaction
        .claim(requested_interaction_id, requested_rpc_id)?;
    pending.resolving = true;
    Ok(pending.rpc_id)
}

fn restore_pending_ask_after_failure(
    session: &mut LiveSession,
    process_id: &str,
    rpc_id: u64,
) -> bool {
    if session.process_id != process_id {
        return false;
    }
    let Some(pending) = session.pending_ask_user.as_mut() else {
        return false;
    };
    if pending.rpc_id != rpc_id || !pending.resolving {
        return false;
    }
    if !pending.interaction.restore_pending() {
        return false;
    }
    pending.resolving = false;
    true
}

fn clear_pending_ask_after_success(
    session: &mut LiveSession,
    process_id: &str,
    rpc_id: u64,
) -> bool {
    if session.process_id != process_id {
        return false;
    }
    let matches = session
        .pending_ask_user
        .as_ref()
        .is_some_and(|pending| pending.rpc_id == rpc_id && pending.resolving);
    if matches {
        if let Some(pending) = session.pending_ask_user.as_mut() {
            if !pending.interaction.resolve() {
                return false;
            }
        }
        session.pending_ask_user = None;
    }
    matches
}

/// Identity for routing ACP event pumps when multiple processes are warm.
type ProcessId = String;

struct LiveSession {
    app_session_id: String,
    /// Stable id for the agent process / event pump (not the App session id).
    process_id: ProcessId,
    meta: SessionMeta,
    fsm: SessionFsm,
    backend: String,
    acp: Option<Arc<AcpClient>>,
    mock_stream: Option<MockStreamHandle>,
    /// Cancels the in-process Sunsetz agent turn.
    agent_cancel: Option<Arc<AtomicBool>>,
    /// Id of the current parent kernel turn; subagents spawned here are cancelled with Stop.
    host_turn_id: Option<String>,
    streaming_message_id: Option<String>,
    /// Accumulated assistant text for current turn (persisted on complete).
    stream_buf: String,
    stream_thought: String,
    /// Last emitted chunk was assistant body — next thought opens a new phase
    /// so thinking and body can interleave (think → write → think → write).
    stream_last_was_assistant: bool,
    /// Host-created phases after a tool boundary keep their UUID even when the
    /// Runtime continues to reuse one messageId for the whole turn.
    stream_phase_id_locked: bool,
    /// Image/file paths produced this turn (image_gen / image_edit).
    stream_attachments: Vec<MessageAttachmentStored>,
    model_id: Option<String>,
    /// Effort applied to the live agent process (from last spawn).
    effort: Option<String>,
    /// Product mode: agent | plan | ask (ACP session/set_mode).
    product_mode: Option<String>,
    project_path: Option<String>,
    allow_cache: SessionAllowCache,
    policy: PermissionPolicy,
    /// Last provider retry attempt observed this turn (0 = none).
    provider_retry_attempt: u32,
    /// Host already aborted this turn after max retries (avoid double cancel).
    provider_retry_aborted: bool,
    /// After session/new (load failed), first prompt should carry journal history.
    needs_history_bootstrap: bool,
    /// Pending `_x.ai/exit_plan_mode` JSON-RPC id awaiting user Approve / revise.
    pending_plan: Option<PendingPlan>,
    /// Pending `session/request_permission` awaiting a Host/UI response.
    pending_permission: Option<PendingPermission>,
    /// Synthetic JSON-RPC ids for in-process Host kernel permission waits.
    host_rpc_seq: u64,
    /// Pending `_x.ai/ask_user_question` payload awaiting user answers.
    pending_ask_user: Option<PendingAskUser>,
    /// Last user/agent activity (send, stream, permission, connect).
    last_activity: Instant,
    /// Last stream chunk or tool event (I06 stall watchdog). Permission waits do not update this.
    last_stream_progress: Instant,
    /// Last time we emitted `session://stream_stall` for the current silence window.
    last_stall_emit: Option<Instant>,
    /// Throttle mid-stream assistant journal upserts (I04).
    journal_throttle: JournalWriteThrottle,
    /// Tool calls still pending/in_progress this turn (#52 early prompt_complete).
    open_tool_ids: HashSet<String>,
    /// Every tool id already observed this turn. Unlike `open_tool_ids`, ids
    /// remain here after completion so updates never create a second boundary.
    seen_tool_ids: HashSet<String>,
    /// `prompt_complete` arrived while tools/gates still open; finish when clear.
    deferred_prompt_complete: Option<String>,
    /// Tool events observed during the current turn (empty-run soft signal).
    tools_this_turn: u32,
    /// Metadata-only Skill evidence for the active turn. Bodies, prompts and
    /// tool payloads never enter this ledger.
    active_skill_uses: Vec<crate::skill_feedback::SkillUseRecordV1>,
    pending_skill_settlement: Option<crate::skill_feedback::SkillUseStatusV1>,
    /// Successful parent PromptComplete may auto-start a wake turn.
    /// Stop / Steer and a user `begin_stream` clear this so queued send wins.
    allow_auto_wake: bool,
}

struct WakeInspect {
    backend: String,
    streaming: bool,
    deferred_prompt_complete: bool,
    awaiting_permission: bool,
    allow_auto_wake: bool,
    host_turn_id: Option<String>,
    process_id: String,
    model_id: Option<String>,
    effort: Option<String>,
    project_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingAskActivity {
    session_id: String,
    activity_id: String,
    question_count: usize,
}

fn take_pending_ask_activity(session: &mut LiveSession) -> Option<PendingAskActivity> {
    let pending = session.pending_ask_user.take()?;
    Some(PendingAskActivity {
        session_id: session.app_session_id.clone(),
        activity_id: pending.activity_id,
        question_count: pending.questions.len(),
    })
}

struct InterruptedSessionGates {
    interactions: Vec<InteractionSnapshotV1>,
    plan_artifacts: Vec<crate::plan_artifacts::PlanArtifactV1>,
}

impl InterruptedSessionGates {
    fn empty() -> Self {
        Self {
            interactions: Vec::new(),
            plan_artifacts: Vec::new(),
        }
    }
}

fn interrupt_pending_interactions(session: &mut LiveSession) -> InterruptedSessionGates {
    let mut interactions = Vec::new();
    if let Some(mut pending) = session.pending_permission.take() {
        pending
            .interaction
            .set_status(InteractionStatusV1::Interrupted);
        interactions.push(pending.interaction);
    }
    if let Some(mut pending) = session.pending_plan.take() {
        pending
            .interaction
            .set_status(InteractionStatusV1::Interrupted);
        interactions.push(pending.interaction);
    }
    if let Some(pending) = session.pending_ask_user.as_mut() {
        pending
            .interaction
            .set_status(InteractionStatusV1::Interrupted);
        interactions.push(pending.interaction.clone());
        if let Some(tx) = pending.host_reply.take() {
            let _ = tx.send(AskUserOutcome::Cancelled);
        }
    }
    let plan_artifacts = match crate::plan_artifacts::interrupt_process(
        &session.app_session_id,
        &session.process_id,
    ) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            tracing::warn!(
                session_id = %session.app_session_id,
                process_id = %session.process_id,
                "interrupt durable Plan artifacts failed: {error}"
            );
            Vec::new()
        }
    };
    InterruptedSessionGates {
        interactions,
        plan_artifacts,
    }
}

/// Ready agent process parked while another App session is focused (I01/I02).
struct ParkedAgent {
    process_id: ProcessId,
    app_session_id: String,
    meta: SessionMeta,
    acp: Arc<AcpClient>,
    last_activity: Instant,
    model_id: Option<String>,
    effort: Option<String>,
    product_mode: Option<String>,
    project_path: Option<String>,
    policy: PermissionPolicy,
    needs_history_bootstrap: bool,
    backend: String,
}

/// How many journal messages (user+assistant) to carry when session/load fails.
const HISTORY_BOOTSTRAP_MAX_MSGS: usize = 16;
/// Cap each message body in the bootstrap block.
const HISTORY_BOOTSTRAP_PER_MSG_CHARS: usize = 2_000;
/// Cap total bootstrap text (excluding the new user turn).
const HISTORY_BOOTSTRAP_MAX_CHARS: usize = 14_000;

fn build_runtime_context_usage(
    meta: &store::SessionMeta,
    backend: &str,
    turn_input_tokens: u64,
    turn_output_tokens: u64,
    turn_cached_read_tokens: u64,
    turn_reasoning_tokens: u64,
    model_calls: u32,
    reported_model_id: Option<String>,
) -> Option<store::SessionTokenUsage> {
    let exact = if agent_loop::is_sunsetz_backend(backend) {
        if turn_input_tokens == 0 && turn_output_tokens == 0 {
            return None;
        }
        Some(crate::context_usage::InferenceUsage {
            input_tokens: turn_input_tokens,
            output_tokens: turn_output_tokens,
            cached_read_tokens: turn_cached_read_tokens,
            reasoning_tokens: turn_reasoning_tokens,
        })
    } else {
        let settings = store::load_settings();
        let agent_home = crate::paths::resolve_agent_grok_home(&settings.session_data_mode);
        meta.agent_session_id
            .as_deref()
            .and_then(|sid| crate::context_usage::latest_inference_usage(&agent_home, sid))
            .or_else(|| {
                // A single-call turn's aggregate is also the exact final inference.
                if model_calls <= 1 {
                    Some(crate::context_usage::InferenceUsage {
                        input_tokens: turn_input_tokens,
                        output_tokens: turn_output_tokens,
                        cached_read_tokens: turn_cached_read_tokens,
                        reasoning_tokens: turn_reasoning_tokens,
                    })
                } else {
                    None
                }
            })
    }?;

    let model_id = reported_model_id
        .filter(|id| !id.trim().is_empty())
        .or_else(|| meta.model_id.clone())
        .unwrap_or_else(|| "unknown".into());
    Some(store::SessionTokenUsage {
        used_tokens: exact.input_tokens.saturating_add(exact.output_tokens),
        input_tokens: exact.input_tokens,
        output_tokens: exact.output_tokens,
        cached_read_tokens: exact.cached_read_tokens,
        reasoning_tokens: exact.reasoning_tokens,
        turn_input_tokens,
        turn_output_tokens,
        model_calls,
        context_window_tokens: crate::context_usage::context_window_tokens(&model_id),
        model_id,
        updated_at: chrono::Utc::now(),
        source: "runtime".into(),
    })
}

enum CompactGate {
    Continue,
    Finished,
    Failed,
}

/// Compact the built-in kernel window before the parent `run_turn`.
async fn apply_sunsetz_compact(
    cfg: &mut agent_loop::AgentTurnConfig,
    session_id: &str,
    last_usage: Option<store::SessionTokenUsage>,
    tx: &tokio::sync::mpsc::UnboundedSender<AcpEvent>,
) -> CompactGate {
    use crate::context_compact::{self, CompactError};

    let journal = store::load_messages(session_id);
    let mut artifact = match context_compact::load_v1(session_id) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!("load context compact sidecar session={session_id}: {error}");
            None
        }
    };
    if artifact
        .as_ref()
        .is_some_and(|existing| !context_compact::artifact_applies(&journal, existing))
    {
        if let Err(error) = context_compact::delete_v1(session_id) {
            tracing::warn!("drop stale compact sidecar session={session_id}: {error}");
        }
        artifact = None;
    }
    let last_user = journal
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone());
    let compact_cmd = last_user
        .as_deref()
        .and_then(context_compact::parse_manual_command);
    let auto = compact_cmd.is_none()
        && context_compact::should_auto_compact(&journal, artifact.as_ref(), last_usage.as_ref());
    if compact_cmd.is_none() && !auto {
        cfg.history = context_compact::history_for_model(&journal, artifact.as_ref());
        return CompactGate::Continue;
    }

    let trigger = if compact_cmd.is_some() {
        "manual"
    } else {
        "auto"
    };
    let note = compact_cmd
        .as_ref()
        .and_then(|command| command.note.clone());
    let stop = Arc::clone(&cfg.stop);
    let client = cfg.client.clone();
    let endpoint = cfg.endpoint.clone();
    let model_id = cfg.endpoint.model.clone();
    let result = context_compact::run_compact(
        &journal,
        artifact.as_ref(),
        note.as_deref(),
        trigger,
        last_usage.as_ref(),
        Some(model_id.as_str()),
        |messages| {
            let stop = Arc::clone(&stop);
            let client = client.clone();
            let endpoint = endpoint.clone();
            async move {
                if stop.load(Ordering::SeqCst) {
                    return Err(CompactError::Cancelled);
                }
                match agent_loop::complete_text(&client, &endpoint, &messages, &stop).await {
                    Ok(text) => Ok(text),
                    Err(error) => {
                        if stop.load(Ordering::SeqCst)
                            || error.message.contains("compact cancelled")
                        {
                            Err(CompactError::Cancelled)
                        } else {
                            Err(CompactError::Provider(error.message))
                        }
                    }
                }
            }
        },
    )
    .await;

    match result {
        Ok(new_artifact) => {
            if let Err(error) = context_compact::save_v1(session_id, &new_artifact) {
                tracing::warn!("persist context compact session={session_id}: {error}");
                if compact_cmd.is_some() {
                    let _ = tx.send(AcpEvent::Error {
                        error: AgentError::new(
                            AgentErrorCode::AgentCrashed,
                            format!("persist compact: {error}"),
                        ),
                    });
                    return CompactGate::Failed;
                }
            }
            let _ = tx.send(AcpEvent::ContextCompact {
                trigger: new_artifact.trigger.clone(),
                tokens_before: new_artifact.tokens_before,
                tokens_after: new_artifact.tokens_after,
                summary_preview: context_compact::summary_preview(&new_artifact.summary),
                note: new_artifact.note.clone(),
            });
            if compact_cmd.is_some() {
                let _ = tx.send(AcpEvent::PromptComplete {
                    stop_reason: "end_turn".into(),
                });
                return CompactGate::Finished;
            }
            cfg.history = context_compact::history_for_model(&journal, Some(&new_artifact));
            CompactGate::Continue
        }
        Err(CompactError::NothingToCompact) => {
            if compact_cmd.is_some() {
                let _ = tx.send(AcpEvent::ContextCompact {
                    trigger: "manual".into(),
                    tokens_before: last_usage.as_ref().map(|usage| usage.used_tokens),
                    tokens_after: None,
                    summary_preview: None,
                    note,
                });
                let _ = tx.send(AcpEvent::PromptComplete {
                    stop_reason: "end_turn".into(),
                });
                return CompactGate::Finished;
            }
            cfg.history = context_compact::history_for_model(&journal, artifact.as_ref());
            CompactGate::Continue
        }
        Err(CompactError::Cancelled) => CompactGate::Finished,
        Err(error) if compact_cmd.is_some() => {
            let message = match error {
                CompactError::EmptySummary => "compact produced an empty summary".into(),
                CompactError::Provider(message) => message,
                CompactError::NothingToCompact | CompactError::Cancelled => "compact failed".into(),
            };
            let _ = tx.send(AcpEvent::Error {
                error: AgentError::new(AgentErrorCode::NetworkProvider, message),
            });
            CompactGate::Failed
        }
        Err(_) => {
            tracing::warn!("auto compact failed session={session_id}; using truncated history");
            cfg.history = agent_loop::chat_history_from_journal(&journal);
            CompactGate::Continue
        }
    }
}

/// Build a continuity preamble from App journal when agent session is new.
/// Keeps recent turns so the model still "remembers" the chat after respawn.
fn build_history_bootstrap(app_session_id: &str) -> Option<String> {
    let msgs = store::load_messages(app_session_id);
    // Take last N non-empty user/assistant turns (errors abbreviated).
    let mut picked: Vec<&store::ChatMessageStored> = Vec::new();
    for m in msgs.iter().rev() {
        if m.role != "user" && m.role != "assistant" {
            continue;
        }
        if m.content.trim().is_empty() {
            continue;
        }
        picked.push(m);
        if picked.len() >= HISTORY_BOOTSTRAP_MAX_MSGS {
            break;
        }
    }
    if picked.is_empty() {
        return None;
    }
    picked.reverse();

    let mut body = String::from(
        "[Prior conversation context — this chat continues an existing Sunsetz session. \
The agent process was restarted; use the following transcript for continuity ONLY. \
Rules: do NOT re-greet; do NOT restate, quote, or re-answer prior assistant turns; \
do NOT reprint the transcript in your reply; answer ONLY the new user message below.]\n\n",
    );
    let header_len = body.len();

    for m in picked {
        let role = if m.role == "user" {
            "User"
        } else if m.is_error {
            "Assistant (error)"
        } else {
            "Assistant"
        };
        let mut content = m.content.trim().to_string();
        // Soft-trim huge tool dumps / tables for bootstrap.
        if content.len() > HISTORY_BOOTSTRAP_PER_MSG_CHARS {
            let keep = HISTORY_BOOTSTRAP_PER_MSG_CHARS.saturating_sub(40);
            content = format!(
                "{}…\n[truncated {} chars]",
                content.chars().take(keep).collect::<String>(),
                m.content.len()
            );
        }
        let block = format!("### {role}\n{content}\n\n");
        if body.len() - header_len + block.len() > HISTORY_BOOTSTRAP_MAX_CHARS {
            body.push_str("### …\n[earlier turns omitted for length]\n\n");
            break;
        }
        body.push_str(&block);
    }
    body.push_str("---\n\n[End of prior context. Continue with the user's new message below.]\n");
    Some(body)
}

fn is_agent_directive_line(line: &str) -> bool {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    !tokens.is_empty()
        && tokens.iter().all(|token| {
            token.starts_with('/')
                && token.len() > 1
                && token[1..].bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')
                })
        })
}

fn next_prompt_line(prompt: &str, start: usize) -> (&str, usize) {
    let rest = &prompt[start..];
    match rest.find('\n') {
        Some(index) => (&rest[..index], start + index + 1),
        None => (rest, prompt.len()),
    }
}

fn leading_agent_directive_end(prompt: &str) -> usize {
    let (first, first_end) = next_prompt_line(prompt, 0);
    if first.trim().eq_ignore_ascii_case("/goal") {
        if first_end < prompt.len() {
            let (second, second_end) = next_prompt_line(prompt, first_end);
            if is_agent_directive_line(second.trim()) {
                return second_end;
            }
        }
        return first_end;
    }
    is_agent_directive_line(first.trim())
        .then_some(first_end)
        .unwrap_or(0)
}

/// Runtime slash directives must remain the first prompt lines. Host-owned
/// context belongs to the Skill's input, not in front of its invocation.
fn prepend_host_context_preserving_directives(prompt: &str, context: &str) -> String {
    let context = context.trim_end();
    if context.is_empty() {
        return prompt.to_string();
    }
    let directive_end = leading_agent_directive_end(prompt);
    if directive_end == 0 {
        return format!("{context}\n{prompt}");
    }
    let (directives, body) = prompt.split_at(directive_end);
    if directives.ends_with('\n') {
        format!("{directives}{context}\n{body}")
    } else if body.is_empty() {
        format!("{directives}\n{context}")
    } else {
        format!("{directives}\n{context}\n{body}")
    }
}

/// Cap content snippets emitted on live tool events (diff panel).
const TOOL_CONTENT_SNIPPET_MAX: usize = 200_000;

/// Extract human-visible path + detail from tool_call payload for activity UI.
fn extract_tool_ui_fields(raw: &serde_json::Value) -> (Option<String>, Option<String>) {
    let path = raw
        .pointer("/locations/0/path")
        .or_else(|| raw.pointer("/rawInput/path"))
        .or_else(|| raw.pointer("/rawInput/file_path"))
        .or_else(|| raw.pointer("/rawInput/filePath"))
        .or_else(|| raw.pointer("/rawInput/target_file"))
        .or_else(|| raw.pointer("/rawInput/targetFile"))
        .or_else(|| raw.pointer("/rawInput/id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let command = raw
        .pointer("/rawInput/command")
        .or_else(|| raw.pointer("/rawInput/cmd"))
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(240).collect::<String>());
    let detail = command.or_else(|| {
        raw.pointer("/rawInput/query")
            .or_else(|| raw.pointer("/rawInput/pattern"))
            .or_else(|| raw.pointer("/rawInput/description"))
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(240).collect::<String>())
    });
    (detail, path)
}

fn take_tool_content_str(v: Option<&serde_json::Value>) -> Option<String> {
    let s = v.and_then(|x| x.as_str())?;
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(TOOL_CONTENT_SNIPPET_MAX).collect())
}

/// Optional before/after text for the session diff panel (from rawInput when present).
/// - str_replace / search_replace: old_string → before, new_string → after
/// - write / create_file: contents → after
fn extract_tool_content_snippets(raw: &serde_json::Value) -> (Option<String>, Option<String>) {
    let before = take_tool_content_str(
        raw.pointer("/rawInput/old_string")
            .or_else(|| raw.pointer("/rawInput/oldString"))
            .or_else(|| raw.pointer("/rawInput/old_str"))
            .or_else(|| raw.pointer("/rawInput/previous"))
            .or_else(|| raw.pointer("/rawInput/before")),
    );
    let after = take_tool_content_str(
        raw.pointer("/rawInput/new_string")
            .or_else(|| raw.pointer("/rawInput/newString"))
            .or_else(|| raw.pointer("/rawInput/new_str"))
            .or_else(|| raw.pointer("/rawInput/contents"))
            .or_else(|| raw.pointer("/rawInput/content"))
            .or_else(|| raw.pointer("/rawInput/new_contents"))
            .or_else(|| raw.pointer("/rawInput/after")),
    );
    (before, after)
}

/// When user asks to open a Sunsetz / foreign agent session by UUID, steer tools.
fn session_lookup_host_hint(user_text: &str) -> Option<String> {
    let t = user_text.trim();
    // UUID v4-ish
    let uuid_re = regex_is_session_uuid(t);
    if !uuid_re {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    let asks = lower.contains("会话")
        || lower.contains("session")
        || lower.contains("上下文")
        || lower.contains("继续")
        || lower.contains("resume")
        || lower.contains("复述")
        || lower.contains("历史");
    if !asks {
        return None;
    }
    Some(
        "[Host hint — session lookup]\n\
This looks like a request to read a **Sunsetz / agent session** by UUID.\n\
Do **not** scan the whole home directory or assume Claude/Codex/Cursor storage first.\n\
Prefer, in order:\n\
1. Sunsetz journal: `~/Library/Application Support/dev.sunsetz.desktop/sessions/<id>/messages.json` \
(and `sessions_index.json` for meta).\n\
2. Runtime agent-home: `…/dev.sunsetz.desktop/agent-home/sessions/<encoded-cwd>/<agentSessionId>/` \
(chat_history.jsonl, updates.jsonl) — map app session id via sessions_index.agentSessionId.\n\
3. Only if missing there, try Claude/Codex/Cursor resume paths with a **narrow** query.\n\
Avoid unbounded `find ~` / multi-GB scans; use index files and known roots.\n\
[/Host hint]\n"
            .to_string(),
    )
}

fn regex_is_session_uuid(text: &str) -> bool {
    // Match standard UUID anywhere in the message.
    let bytes = text.as_bytes();
    // Simple scan for 8-4-4-4-12 hex pattern
    let s = text;
    let mut i = 0;
    let chars: Vec<char> = s.chars().collect();
    while i + 36 <= chars.len() {
        let slice: String = chars[i..i + 36].iter().collect();
        if is_uuid_str(&slice) {
            return true;
        }
        i += 1;
    }
    let _ = bytes;
    false
}

fn is_uuid_str(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let b = s.as_bytes();
    let hex = |c: u8| c.is_ascii_hexdigit();
    for (i, &c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != b'-' {
                    return false;
                }
            }
            _ => {
                if !hex(c) {
                    return false;
                }
            }
        }
    }
    true
}

/// Pull absolute media path from ACP tool_call / tool_call_update payload
/// (image_gen, image_edit, image_to_video, reference_to_video, …).
fn extract_generated_media_path(raw: &serde_json::Value) -> Option<String> {
    // ImageGen / ImageEdit / video tools rawOutput
    if let Some(path) = raw
        .pointer("/rawOutput/path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(path.to_string());
    }
    // Nested under toolCall (some hosts wrap)
    if let Some(path) = raw
        .pointer("/toolCall/rawOutput/path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(path.to_string());
    }
    // content[].content.text is often a JSON string with {"path":"..."}
    if let Some(arr) = raw.get("content").and_then(|v| v.as_array()) {
        for item in arr {
            let text = item
                .pointer("/content/text")
                .or_else(|| item.get("text"))
                .and_then(|v| v.as_str());
            if let Some(t) = text {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(t) {
                    if let Some(path) = j.get("path").and_then(|v| v.as_str()) {
                        if !path.is_empty() {
                            return Some(path.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn is_image_fs_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".heic", ".avif",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

fn is_video_fs_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".mp4", ".webm", ".mov", ".mkv", ".m4v", ".avi", ".ogv", ".mpeg", ".mpg",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

fn is_media_fs_path(path: &str) -> bool {
    is_image_fs_path(path) || is_video_fs_path(path)
}

fn attachment_from_path(path: &str) -> MessageAttachmentStored {
    // `Path::file_name` follows the host platform. Split both separators so a
    // Windows absolute path also has a useful name when a journal is inspected
    // or migrated on macOS/Linux.
    let name = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string();
    MessageAttachmentStored {
        path: path.to_string(),
        name,
        is_dir: std::fs::metadata(path)
            .map(|meta| meta.is_dir())
            .unwrap_or(false),
    }
}

fn is_absolute_attachment_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let unix_absolute = path.starts_with('/');
    let windows_drive_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let windows_unc_absolute = path
        .strip_prefix(r"\\")
        .map(|tail| {
            let mut parts = tail.split('\\').filter(|part| !part.is_empty());
            parts.next().is_some() && parts.next().is_some()
        })
        .unwrap_or(false);
    unix_absolute || windows_drive_absolute || windows_unc_absolute
}

fn absolute_attachment_ref(line: &str) -> Option<&str> {
    let path = line.trim().strip_prefix('@')?.trim();
    is_absolute_attachment_path(path).then_some(path)
}

fn normalize_explicit_attachments(
    attachments: Vec<MessageAttachmentStored>,
) -> Result<Vec<MessageAttachmentStored>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        if attachment.path.is_empty() || attachment.path.contains('\0') {
            return Err("attachment path is empty or invalid".into());
        }
        if !is_absolute_attachment_path(&attachment.path) {
            return Err(format!(
                "attachment path must be absolute: {}",
                attachment.path
            ));
        }
        if attachment.name.trim().is_empty() || attachment.name.contains('\0') {
            return Err("attachment name is empty or invalid".into());
        }
        if seen.insert(attachment.path.clone()) {
            normalized.push(attachment);
        }
    }
    Ok(normalized)
}

/// Recover the attachment block appended by the existing frontend
/// `buildAgentPrompt` contract. Only consecutive absolute `@path` lines at the
/// tail are considered, so ordinary mentions in the user's prose stay prose.
fn attachments_from_agent_text(text: &str) -> Vec<MessageAttachmentStored> {
    let mut reversed = Vec::new();
    let mut saw_attachment = false;
    for line in text.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() && saw_attachment {
            break;
        }
        let Some(path) = absolute_attachment_ref(trimmed) else {
            break;
        };
        saw_attachment = true;
        reversed.push(attachment_from_path(path));
    }
    reversed.reverse();
    let mut seen = HashSet::new();
    reversed
        .into_iter()
        .filter(|attachment| seen.insert(attachment.path.clone()))
        .collect()
}

fn user_journal_message(
    id: String,
    journal_content: String,
    agent_text: &str,
    explicit_attachments: Option<Vec<MessageAttachmentStored>>,
) -> ChatMessageStored {
    let attachments =
        explicit_attachments.unwrap_or_else(|| attachments_from_agent_text(agent_text));
    ChatMessageStored {
        id,
        role: "user".into(),
        content: journal_content,
        thought: None,
        created_at: chrono::Utc::now(),
        is_error: false,
        attachments: (!attachments.is_empty()).then_some(attachments),
        marker: None,
    }
}

fn tool_step_content(
    status: &str,
    kind: &str,
    title: &str,
    detail: Option<&str>,
    path: Option<&str>,
) -> String {
    let label = if !title.is_empty() {
        title
    } else if !kind.is_empty() {
        kind
    } else {
        "tool"
    };
    let mut content = format!("tool_step|{status}|{kind}|{label}");
    let detail = detail.filter(|value| !value.is_empty());
    let path = path.filter(|value| !value.is_empty());
    if let Some(detail) = detail {
        content.push('\n');
        content.push_str(&detail.chars().take(400).collect::<String>());
    }
    if let Some(path) = path {
        // Preserve the stable `header\ndetail\npath` shape even when detail is
        // absent; frontend reload parsers treat the second line as detail.
        if detail.is_none() {
            content.push('\n');
        }
        content.push('\n');
        content.push_str(path);
    }
    content
}

/// Insert a tool row at first observation and update that exact slot later.
/// Keeping the original `created_at` and vector index is what preserves the
/// assistant → tool → assistant timeline without changing the journal schema.
fn upsert_tool_step_message(
    messages: &mut Vec<ChatMessageStored>,
    tool_call_id: &str,
    status: &str,
    kind: &str,
    title: &str,
    detail: Option<&str>,
    path: Option<&str>,
) {
    let id = format!("tool-{tool_call_id}");
    let is_error = matches!(status, "failed" | "error");
    if let Some(slot) = messages.iter_mut().find(|message| message.id == id) {
        let mut parts = slot.content.split('\n');
        let header = parts.next().unwrap_or_default();
        let header_parts = header.split('|').collect::<Vec<_>>();
        let old_kind = header_parts.get(2).copied().unwrap_or_default();
        let old_title = if header_parts.len() > 3 {
            header_parts[3..].join("|")
        } else {
            String::new()
        };
        let old_detail = parts.next().filter(|value| !value.is_empty());
        let old_path = parts.next().filter(|value| !value.is_empty());
        let merged_kind = if kind.is_empty() { old_kind } else { kind };
        let merged_title = if title.is_empty() {
            old_title.as_str()
        } else {
            title
        };
        let content = tool_step_content(
            status,
            merged_kind,
            merged_title,
            detail.or(old_detail),
            path.or(old_path),
        );
        slot.role = "tool".into();
        slot.content = content;
        slot.is_error = is_error;
        slot.marker = Some("tool_step".into());
        return;
    }
    let content = tool_step_content(status, kind, title, detail, path);
    messages.push(ChatMessageStored {
        id,
        role: "tool".into(),
        content,
        thought: None,
        created_at: chrono::Utc::now(),
        is_error,
        attachments: None,
        marker: Some("tool_step".into()),
    });
}

pub(super) fn persist_tool_step(
    session_id: &str,
    tool_call_id: &str,
    status: &str,
    kind: &str,
    title: &str,
    detail: Option<&str>,
    path: Option<&str>,
) {
    if session_id.is_empty() || tool_call_id.is_empty() {
        return;
    }
    if let Err(error) = store::update_messages(session_id, |messages| {
        upsert_tool_step_message(messages, tool_call_id, status, kind, title, detail, path);
        Ok(())
    }) {
        tracing::warn!(
            "persist tool activity failed session={session_id} tool={tool_call_id}: {error}"
        );
    }
}

fn context_compact_content(
    trigger: &str,
    tokens_before: Option<u64>,
    tokens_after: Option<u64>,
    summary_preview: Option<&str>,
    note: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if trigger == "manual" {
        parts.push("manual".to_string());
    } else {
        parts.push("auto".to_string());
    }
    if let (Some(before), Some(after)) = (tokens_before, tokens_after) {
        parts.push(format!("tokens:{before}->{after}"));
    } else if let Some(before) = tokens_before {
        parts.push(format!("tokens_before:{before}"));
    } else if let Some(after) = tokens_after {
        parts.push(format!("tokens_after:{after}"));
    }
    if let Some(note) = note.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("note:{note}"));
    }

    // Machine-readable first line; localized human copy belongs to the UI.
    let mut content = format!("context_compact|{}", parts.join("|"));
    if let Some(summary) = summary_preview
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        content.push('\n');
        content.push_str(summary);
    }
    content
}

fn upsert_context_compact_message(
    messages: &mut Vec<ChatMessageStored>,
    message_id: &str,
    content: &str,
) {
    if let Some(slot) = messages.iter_mut().find(|message| message.id == message_id) {
        slot.role = "tool".into();
        slot.content = content.into();
        slot.thought = None;
        slot.is_error = false;
        slot.attachments = None;
        slot.marker = Some("context_compact".into());
        return;
    }
    messages.push(ChatMessageStored {
        id: message_id.into(),
        role: "tool".into(),
        content: content.into(),
        thought: None,
        created_at: chrono::Utc::now(),
        is_error: false,
        attachments: None,
        marker: Some("context_compact".into()),
    });
}

fn persist_context_compact(session_id: &str, message_id: &str, content: &str) {
    if session_id.is_empty() || message_id.is_empty() {
        return;
    }
    if let Err(error) = store::update_messages(session_id, |messages| {
        upsert_context_compact_message(messages, message_id, content);
        Ok(())
    }) {
        tracing::warn!(
            "persist context compact failed session={session_id} message={message_id}: {error}"
        );
    }
}

fn upsert_ask_user_activity_message(
    messages: &mut Vec<ChatMessageStored>,
    activity_id: &str,
    status: &str,
    question_count: usize,
    answered_count: Option<usize>,
) {
    let title = ask_user_activity_title(status, question_count, answered_count);
    upsert_tool_step_message(
        messages,
        activity_id,
        status,
        "ask_user",
        &title,
        None,
        None,
    );
}

fn record_ask_user_activity(
    app: &AppHandle,
    session_id: &str,
    activity_id: &str,
    status: &str,
    question_count: usize,
    answered_count: Option<usize>,
) {
    if session_id.is_empty() || activity_id.is_empty() {
        return;
    }
    let title = ask_user_activity_title(status, question_count, answered_count);
    if let Err(error) = store::update_messages(session_id, |messages| {
        upsert_ask_user_activity_message(
            messages,
            activity_id,
            status,
            question_count,
            answered_count,
        );
        Ok(())
    }) {
        tracing::warn!(
            "persist ask_user activity failed session={session_id} activity={activity_id}: {error}"
        );
    }
    let _ = app.emit(
        "session://tool",
        serde_json::json!({
            "sessionId": session_id,
            "toolCallId": activity_id,
            "title": title,
            "kind": "ask_user",
            "status": status,
        }),
    );
}

fn memory_injection_mutation_request(
    prepared: &crate::memory_injection::MemoryInjectionPreparedV1,
) -> crate::memory_injection::MemoryInjectionMutationRequestV1 {
    crate::memory_injection::MemoryInjectionMutationRequestV1 {
        version: crate::memory_injection::MEMORY_INJECTION_VERSION,
        session_id: prepared.record.session_id.clone(),
        injection_id: prepared.record.injection_id.clone(),
        expected_context_hash: prepared.record.context_hash.clone(),
        expected_revision: prepared.record.revision,
    }
}

fn memory_injection_failed_request(
    prepared: &crate::memory_injection::MemoryInjectionPreparedV1,
    failure_code: crate::memory_injection::MemoryInjectionFailureCodeV1,
) -> crate::memory_injection::MemoryInjectionFailedRequestV1 {
    crate::memory_injection::MemoryInjectionFailedRequestV1 {
        version: crate::memory_injection::MEMORY_INJECTION_VERSION,
        session_id: prepared.record.session_id.clone(),
        injection_id: prepared.record.injection_id.clone(),
        expected_context_hash: prepared.record.context_hash.clone(),
        expected_revision: prepared.record.revision,
        failure_code,
    }
}

fn memory_injection_marker_message(
    prepared: &crate::memory_injection::MemoryInjectionPreparedV1,
) -> ChatMessageStored {
    ChatMessageStored {
        id: format!("memory-injection-{}", prepared.record.injection_id),
        role: "tool".into(),
        content: serde_json::json!({
            "version": crate::memory_injection::MEMORY_INJECTION_VERSION,
            "injectionId": prepared.record.injection_id,
            "contextHash": prepared.record.context_hash,
            "reviewedMemoryCount": prepared.disclosure.items.len(),
            "contextOnlyNotInstructions": true,
        })
        .to_string(),
        thought: None,
        created_at: chrono::Utc::now(),
        is_error: false,
        attachments: None,
        marker: Some("memory_injection".into()),
    }
}

fn skill_use_transition_requests(
    records: &[crate::skill_feedback::SkillUseRecordV1],
    next_status: crate::skill_feedback::SkillUseStatusV1,
) -> Vec<crate::skill_feedback::SkillUseTransitionRequestV1> {
    records
        .iter()
        .map(
            |record| crate::skill_feedback::SkillUseTransitionRequestV1 {
                version: crate::skill_feedback::SKILL_FEEDBACK_VERSION,
                id: record.id.clone(),
                expected_revision: record.revision,
                expected_skill_tree_hash: record.skill.tree_hash.clone(),
                next_status,
            },
        )
        .collect()
}

fn transition_skill_use_records(
    records: &[crate::skill_feedback::SkillUseRecordV1],
    next_status: crate::skill_feedback::SkillUseStatusV1,
) -> Result<Vec<crate::skill_feedback::SkillUseRecordV1>, String> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    crate::skill_feedback::transition_uses_v1(skill_use_transition_requests(records, next_status))
}

fn rollback_unwritten_user_turn(session_id: &str, turn_id: &str) -> Result<(), String> {
    store::update_messages(session_id, |messages| {
        let Some(last) = messages.last() else {
            return Err("user turn journal is empty".into());
        };
        if last.id != turn_id || last.role != "user" {
            return Err("user turn journal advanced before Runtime dispatch rollback".into());
        }
        messages.pop();
        Ok(())
    })
}

fn record_runtime_write_rejected_marker(
    app: &AppHandle,
    session_id: &str,
    turn_id: &str,
) -> Result<(), String> {
    let message_id = Uuid::new_v4().to_string();
    let content = format!("turn_cancelled|runtime_write_rejected|turn:{turn_id}");
    store::append_message(
        session_id,
        ChatMessageStored {
            id: message_id.clone(),
            role: "tool".into(),
            content: content.clone(),
            thought: None,
            created_at: chrono::Utc::now(),
            is_error: true,
            attachments: None,
            marker: Some("turn_cancelled".into()),
        },
    )?;
    let _ = app.emit(
        "session://turn_marker",
        serde_json::json!({
            "sessionId": session_id,
            "messageId": message_id,
            "marker": "turn_cancelled",
            "reason": "runtime_write_rejected",
            "content": content,
        }),
    );
    Ok(())
}

fn settle_active_skill_uses(
    session: &mut LiveSession,
    next_status: crate::skill_feedback::SkillUseStatusV1,
) {
    if session.active_skill_uses.is_empty() {
        session.pending_skill_settlement = None;
        return;
    }
    let records = session.active_skill_uses.clone();
    match transition_skill_use_records(&records, next_status) {
        Ok(_) => {
            session.active_skill_uses.clear();
            session.pending_skill_settlement = None;
        }
        Err(error) => {
            // The transport acknowledgement and terminal event can race. Only
            // discard the retry source when the durable store proves every row
            // already reached a terminal state.
            let terminal_elsewhere = if error.contains("STALE_SKILL_USE")
                || error.contains("SKILL_USE_INVALID_TRANSITION")
            {
                crate::skill_feedback::list_uses_v1()
                    .ok()
                    .map(|stored| {
                        records.iter().all(|record| {
                            stored
                                .iter()
                                .find(|item| item.id == record.id)
                                .is_some_and(|item| {
                                    matches!(
                                        item.status,
                                        crate::skill_feedback::SkillUseStatusV1::Succeeded
                                            | crate::skill_feedback::SkillUseStatusV1::Failed
                                            | crate::skill_feedback::SkillUseStatusV1::Interrupted
                                    )
                                })
                        })
                    })
                    .unwrap_or(false)
            } else {
                false
            };
            if terminal_elsewhere {
                session.active_skill_uses.clear();
                session.pending_skill_settlement = None;
            } else {
                session.pending_skill_settlement = Some(next_status);
                tracing::warn!(
                    session_id = %session.app_session_id,
                    "settle Skill use evidence deferred: {error}"
                );
            }
        }
    }
}

fn reset_rejected_turn(session: &mut LiveSession) {
    if matches!(
        session.fsm.state(),
        SessionState::Streaming | SessionState::AwaitingPermission
    ) {
        let _ = session.fsm.end_stream();
    }
    session.streaming_message_id = None;
    session.stream_buf.clear();
    session.stream_thought.clear();
    session.stream_last_was_assistant = false;
    session.stream_phase_id_locked = false;
    session.stream_attachments.clear();
    session.journal_throttle.reset();
    session.open_tool_ids.clear();
    session.seen_tool_ids.clear();
    session.deferred_prompt_complete = None;
    session.last_stall_emit = None;
    session.tools_this_turn = 0;
    session.active_skill_uses.clear();
    session.pending_skill_settlement = None;
}

pub struct SessionManager {
    /// Currently focused live session (UI-bound for send).
    inner: Mutex<Option<LiveSession>>,
    /// Busy sessions still receiving ACP events (streaming / permission).
    /// Keyed by app session id. Enables multi-session parallel streaming.
    background: Mutex<HashMap<String, LiveSession>>,
    /// Warm Ready agents for other App sessions (keyed by app session id).
    parked: Mutex<HashMap<String, ParkedAgent>>,
    /// Serialize connect / park / unpark so openSession prefetch cannot race first send.
    connect_lock: tokio::sync::Mutex<()>,
    /// In-process child agents for the Sunsetz kernel.
    subagents: Arc<tokio::sync::Mutex<subagents::SubagentRegistry>>,
    /// Session ids with a Host wake turn starting or running.
    wake_inflight: Mutex<HashSet<String>>,
}

enum AskUserReplyChannel {
    Acp(Arc<AcpClient>),
    Host(tokio::sync::oneshot::Sender<AskUserOutcome>),
    Taken,
}

struct AskUserResolveTarget {
    session_id: String,
    process_id: ProcessId,
    rpc_id: u64,
    activity_id: String,
    question_count: usize,
    reply: AskUserReplyChannel,
    snapshot: InteractionSnapshotV1,
}

enum PermissionReplyChannel {
    Acp(Arc<AcpClient>),
    Host(tokio::sync::oneshot::Sender<PermissionOutcome>),
}

struct PermissionResolveTarget {
    session_id: String,
    process_id: ProcessId,
    interaction_id: String,
    rpc_id: u64,
    reply: Option<PermissionReplyChannel>,
}

struct PlanResolveTarget {
    session_id: String,
    process_id: ProcessId,
    interaction_id: String,
    rpc_id: u64,
    acp: Arc<AcpClient>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            background: Mutex::new(HashMap::new()),
            parked: Mutex::new(HashMap::new()),
            connect_lock: tokio::sync::Mutex::new(()),
            subagents: Arc::new(tokio::sync::Mutex::new(
                subagents::SubagentRegistry::default(),
            )),
            wake_inflight: Mutex::new(HashSet::new()),
        }
    }

    /// Background idle recycle loop (I03). Safe to call once from app setup.
    pub fn start_idle_watchdog(self: &Arc<Self>, app: AppHandle) {
        let mgr = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                mgr.tick_idle_recycle(&app).await;
            }
        });
    }

    /// Background stream stall detector (I06). Safe to call once from app setup.
    pub fn start_stream_stall_watchdog(self: &Arc<Self>, app: AppHandle) {
        let mgr = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(5));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                mgr.tick_stream_stall(&app);
            }
        });
    }

    fn touch_activity_locked(s: &mut LiveSession) {
        s.last_activity = Instant::now();
    }

    fn settle_automation_before_host_kill(session_id: &str, reason: &str) {
        if let Err(error) =
            crate::automation_scheduler::complete_for_session(session_id, false, Some(reason))
        {
            tracing::warn!(
                session_id,
                "settle automation before intentional Runtime kill: {error}"
            );
        }
    }

    fn reset_rejected_session(&self, session_id: &str) {
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if session.app_session_id == session_id {
                    reset_rejected_turn(session);
                    return;
                }
            }
        }
        if let Some(session) = self.background.lock().get_mut(session_id) {
            reset_rejected_turn(session);
        }
    }

    fn update_active_skill_uses_for_session(
        &self,
        session_id: &str,
        records: Vec<crate::skill_feedback::SkillUseRecordV1>,
    ) -> bool {
        let update = |session: &mut LiveSession,
                      records: Vec<crate::skill_feedback::SkillUseRecordV1>| {
            if session.active_skill_uses.len() != records.len()
                || !session
                    .active_skill_uses
                    .iter()
                    .zip(records.iter())
                    .all(|(current, next)| current.id == next.id)
            {
                return false;
            }
            session.active_skill_uses = records;
            true
        };
        {
            let mut live = self.inner.lock();
            if let Some(session) = live
                .as_mut()
                .filter(|session| session.app_session_id == session_id)
            {
                return update(session, records);
            }
        }
        let mut background = self.background.lock();
        let Some(session) = background.get_mut(session_id) else {
            return false;
        };
        update(session, records)
    }

    fn defer_skill_settlement_for_session(
        &self,
        session_id: &str,
        records: Vec<crate::skill_feedback::SkillUseRecordV1>,
        next_status: crate::skill_feedback::SkillUseStatusV1,
    ) -> bool {
        let defer = |session: &mut LiveSession,
                     records: Vec<crate::skill_feedback::SkillUseRecordV1>| {
            session.active_skill_uses = records;
            session.pending_skill_settlement = Some(next_status);
        };
        {
            let mut live = self.inner.lock();
            if let Some(session) = live
                .as_mut()
                .filter(|session| session.app_session_id == session_id)
            {
                defer(session, records);
                return true;
            }
        }
        let mut background = self.background.lock();
        let Some(session) = background.get_mut(session_id) else {
            return false;
        };
        defer(session, records);
        true
    }

    fn fail_prompt_for_session(
        &self,
        app: &AppHandle,
        session_id: &str,
        error: &AgentError,
    ) -> bool {
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if session.app_session_id == session_id {
                    if !session.provider_retry_aborted {
                        Self::record_turn_error(session, app, error);
                        settle_active_skill_uses(
                            session,
                            crate::skill_feedback::SkillUseStatusV1::Failed,
                        );
                        let _ = session.fsm.fail_with(error.clone());
                    }
                    return true;
                }
            }
        }
        let mut background = self.background.lock();
        let Some(session) = background.get_mut(session_id) else {
            return false;
        };
        if !session.provider_retry_aborted {
            Self::record_turn_error(session, app, error);
            settle_active_skill_uses(session, crate::skill_feedback::SkillUseStatusV1::Failed);
            let _ = session.fsm.fail_with(error.clone());
        }
        true
    }

    /// Stream chunk or tool activity — advances stall deadline (I06).

    /// Soft signal when a non-ask turn ends with zero tool events (diagnostic aid for #52).
    /// Call **before** stream buffers are cleared.
    fn empty_run_signal_from_live(
        s: &LiveSession,
        stop_reason: &str,
    ) -> Option<(String, String, String)> {
        let had_body = !s.stream_buf.trim().is_empty();
        let had_thought = !s.stream_thought.trim().is_empty();
        let tools = s.tools_this_turn;
        let mode = s.product_mode.clone().unwrap_or_else(|| "agent".into());
        let app_sid = s.app_session_id.clone();
        let empty = tools == 0
            && (had_body || had_thought)
            && mode != "ask"
            && !s.provider_retry_aborted
            && stop_reason != "cancelled"
            && stop_reason != "stop";
        if empty {
            Some((app_sid, stop_reason.to_string(), mode))
        } else {
            None
        }
    }

    /// Finish turn when a deferred `prompt_complete` is safe (#52).
    /// Returns `Some(empty_run)` if finished (`None` inside = finished, not empty);
    /// returns `None` if still deferred.
    fn try_finish_deferred_prompt_complete(
        s: &mut LiveSession,
    ) -> Option<Option<(String, String, String)>> {
        let Some(stop_reason) = s.deferred_prompt_complete.clone() else {
            return None;
        };
        let awaiting_perm = s.fsm.state() == SessionState::AwaitingPermission;
        if should_defer_prompt_complete(
            awaiting_perm,
            s.pending_plan.is_some(),
            s.pending_ask_user.is_some(),
            s.open_tool_ids.len(),
        ) {
            return None;
        }
        let empty = Self::empty_run_signal_from_live(s, &stop_reason);
        s.deferred_prompt_complete = None;
        // Force-flush assistant turn (I04 end-of-turn path).
        Self::maybe_flush_stream_journal(s, true, false);
        if s.tools_this_turn > 0
            && !s.provider_retry_aborted
            && is_successful_prompt_complete(&stop_reason)
        {
            if let Err(error) = crate::skill_candidates::create_for_session(&s.app_session_id) {
                tracing::warn!(
                    "skill candidate generation failed session={}: {error}",
                    s.app_session_id
                );
            }
        }
        let skill_status =
            if !s.provider_retry_aborted && is_successful_prompt_complete(&stop_reason) {
                crate::skill_feedback::SkillUseStatusV1::Succeeded
            } else if matches!(stop_reason.as_str(), "cancelled" | "stop") {
                crate::skill_feedback::SkillUseStatusV1::Interrupted
            } else {
                crate::skill_feedback::SkillUseStatusV1::Failed
            };
        settle_active_skill_uses(s, skill_status);
        s.stream_buf.clear();
        s.stream_thought.clear();
        s.stream_last_was_assistant = false;
        s.stream_phase_id_locked = false;
        s.stream_attachments.clear();
        s.journal_throttle.reset();
        s.open_tool_ids.clear();
        s.tools_this_turn = 0;
        if s.fsm.state() == SessionState::Streaming
            || s.fsm.state() == SessionState::AwaitingPermission
        {
            let _ = s.fsm.end_stream();
        }
        s.streaming_message_id = None;
        s.last_stall_emit = None;
        tracing::info!("acp turn finished after deferred prompt_complete stop={stop_reason}");
        Some(empty)
    }

    /// Emit empty-run toast event if the finish result says so.
    fn emit_empty_run_if_any(app: &AppHandle, empty: Option<(String, String, String)>) {
        let Some((app_sid, reason, mode)) = empty else {
            return;
        };
        tracing::info!(
            target: "session",
            session = %app_sid,
            stop_reason = %reason,
            mode = %mode,
            "turn ended with zero tool calls (soft empty-run signal)"
        );
        let _ = app.emit(
            "session://turn_empty_run",
            serde_json::json!({
                "sessionId": app_sid,
                "stopReason": reason,
                "mode": mode,
                "toolCount": 0,
            }),
        );
    }

    fn touch_stream_progress_locked(s: &mut LiveSession) {
        let now = Instant::now();
        s.last_activity = now;
        s.last_stream_progress = now;
        s.last_stall_emit = None;
    }

    fn ensure_stream_message_id(
        s: &mut LiveSession,
        runtime_message_id: Option<&str>,
        kind: StreamKind,
    ) {
        if !s.stream_phase_id_locked {
            if let Some(message_id) = runtime_message_id {
                if s.streaming_message_id.as_deref() != Some(message_id)
                    && (s.streaming_message_id.is_none() || matches!(kind, StreamKind::Assistant))
                {
                    s.streaming_message_id = Some(message_id.to_string());
                }
            }
        }
        if s.streaming_message_id.is_none() {
            s.streaming_message_id = Some(
                runtime_message_id
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
            );
        }
    }

    /// Force-flush the old assistant phase, then reserve a Host-owned phase for
    /// any body/media that arrives after an activity row. Runtime message ids
    /// are intentionally ignored because some agents reuse one id per turn.
    fn begin_activity_boundary(s: &mut LiveSession) -> Option<String> {
        Self::maybe_flush_stream_journal(s, true, false);
        let completed_phase_id = s.streaming_message_id.clone();
        s.stream_buf.clear();
        s.stream_thought.clear();
        s.stream_last_was_assistant = false;
        s.stream_attachments.clear();
        s.journal_throttle.reset();
        s.streaming_message_id = Some(Uuid::new_v4().to_string());
        s.stream_phase_id_locked = true;
        completed_phase_id
    }

    fn begin_tool_boundary(s: &mut LiveSession, tool_call_id: &str) -> Option<Option<String>> {
        if tool_call_id.is_empty() || !s.seen_tool_ids.insert(tool_call_id.to_string()) {
            return None;
        }
        Some(Self::begin_activity_boundary(s))
    }

    fn begin_context_compact_boundary(s: &mut LiveSession) -> Option<String> {
        let has_active_phase = s.streaming_message_id.is_some()
            || !s.stream_buf.is_empty()
            || !s.stream_thought.is_empty()
            || !s.stream_attachments.is_empty();
        if !has_active_phase {
            return None;
        }
        Self::begin_activity_boundary(s)
    }

    fn stream_stall_seconds_from_settings() -> u32 {
        normalize_stream_stall_seconds(store::load_settings().stream_stall_seconds)
    }

    fn emit_stream_stall(app: &AppHandle, session_id: &str, stall_seconds: u32) {
        let _ = app.emit(
            "session://stream_stall",
            serde_json::json!({
                "sessionId": session_id,
                "stallSeconds": stall_seconds,
                "code": "STREAM_STALL",
                "message": stream_stall_message(stall_seconds),
            }),
        );
    }

    /// Persist accumulated assistant stream (I04). `force` bypasses the throttle.
    fn maybe_flush_stream_journal(s: &mut LiveSession, force: bool, paragraph_break: bool) {
        let has_content = !s.stream_buf.is_empty()
            || !s.stream_thought.is_empty()
            || !s.stream_attachments.is_empty();
        if !has_content {
            return;
        }
        let now = Instant::now();
        if !s.journal_throttle.should_flush(now, force, paragraph_break) {
            return;
        }
        let mid = s
            .streaming_message_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if s.streaming_message_id.is_none() {
            s.streaming_message_id = Some(mid.clone());
        }
        let atts = if s.stream_attachments.is_empty() {
            None
        } else {
            Some(s.stream_attachments.clone())
        };
        let _ = store::append_message(
            &s.app_session_id,
            ChatMessageStored {
                id: mid,
                role: "assistant".into(),
                content: s.stream_buf.clone(),
                thought: if s.stream_thought.is_empty() {
                    None
                } else {
                    Some(s.stream_thought.clone())
                },
                created_at: chrono::Utc::now(),
                is_error: false,
                attachments: atts,
                marker: None,
            },
        );
        s.meta.updated_at = chrono::Utc::now();
        let _ = store::update_session_meta(&s.meta);
        s.journal_throttle.mark_flushed(now);
        if force {
            s.journal_throttle.reset();
        }
    }

    /// I06: if live session is Streaming with pure silence, emit cancel prompt.
    fn tick_stream_stall(&self, app: &AppHandle) {
        let stall_secs = Self::stream_stall_seconds_from_settings();
        let now = Instant::now();
        let mut guard = self.inner.lock();
        let Some(s) = guard.as_mut() else {
            return;
        };
        // Only pure streaming silence — not permission / plan / ask-user waits.
        if s.fsm.state() != SessionState::Streaming || s.pending_ask_user.is_some() {
            return;
        }
        if s.streaming_message_id.is_none() {
            return;
        }
        if !should_emit_stall(s.last_stream_progress, s.last_stall_emit, stall_secs, now) {
            return;
        }
        s.last_stall_emit = Some(now);
        let sid = s.app_session_id.clone();
        drop(guard);
        tracing::warn!(
            "stream stall: session={sid} silence≥{stall_secs}s — emitting cancel prompt"
        );
        Self::emit_stream_stall(app, &sid, stall_secs);
    }

    /// Live + background + parked processes that still have a living ACP child.
    fn active_process_count(&self) -> u32 {
        let live = self
            .inner
            .lock()
            .as_ref()
            .and_then(|s| s.acp.as_ref())
            .filter(|c| c.is_alive())
            .is_some() as u32;
        let background = self
            .background
            .lock()
            .values()
            .filter(|s| s.acp.as_ref().is_some_and(|c| c.is_alive()))
            .count() as u32;
        let parked = self
            .parked
            .lock()
            .values()
            .filter(|p| p.acp.is_alive())
            .count() as u32;
        live + background + parked
    }

    fn max_concurrent_from_settings() -> u32 {
        normalize_max_concurrent(store::load_settings().max_concurrent_agents)
    }

    fn idle_minutes_from_settings() -> u32 {
        normalize_idle_minutes(store::load_settings().agent_idle_minutes)
    }

    fn emit_idle_recycled(app: &AppHandle, session_id: &str, reason: &str) {
        let _ = app.emit(
            "session://idle_recycled",
            serde_json::json!({
                "sessionId": session_id,
                "reason": reason,
            }),
        );
    }

    fn emit_process_limit(app: &AppHandle, session_id: Option<&str>, max: u32) {
        let _ = app.emit(
            "session://process_limit",
            serde_json::json!({
                "sessionId": session_id,
                "maxConcurrentAgents": max,
                "code": "PROCESS_LIMIT",
                "message": process_limit_message(max),
            }),
        );
    }

    /// Drop dead parked entries; return killed count (for logging).
    fn sweep_dead_parked(&self) -> usize {
        let mut parked = self.parked.lock();
        let before = parked.len();
        parked.retain(|_, p| p.acp.is_alive());
        before.saturating_sub(parked.len())
    }

    /// Park or background the current live session so focus can move.
    ///
    /// - Ready → warm `parked` (idle process).
    /// - Streaming / AwaitingPermission → `background` (keeps full LiveSession + event pump).
    /// - Capacity exceeded → error (PROCESS_LIMIT).
    fn try_park_live(&self) -> Result<(), AgentError> {
        let max = Self::max_concurrent_from_settings();
        let mut guard = self.inner.lock();
        let Some(s) = guard.as_mut() else {
            return Ok(());
        };
        let acp_alive = s.acp.as_ref().is_some_and(|c| c.is_alive());
        let busy = matches!(
            s.fsm.state(),
            SessionState::Streaming | SessionState::AwaitingPermission | SessionState::Connecting
        );
        // Sunsetz kernel has no ACP process. Ready shells are cheap to rebuild,
        // but an in-flight turn must still move to `background` or switching
        // chats drops the live agent_loop (no reply, possible crash).
        if !acp_alive && !busy {
            return Ok(());
        }
        if let Some(pending_status) = s.pending_skill_settlement {
            settle_active_skill_uses(s, pending_status);
        }
        if matches!(s.fsm.state(), SessionState::Ready)
            && (!s.active_skill_uses.is_empty() || s.pending_skill_settlement.is_some())
        {
            return Err(AgentError::new(
                AgentErrorCode::ProcessLimit,
                "Skill use settlement is still pending; retry settlement before switching chats",
            ));
        }
        match s.fsm.state() {
            SessionState::Ready if s.streaming_message_id.is_none() => {
                let acp = match s.acp.take() {
                    Some(c) if c.is_alive() => c,
                    Some(_) | None => return Ok(()),
                };
                let parked = ParkedAgent {
                    process_id: s.process_id.clone(),
                    app_session_id: s.app_session_id.clone(),
                    meta: s.meta.clone(),
                    acp,
                    last_activity: s.last_activity,
                    model_id: s.model_id.clone(),
                    effort: s.effort.clone(),
                    product_mode: s.product_mode.clone(),
                    project_path: s.project_path.clone(),
                    policy: s.policy,
                    needs_history_bootstrap: s.needs_history_bootstrap,
                    backend: s.backend.clone(),
                };
                let _ = guard.take();
                drop(guard);
                self.parked
                    .lock()
                    .insert(parked.app_session_id.clone(), parked);
                Ok(())
            }
            SessionState::Idle | SessionState::Disconnected => Ok(()),
            // Busy: keep full LiveSession in background so streaming continues.
            SessionState::Streaming
            | SessionState::AwaitingPermission
            | SessionState::Connecting => {
                // Do not call active_process_count() here: it also locks `inner`.
                let background = self
                    .background
                    .lock()
                    .values()
                    .filter(|session| session.acp.as_ref().is_some_and(|c| c.is_alive()))
                    .count() as u32;
                let parked = self
                    .parked
                    .lock()
                    .values()
                    .filter(|parked| parked.acp.is_alive())
                    .count() as u32;
                let this_process = acp_alive as u32;
                if background
                    .saturating_add(parked)
                    .saturating_add(this_process)
                    > max
                {
                    return Err(AgentError::new(
                        AgentErrorCode::ProcessLimit,
                        format!(
                            "Session is busy and process limit ({max}) is full. Stop a turn or raise the limit. {}",
                            process_limit_message(max)
                        ),
                    ));
                }
                let Some(live) = guard.take() else {
                    return Ok(());
                };
                let sid = live.app_session_id.clone();
                drop(guard);
                tracing::info!(
                    "acp demote busy session to background sid={sid} state={:?}",
                    live.fsm.state()
                );
                self.background.lock().insert(sid, live);
                Ok(())
            }
            other => Err(AgentError::new(
                AgentErrorCode::ProcessLimit,
                format!(
                    "Session is busy ({other:?}). Stop the turn or wait, then switch chats. {}",
                    process_limit_message(max)
                ),
            )),
        }
    }

    /// If a background session finished its turn (Ready), convert to warm parked.
    fn promote_background_ready_to_parked(&self, app_session_id: &str) {
        let mut bg = self.background.lock();
        if let Some(session) = bg.get_mut(app_session_id) {
            if let Some(pending_status) = session.pending_skill_settlement {
                settle_active_skill_uses(session, pending_status);
            }
        }
        let ready = bg.get(app_session_id).is_some_and(|s| {
            matches!(s.fsm.state(), SessionState::Ready)
                && s.streaming_message_id.is_none()
                && s.active_skill_uses.is_empty()
                && s.pending_skill_settlement.is_none()
                && s.acp.as_ref().is_some_and(|c| c.is_alive())
        });
        if !ready {
            return;
        }
        let Some(mut s) = bg.remove(app_session_id) else {
            return;
        };
        drop(bg);
        let Some(acp) = s.acp.take() else {
            return;
        };
        let parked = ParkedAgent {
            process_id: s.process_id.clone(),
            app_session_id: s.app_session_id.clone(),
            meta: s.meta.clone(),
            acp,
            last_activity: s.last_activity,
            model_id: s.model_id.clone(),
            effort: s.effort.clone(),
            product_mode: s.product_mode.clone(),
            project_path: s.project_path.clone(),
            policy: s.policy,
            needs_history_bootstrap: s.needs_history_bootstrap,
            backend: s.backend.clone(),
        };
        self.parked
            .lock()
            .insert(parked.app_session_id.clone(), parked);
        tracing::info!(
            "acp background session ready → parked sid={}",
            app_session_id
        );
    }

    /// Promote a parked agent into the live slot (caller must have cleared live).
    fn unpark_to_live(&self, app_session_id: &str) -> Option<LiveSession> {
        let parked = self.parked.lock().remove(app_session_id)?;
        if !parked.acp.is_alive() {
            return None;
        }
        let mut fsm = SessionFsm::new();
        // Parked agents were Ready; restore Ready without connect handshake.
        let _ = fsm.start_connect();
        let _ = fsm.handshake_ok();
        let now = Instant::now();
        Some(LiveSession {
            app_session_id: parked.app_session_id,
            process_id: parked.process_id,
            meta: parked.meta,
            fsm,
            backend: parked.backend,
            acp: Some(parked.acp),
            mock_stream: None,
            agent_cancel: None,
            host_turn_id: None,
            streaming_message_id: None,
            stream_buf: String::new(),
            stream_thought: String::new(),
            stream_last_was_assistant: false,
            stream_phase_id_locked: false,
            stream_attachments: Vec::new(),
            model_id: parked.model_id,
            effort: parked.effort,
            product_mode: parked.product_mode,
            project_path: parked.project_path,
            allow_cache: SessionAllowCache::default(),
            policy: parked.policy,
            provider_retry_attempt: 0,
            provider_retry_aborted: false,
            needs_history_bootstrap: parked.needs_history_bootstrap,
            pending_plan: None,
            pending_permission: None,
            host_rpc_seq: 0,
            pending_ask_user: None,
            last_activity: now,
            last_stream_progress: now,
            last_stall_emit: None,
            journal_throttle: JournalWriteThrottle::with_default_interval(),
            open_tool_ids: HashSet::new(),
            seen_tool_ids: HashSet::new(),
            deferred_prompt_complete: None,
            tools_this_turn: 0,
            active_skill_uses: Vec::new(),
            pending_skill_settlement: None,
            allow_auto_wake: false,
        })
    }

    /// Kill oldest parked agents until under capacity (or none left).
    async fn free_parked_for_capacity(&self, app: &AppHandle, need_slots: u32) {
        if need_slots == 0 {
            return;
        }
        for _ in 0..need_slots {
            let victim = {
                let mut parked = self.parked.lock();
                let key = parked
                    .iter()
                    .min_by_key(|(_, p)| p.last_activity)
                    .map(|(k, _)| k.clone());
                key.and_then(|k| parked.remove(&k))
            };
            let Some(p) = victim else {
                break;
            };
            tracing::info!(
                "process limit: recycling parked session={} process={}",
                p.app_session_id,
                p.process_id
            );
            p.acp.kill().await;
            Self::emit_idle_recycled(app, &p.app_session_id, "capacity");
        }
    }

    /// Idle recycle for live + parked (I03).
    async fn tick_idle_recycle(&self, app: &AppHandle) {
        let idle_mins = Self::idle_minutes_from_settings();
        let now = Instant::now();
        self.sweep_dead_parked();

        // Parked first
        let expired_parked: Vec<ParkedAgent> = {
            let mut parked = self.parked.lock();
            let keys: Vec<String> = parked
                .iter()
                .filter(|(_, p)| is_idle_expired(p.last_activity, idle_mins, now))
                .map(|(k, _)| k.clone())
                .collect();
            keys.into_iter().filter_map(|k| parked.remove(&k)).collect()
        };
        for p in expired_parked {
            tracing::info!(
                "idle recycle parked session={} after {}min",
                p.app_session_id,
                idle_mins
            );
            p.acp.kill().await;
            Self::emit_idle_recycled(app, &p.app_session_id, "idle");
        }

        // Live: only when Ready (not mid-turn)
        let live_kill = {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                let idle = is_idle_expired(s.last_activity, idle_mins, now);
                let ready = matches!(s.fsm.state(), SessionState::Ready)
                    && s.streaming_message_id.is_none();
                if idle && ready {
                    if let Some(acp) = s.acp.take() {
                        s.fsm.soft_disconnect();
                        s.needs_history_bootstrap = false;
                        Some((s.app_session_id.clone(), acp))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some((sid, acp)) = live_kill {
            tracing::info!("idle recycle live session={sid} after {idle_mins}min");
            acp.kill().await;
            Self::emit_idle_recycled(app, &sid, "idle");
            Self::emit_state(app, &self.snapshot());
        }
    }

    fn backend_name() -> String {
        agent_loop::current_backend()
    }

    fn session_is_busy(session: &LiveSession) -> bool {
        matches!(
            session.fsm.state(),
            SessionState::Connecting | SessionState::Streaming | SessionState::AwaitingPermission
        )
    }

    fn busy_session_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        let push = |ids: &mut Vec<String>, seen: &mut HashSet<String>, session: &LiveSession| {
            if Self::session_is_busy(session) && seen.insert(session.app_session_id.clone()) {
                ids.push(session.app_session_id.clone());
            }
        };
        if let Some(live) = self.inner.lock().as_ref() {
            push(&mut ids, &mut seen, live);
        }
        for session in self.background.lock().values() {
            push(&mut ids, &mut seen, session);
        }
        ids
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let settings = store::load_settings();
        let requested = crate::runtime_compat::SandboxProfileV1::parse(&settings.sandbox_profile);
        let busy_session_ids = self.busy_session_ids();
        let guard = self.inner.lock();
        match guard.as_ref() {
            None => SessionSnapshot {
                session_id: None,
                agent_session_id: None,
                state: SessionState::Idle,
                last_error: None,
                streaming_message_id: None,
                backend: Self::backend_name(),
                model_id: None,
                project_path: None,
                title: String::new(),
                context_usage: None,
                sandbox: crate::runtime_compat::sandbox_support(requested),
                busy_session_ids,
            },
            Some(s) => {
                let mut sandbox = s
                    .acp
                    .as_ref()
                    .map(|client| client.sandbox_application())
                    .unwrap_or_else(|| crate::runtime_compat::sandbox_support(requested));
                if sandbox.requested != requested.as_str() {
                    sandbox.requested = requested.as_str().into();
                    sandbox.state = "restart_required".into();
                    sandbox.reason =
                        Some("Sandbox profile changed; the active Runtime must restart".into());
                }
                SessionSnapshot {
                    session_id: Some(s.app_session_id.clone()),
                    agent_session_id: s.meta.agent_session_id.clone(),
                    state: s.fsm.state(),
                    last_error: s.fsm.last_error().cloned(),
                    streaming_message_id: s.streaming_message_id.clone(),
                    backend: s.backend.clone(),
                    model_id: s.model_id.clone(),
                    project_path: s.project_path.clone(),
                    title: s.meta.title.clone(),
                    context_usage: s.meta.context_usage.clone(),
                    sandbox,
                    busy_session_ids,
                }
            }
        }
    }

    pub fn active_sandbox_application(
        &self,
    ) -> Option<crate::runtime_compat::SandboxApplicationV1> {
        self.inner
            .lock()
            .as_ref()
            .and_then(|session| session.acp.as_ref())
            .map(|client| client.sandbox_application())
    }

    /// Runtime diagnostics for a session export package (live or parked).
    /// Returns `None` when the session is not currently attached to a process.
    pub fn diagnostic_runtime_for(&self, app_session_id: &str) -> Option<serde_json::Value> {
        {
            let guard = self.inner.lock();
            if let Some(s) = guard.as_ref() {
                if s.app_session_id == app_session_id {
                    let cwd = s.acp.as_ref().map(|c| c.cwd().display().to_string());
                    let agent_alive = s.acp.as_ref().is_some_and(|c| c.is_alive());
                    return Some(serde_json::json!({
                        "slot": "live",
                        "state": format!("{:?}", s.fsm.state()),
                        "backend": s.backend,
                        "modelId": s.model_id,
                        "effort": s.effort,
                        "mode": s.product_mode,
                        "permissionPolicy": s.policy.as_str(),
                        "projectPath": s.project_path,
                        "agentSessionId": s.meta.agent_session_id,
                        "processId": s.process_id,
                        "agentAlive": agent_alive,
                        "cwd": cwd,
                        "streamingMessageId": s.streaming_message_id,
                        "toolsThisTurn": s.tools_this_turn,
                        "needsHistoryBootstrap": s.needs_history_bootstrap,
                        "lastError": s.fsm.last_error().map(|e| {
                            serde_json::json!({
                                "code": e.code.as_str(),
                                "message": e.message,
                            })
                        }),
                    }));
                }
            }
        }
        let parked = self.parked.lock();
        if let Some(p) = parked.get(app_session_id) {
            return Some(serde_json::json!({
                "slot": "parked",
                "state": "Ready",
                "backend": p.backend,
                "modelId": p.model_id,
                "effort": p.effort,
                "mode": p.product_mode,
                "permissionPolicy": p.policy.as_str(),
                "projectPath": p.project_path,
                "agentSessionId": p.meta.agent_session_id,
                "processId": p.process_id,
                "agentAlive": p.acp.is_alive(),
                "cwd": p.acp.cwd().display().to_string(),
                "streamingMessageId": serde_json::Value::Null,
                "toolsThisTurn": 0,
                "needsHistoryBootstrap": p.needs_history_bootstrap,
                "lastError": serde_json::Value::Null,
            }));
        }
        None
    }

    /// Keep live session meta title in sync after store rename / auto-title.
    /// Without this, later `session://state` events re-emit the stale connect-time title
    /// and wipe sidebar / header renames.
    pub fn apply_title(&self, app: &AppHandle, session_id: &str, title: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let mut guard = self.inner.lock();
        let Some(s) = guard.as_mut() else {
            return false;
        };
        if s.app_session_id != session_id {
            return false;
        }
        if s.meta.title == title {
            return true;
        }
        s.meta.title = title.to_string();
        s.meta.updated_at = chrono::Utc::now();
        drop(guard);
        Self::emit_state(app, &self.snapshot());
        true
    }

    fn emit_state(app: &AppHandle, snap: &SessionSnapshot) {
        let _ = app.emit("session://state", snap);
    }

    fn publish_interaction(app: &AppHandle, snapshot: &InteractionSnapshotV1) {
        if let Err(error) = crate::interactions::record(snapshot) {
            tracing::warn!(
                session = %snapshot.session_id,
                interaction = %snapshot.interaction_id,
                "persist interaction snapshot: {error}"
            );
        }
        let _ = app.emit("session://interaction", snapshot);
    }

    fn emit_plan_compat(
        app: &AppHandle,
        session_id: &str,
        entries: &serde_json::Value,
        body: &Option<String>,
        rpc_id: Option<u64>,
        tool_call_id: &Option<String>,
        interaction_id: Option<&str>,
    ) {
        let payload = serde_json::json!({
            "sessionId": session_id,
            "entries": entries,
            "body": body,
            "rpcId": rpc_id,
            "toolCallId": tool_call_id,
            "waiting": rpc_id.is_none(),
            "interactionId": interaction_id,
        });
        let _ = app.emit("session://plan", &payload);
    }

    fn emit_plan_artifact(app: &AppHandle, artifact: &crate::plan_artifacts::PlanArtifactV1) {
        let _ = app.emit("session://plan_artifact", artifact);
    }

    fn emit_plan_artifact_pair(
        app: &AppHandle,
        artifact: &crate::plan_artifacts::PlanArtifactV1,
        rpc_id: Option<u64>,
    ) {
        let revision = artifact.revisions.last();
        let empty_entries = serde_json::Value::Array(Vec::new());
        Self::emit_plan_compat(
            app,
            &artifact.session_id,
            revision.map(|row| &row.entries).unwrap_or(&empty_entries),
            &revision.and_then(|row| row.body.clone()),
            rpc_id,
            &artifact.tool_call_id,
            artifact.interaction_id.as_deref(),
        );
        Self::emit_plan_artifact(app, artifact);
    }

    fn publish_interrupted_session_gates(app: &AppHandle, interrupted: InterruptedSessionGates) {
        for snapshot in interrupted.interactions {
            Self::publish_interaction(app, &snapshot);
        }
        for artifact in interrupted.plan_artifacts {
            Self::emit_plan_artifact_pair(app, &artifact, None);
        }
    }

    /// Persist + push a chat-visible error for a failed turn (retries exhausted, RPC fail, …).
    /// Updates UI via `session://turn_error` so the optimistic thinking bubble becomes a record.
    ///
    /// Content is intentionally short (code + compact reason). The UI maps codes to i18n copy
    /// and must not dump raw RPC/MCP stderr into the chat bubble.
    fn record_turn_error(s: &mut LiveSession, app: &AppHandle, err: &AgentError) {
        let mid = s
            .streaming_message_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let code = err.code.as_str();
        let detail = sanitize_error_detail(err.message.trim());
        // Persist machine-readable code first so the frontend can i18n the summary.
        let content = if detail.is_empty() {
            format!("**{code}**")
        } else {
            format!("**{code}**\n\n{detail}")
        };
        let _ = store::append_message(
            &s.app_session_id,
            ChatMessageStored {
                id: mid.clone(),
                role: "assistant".into(),
                content: content.clone(),
                thought: None,
                created_at: chrono::Utc::now(),
                is_error: true,
                attachments: None,
                marker: None,
            },
        );
        s.meta.updated_at = chrono::Utc::now();
        let _ = store::update_session_meta(&s.meta);
        s.stream_buf.clear();
        s.stream_thought.clear();
        s.stream_last_was_assistant = false;
        s.stream_phase_id_locked = false;
        s.stream_attachments.clear();
        s.streaming_message_id = None;
        s.journal_throttle.reset();
        s.open_tool_ids.clear();
        s.last_stall_emit = None;

        let _ = app.emit(
            "session://turn_error",
            serde_json::json!({
                "sessionId": s.app_session_id,
                "messageId": mid,
                "code": code,
                "message": detail,
                "content": content,
            }),
        );
    }

    pub async fn connect(
        self: &Arc<Self>,
        app: AppHandle,
        project_path: Option<String>,
        app_session_id: Option<String>,
        mock_mode: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let _connect_guard = self.connect_lock.lock().await;
        self.connect_inner(app, project_path, app_session_id, mock_mode)
            .await
    }

    fn kernel_live_session(
        meta: SessionMeta,
        process_id: String,
        project_path: Option<String>,
        prefs: &store::ComposerPrefs,
        policy: PermissionPolicy,
        backend: String,
    ) -> Result<LiveSession, String> {
        let mut fsm = SessionFsm::new();
        fsm.start_connect().map_err(|e| e.to_string())?;
        fsm.handshake_ok().map_err(|e| e.to_string())?;
        let now = Instant::now();
        let journal_has_history = store::load_messages(&meta.id).iter().any(|m| {
            (m.role == "user" || m.role == "assistant")
                && !m.content.trim().is_empty()
                && !m.is_error
        });
        Ok(LiveSession {
            app_session_id: meta.id.clone(),
            process_id,
            meta,
            fsm,
            backend,
            acp: None,
            mock_stream: None,
            agent_cancel: None,
            host_turn_id: None,
            streaming_message_id: None,
            stream_buf: String::new(),
            stream_thought: String::new(),
            stream_last_was_assistant: false,
            stream_phase_id_locked: false,
            stream_attachments: Vec::new(),
            model_id: Some(prefs.model_id.clone()),
            effort: Some(prefs.effort.clone()),
            product_mode: Some(prefs.mode.clone()),
            project_path,
            allow_cache: SessionAllowCache::default(),
            policy,
            provider_retry_attempt: 0,
            provider_retry_aborted: false,
            needs_history_bootstrap: journal_has_history,
            pending_plan: None,
            pending_permission: None,
            host_rpc_seq: 0,
            pending_ask_user: None,
            last_activity: now,
            last_stream_progress: now,
            last_stall_emit: None,
            journal_throttle: JournalWriteThrottle::with_default_interval(),
            open_tool_ids: HashSet::new(),
            seen_tool_ids: HashSet::new(),
            deferred_prompt_complete: None,
            tools_this_turn: 0,
            active_skill_uses: Vec::new(),
            pending_skill_settlement: None,
            allow_auto_wake: false,
        })
    }

    fn install_background_kernel_session(
        &self,
        session_id: &str,
        project_path: Option<String>,
    ) -> Result<(), String> {
        {
            if self
                .inner
                .lock()
                .as_ref()
                .is_some_and(|session| session.app_session_id == session_id)
            {
                return Ok(());
            }
            if self.background.lock().contains_key(session_id) {
                return Ok(());
            }
        }
        let backend = Self::backend_name();
        if !agent_loop::is_sunsetz_backend(&backend) && backend != agent_loop::BACKEND_MOCK {
            return Err("host ignition requires the Sunsetz kernel".into());
        }
        let mut meta = store::load_sessions_index()
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "session not found".to_string())?;
        let prefs =
            store::resolve_composer_prefs(meta.project_id.as_deref(), Some(meta.id.as_str()));
        let policy = PermissionPolicy::parse(&prefs.permission_policy);
        meta.model_id = Some(prefs.model_id.clone());
        meta.effort = Some(prefs.effort.clone());
        meta.mode = Some(prefs.mode.clone());
        meta.permission_policy = Some(prefs.permission_policy.clone());
        meta.agent_session_id = Some(Uuid::new_v4().to_string());
        let _ = store::update_session_meta(&meta);
        let session = Self::kernel_live_session(
            meta,
            Uuid::new_v4().to_string(),
            project_path,
            &prefs,
            policy,
            backend,
        )?;
        self.background
            .lock()
            .insert(session.app_session_id.clone(), session);
        Ok(())
    }

    pub async fn connect_background_kernel(
        self: &Arc<Self>,
        app: AppHandle,
        session_id: String,
        project_path: Option<String>,
    ) -> Result<(), String> {
        let _connect_guard = self.connect_lock.lock().await;
        self.install_background_kernel_session(&session_id, project_path)?;
        Self::emit_state(&app, &self.snapshot());
        Ok(())
    }

    /// Create, bind, and start a scheduled turn in the background without
    /// stealing the live workbench session. `bound` is true after claim bind.
    pub async fn ignite_automation_claim(
        self: &Arc<Self>,
        app: AppHandle,
        mut claim: crate::automation_scheduler::AutomationClaimV1,
    ) -> Result<crate::automation_scheduler::AutomationClaimV1, crate::automation_scheduler::IgniteFailure>
    {
        use crate::automation_scheduler::IgniteFailure;
        let fail = |bound: bool, error: String| Err(IgniteFailure { bound, error });
        let backend = Self::backend_name();
        if !agent_loop::is_sunsetz_backend(&backend) && backend != agent_loop::BACKEND_MOCK {
            return fail(false, "host ignition requires the Sunsetz kernel".into());
        }
        let project = claim
            .automation
            .project_id
            .as_deref()
            .and_then(|id| store::load_projects().into_iter().find(|project| project.id == id));
        if let Some(project) = project.as_ref() {
            if !project.trusted {
                let error = format!("project `{}` is not trusted", project.name);
                let _ = crate::automation_scheduler::complete(
                    &claim.claim_id,
                    false,
                    Some(&error),
                );
                return fail(false, error);
            }
        }
        let project_path = project.as_ref().map(|project| project.path.clone());
        let title = if claim.automation.title.trim().is_empty() {
            "Scheduled".to_string()
        } else {
            claim.automation.title.clone()
        };
        let mut meta = match store::create_session(
            claim.automation.project_id.clone(),
            Some(title),
            true,
        ) {
            Ok(meta) => meta,
            Err(error) => return fail(false, error),
        };
        if claim.automation.model_id.is_some() || claim.automation.effort.is_some() {
            if let Some(model_id) = claim.automation.model_id.clone() {
                meta.model_id = Some(model_id);
            }
            if let Some(effort) = claim.automation.effort.clone() {
                meta.effort = Some(effort);
            }
            let _ = store::update_session_meta(&meta);
        }
        if let Err(error) =
            crate::automation_scheduler::bind_session(&claim.claim_id, &meta.id)
        {
            let _ = store::delete_session(&meta.id);
            return fail(false, error);
        }
        claim.session_id = Some(meta.id.clone());
        if let Err(error) = self
            .connect_background_kernel(app.clone(), meta.id.clone(), project_path)
            .await
        {
            let _ = crate::automation_scheduler::complete(
                &claim.claim_id,
                false,
                Some(&error),
            );
            return fail(true, error);
        }
        let prompt = format!(
            "[Scheduled: {}]\n\n{}",
            claim.automation.title, claim.automation.prompt
        );
        if let Err(error) = self
            .send_message_for_session(app, meta.id, prompt, None, None)
            .await
        {
            let _ = crate::automation_scheduler::complete(
                &claim.claim_id,
                false,
                Some(&error),
            );
            return fail(true, error);
        }
        Ok(claim)
    }

    async fn connect_inner(
        self: &Arc<Self>,
        app: AppHandle,
        project_path: Option<String>,
        app_session_id: Option<String>,
        mock_mode: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let settings = store::load_settings();
        let max_concurrent = normalize_max_concurrent(settings.max_concurrent_agents);
        self.sweep_dead_parked();

        // Orphan chats (no project): use $HOME, never process cwd.
        // Dock-launched macOS apps often have cwd `/`, which confuses the agent.
        let cwd = project_path
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = crate::process_util::user_home();
                if home.is_dir() {
                    home
                } else {
                    std::env::current_dir().unwrap_or_else(|_| ".".into())
                }
            });

        // Ensure app session meta
        let mut meta = if let Some(id) = app_session_id {
            store::load_sessions_index()
                .into_iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| {
                    store::create_session(None, Some("New chat".into()), false)
                        .expect("create session")
                })
        } else {
            store::create_session(None, Some("New chat".into()), false).map_err(|e| e)?
        };

        // Resolve model / effort / permission / mode for this project+session scope.
        let prefs =
            store::resolve_composer_prefs(meta.project_id.as_deref(), Some(meta.id.as_str()));
        let policy = PermissionPolicy::parse(&prefs.permission_policy);
        let agent_model = crate::providers::agent_spawn_model_id(&prefs.model_id);
        let sandbox_profile =
            crate::runtime_compat::SandboxProfileV1::parse(&settings.sandbox_profile);

        // Already live on this App session with a healthy agent → no-op (or soft re-bind prefs).
        {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                let kernel_ready = agent_loop::is_sunsetz_backend(&s.backend)
                    || s.acp.as_ref().is_some_and(|c| c.is_alive());
                let sandbox_ok = agent_loop::is_sunsetz_backend(&s.backend)
                    || s.acp.as_ref().is_some_and(|client| {
                        client.sandbox_application().requested == sandbox_profile.as_str()
                    });
                if s.app_session_id == meta.id
                    && s.project_path == project_path
                    && kernel_ready
                    && matches!(s.fsm.state(), SessionState::Ready)
                    && s.streaming_message_id.is_none()
                    && s.effort.as_deref() == Some(prefs.effort.as_str())
                    && sandbox_ok
                {
                    Self::touch_activity_locked(s);
                    tracing::info!("acp connect no-op: already ready session={}", meta.id);
                    return Ok(self.snapshot());
                }
            }
        }

        // Target already streaming in background → promote to focus.
        if self.background.lock().contains_key(&meta.id) {
            if let Err(e) = self.try_park_live() {
                Self::emit_process_limit(&app, Some(&meta.id), max_concurrent);
                return Err(format!("{}: {}", e.code.as_str(), e.message));
            }
            if let Some(live) = self.background.lock().remove(&meta.id) {
                *self.inner.lock() = Some(live);
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                tracing::info!("acp promoted background session to live sid={}", meta.id);
                return Ok(snap);
            }
        }

        // Target already parked (warm multi-session) → unpark.
        // Sunsetz kernel does not attach a grok ACP process.
        if agent_loop::use_sunsetz_kernel() {
            let leftover = { self.parked.lock().remove(&meta.id).map(|parked| parked.acp) };
            if let Some(client) = leftover {
                client.kill().await;
            }
        } else if self.parked.lock().contains_key(&meta.id) {
            let stale = self.parked.lock().get(&meta.id).is_some_and(|parked| {
                parked.acp.sandbox_application().requested != sandbox_profile.as_str()
            });
            if stale {
                let stale_client = { self.parked.lock().remove(&meta.id).map(|parked| parked.acp) };
                if let Some(client) = stale_client {
                    client.kill().await;
                }
            }
            // Park current live if needed (busy → demote to background / park).
            if let Err(e) = self.try_park_live() {
                Self::emit_process_limit(&app, Some(&meta.id), max_concurrent);
                return Err(format!("{}: {}", e.code.as_str(), e.message));
            }
            if let Some(live) = self.unpark_to_live(&meta.id) {
                // Refresh prefs on shell (model may have changed in UI).
                let mut live = live;
                live.model_id = Some(prefs.model_id.clone());
                live.effort = Some(prefs.effort.clone());
                live.product_mode = Some(prefs.mode.clone());
                live.policy = policy;
                live.project_path = project_path.clone();
                live.meta.model_id = Some(prefs.model_id.clone());
                live.meta.mode = Some(prefs.mode.clone());
                live.meta.effort = Some(prefs.effort.clone());
                live.meta.permission_policy = Some(prefs.permission_policy.clone());
                // Best-effort align agent process to channel prefs.
                if let Some(acp) = live.acp.clone() {
                    if let Err(e) = acp.set_model(&agent_model).await {
                        tracing::warn!("acp set_model on unpark soft-fail: {e}");
                    }
                    if let Err(e) = acp.set_mode(&prefs.mode).await {
                        tracing::warn!("acp set_mode on unpark soft-fail: {e}");
                    }
                }
                *self.inner.lock() = Some(live);
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                tracing::info!("acp unparked warm session={}", meta.id);
                return Ok(snap);
            }
            // Parked process died — fall through to cold spawn.
        }

        // Cross-session warm reuse: same process, switch ACP session without respawn.
        // Only when target is not a different parked agent and flags match.
        let reuse_pair = {
            if agent_loop::use_sunsetz_kernel() {
                None
            } else {
                let same_focus = self
                    .inner
                    .lock()
                    .as_ref()
                    .map(|s| s.app_session_id == meta.id)
                    .unwrap_or(false);
                if same_focus {
                    None
                } else {
                    Self::take_reusable_acp(
                        &self.inner,
                        &cwd,
                        &project_path,
                        &prefs,
                        policy,
                        sandbox_profile,
                    )
                }
            }
        };

        if reuse_pair.is_some() {
            // Keep process; drop LiveSession shell so we can rebind (1 process stays).
            let _ = self.inner.lock().take();
            Self::emit_state(&app, &self.snapshot());
        } else {
            // Park live Ready agent when switching focus (multi-warm). Busy → error.
            let live_sid = self.inner.lock().as_ref().map(|s| s.app_session_id.clone());
            if live_sid.as_deref() != Some(meta.id.as_str()) {
                if let Err(e) = self.try_park_live() {
                    Self::emit_process_limit(&app, Some(&meta.id), max_concurrent);
                    return Err(format!("{}: {}", e.code.as_str(), e.message));
                }
                // Clear disconnected / dead live shell so we can rebuild.
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_ref() {
                        let busy = matches!(
                            s.fsm.state(),
                            SessionState::Streaming
                                | SessionState::AwaitingPermission
                                | SessionState::Connecting
                        );
                        if busy {
                            // try_park_live should have moved this; never drop it.
                        } else if s.app_session_id != meta.id
                            || s.acp.is_none()
                            || !matches!(s.fsm.state(), SessionState::Ready)
                        {
                            let _ = guard.take();
                        }
                    }
                }
            } else {
                // Same session reconnect / flag change — kill any leftover process.
                let leftover = {
                    let mut guard = self.inner.lock();
                    guard.take().and_then(|mut s| s.acp.take())
                };
                if let Some(acp) = leftover {
                    acp.kill().await;
                }
            }
            Self::emit_state(&app, &self.snapshot());
        }

        // Independent GROK_HOME: push permission into agent config before spawn so
        // dontAsk / acceptEdits / YOLO apply agent-side (not only Host).
        if let Err(e) = crate::agent_prefs::sync_permission_to_agent_profile(
            &settings.session_data_mode,
            &prefs.permission_policy,
        ) {
            tracing::warn!("sync agent permission prefs: {e}");
        }

        // Warm reuse must keep the original process_id so the event pump still routes.
        let process_id = reuse_pair
            .as_ref()
            .map(|(pid, _)| pid.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        {
            let mut fsm = SessionFsm::new();
            fsm.start_connect().map_err(|e| e.to_string())?;
            let now = Instant::now();
            *self.inner.lock() = Some(LiveSession {
                app_session_id: meta.id.clone(),
                process_id: process_id.clone(),
                meta: meta.clone(),
                fsm,
                backend: Self::backend_name(),
                acp: None,
                mock_stream: None,
                agent_cancel: None,
                host_turn_id: None,
                streaming_message_id: None,
                stream_buf: String::new(),
                stream_thought: String::new(),
                stream_last_was_assistant: false,
                stream_phase_id_locked: false,
                stream_attachments: Vec::new(),
                model_id: Some(prefs.model_id.clone()),
                effort: Some(prefs.effort.clone()),
                product_mode: Some(prefs.mode.clone()),
                project_path: project_path.clone(),
                allow_cache: SessionAllowCache::default(),
                policy,
                provider_retry_attempt: 0,
                provider_retry_aborted: false,
                needs_history_bootstrap: false,
                pending_plan: None,
                pending_permission: None,
                host_rpc_seq: 0,
                pending_ask_user: None,
                last_activity: now,
                last_stream_progress: now,
                last_stall_emit: None,
                journal_throttle: JournalWriteThrottle::with_default_interval(),
                open_tool_ids: HashSet::new(),
                seen_tool_ids: HashSet::new(),
                deferred_prompt_complete: None,
                tools_this_turn: 0,
                active_skill_uses: Vec::new(),
                pending_skill_settlement: None,
                allow_auto_wake: false,
            });
        }
        Self::emit_state(&app, &self.snapshot());

        let use_mock = AcpClient::use_mock()
            || mock_mode.as_deref() == Some("mock")
            || mock_mode.as_deref() == Some("fail_cli_not_found");

        if use_mock {
            return self.connect_mock(app, mock_mode).await;
        }

        if agent_loop::use_sunsetz_kernel() {
            return self.connect_sunsetz(app, meta).await;
        }

        // Remember prior agent session for resume (before we overwrite meta).
        let resume_agent_sid = meta.agent_session_id.clone();
        let journal_has_history = store::load_messages(&meta.id).iter().any(|m| {
            (m.role == "user" || m.role == "assistant")
                && !m.content.trim().is_empty()
                && !m.is_error
        });

        let (client, reused_process, process_id) = if let Some((pid, existing)) = reuse_pair {
            tracing::info!(
                "acp warm reuse process cwd={} effort={} app_session={}",
                cwd.display(),
                prefs.effort,
                meta.id
            );
            (existing, true, pid)
        } else {
            // Capacity: free LRU parked if needed, else reject.
            self.sweep_dead_parked();
            let active = self.active_process_count();
            // active does not yet include this new spawn (live has no acp).
            if !can_spawn_process(active, max_concurrent) {
                // Try freeing one parked LRU slot.
                self.free_parked_for_capacity(&app, 1).await;
            }
            let active = self.active_process_count();
            if !can_spawn_process(active, max_concurrent) {
                let err = AgentError::new(
                    AgentErrorCode::ProcessLimit,
                    process_limit_message(max_concurrent),
                );
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let _ = s.fsm.connect_failed(err.clone());
                    }
                }
                Self::emit_process_limit(&app, Some(&meta.id), max_concurrent);
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                return Ok(snap);
            }

            // Real ACP cold spawn
            let probe = cli_probe::probe_cli(settings.manual_cli_path.as_deref());
            if !probe.found {
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let _ = s.fsm.connect_failed(AgentError::new(
                            AgentErrorCode::CliNotFound,
                            "Grok Build CLI not found. Install Grok Build or set path in Settings.",
                        ));
                    }
                }
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                return Ok(snap);
            }

            let cli_path = std::path::PathBuf::from(probe.path.unwrap());
            let spawn_opts = crate::acp_client::SpawnOptions {
                model_id: Some(agent_model.clone()),
                effort: Some(prefs.effort.clone()),
                permission_policy: Some(prefs.permission_policy.clone()),
                sandbox_profile: Some(sandbox_profile.as_str().into()),
            };

            let (client, mut events) =
                match AcpClient::spawn_with_options(cli_path, cwd, spawn_opts) {
                    Ok(v) => v,
                    Err(e) => {
                        {
                            let mut guard = self.inner.lock();
                            if let Some(s) = guard.as_mut() {
                                let _ = s.fsm.connect_failed(e);
                            }
                        }
                        let snap = self.snapshot();
                        Self::emit_state(&app, &snap);
                        return Ok(snap);
                    }
                };

            // Event pump tagged with process_id (multi-process routing).
            {
                let mgr = Arc::clone(self);
                let app_ev = app.clone();
                let pid = process_id.clone();
                tokio::spawn(async move {
                    while let Some(ev) = events.recv().await {
                        mgr.handle_acp_event(&app_ev, &pid, ev).await;
                    }
                });
            }
            (client, false, process_id)
        };

        let open_result = if reused_process {
            client.open_session(resume_agent_sid.as_deref()).await
        } else {
            client
                .initialize_and_open_session(resume_agent_sid.as_deref())
                .await
        };

        match open_result {
            Ok((agent_sid, resumed)) => {
                // Align live agent model / product mode with active channel.
                if let Err(e) = client.set_model(&agent_model).await {
                    tracing::warn!("acp set_model after session open soft-fail: {e}");
                }
                if let Err(e) = client.set_mode(&prefs.mode).await {
                    tracing::warn!("acp set_mode after session open soft-fail: {e}");
                }
                // Native resume = full agent context. Fresh session + existing UI
                // journal → bootstrap history into the next prompt.
                let need_bootstrap = !resumed && journal_has_history;
                if resumed {
                    tracing::info!(
                        "agent session resumed id={agent_sid} (full context) warm={reused_process}"
                    );
                } else if need_bootstrap {
                    tracing::info!(
                        "agent session new id={agent_sid}; will bootstrap journal history on first send warm={reused_process}"
                    );
                }
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let _ = s.fsm.handshake_ok();
                        s.acp = Some(client);
                        s.process_id = process_id;
                        s.meta.agent_session_id = Some(agent_sid);
                        s.meta.model_id = Some(prefs.model_id.clone());
                        s.meta.mode = Some(prefs.mode.clone());
                        s.meta.effort = Some(prefs.effort.clone());
                        s.meta.permission_policy = Some(prefs.permission_policy.clone());
                        s.model_id = Some(prefs.model_id.clone());
                        s.effort = Some(prefs.effort.clone());
                        s.product_mode = Some(prefs.mode.clone());
                        s.backend = "grok_agent_stdio".into();
                        s.needs_history_bootstrap = need_bootstrap;
                        Self::touch_activity_locked(s);
                        meta = s.meta.clone();
                    }
                }
                let _ = store::update_session_meta(&meta);
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                Ok(snap)
            }
            Err(e) => {
                client.kill().await;
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let _ = s.fsm.connect_failed(e);
                    }
                }
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                Ok(snap)
            }
        }
    }

    /// Detach a live ACP client when spawn-critical flags match the next connect.
    /// Event pump keeps running on the Arc; caller rebinds into a new LiveSession.
    /// Prefer park+spawn for multi-session; this path keeps a single process for same cwd.
    /// Returns `(process_id, client)` so the event pump tag stays valid.
    fn take_reusable_acp(
        inner: &Mutex<Option<LiveSession>>,
        cwd: &std::path::Path,
        project_path: &Option<String>,
        prefs: &store::ComposerPrefs,
        next_policy: PermissionPolicy,
        sandbox_profile: crate::runtime_compat::SandboxProfileV1,
    ) -> Option<(ProcessId, Arc<AcpClient>)> {
        let mut guard = inner.lock();
        let s = guard.as_mut()?;
        if !matches!(s.fsm.state(), SessionState::Ready) {
            return None;
        }
        if s.streaming_message_id.is_some() {
            return None;
        }
        if s.project_path != *project_path {
            return None;
        }
        let client = s.acp.as_ref()?;
        if !client.is_alive() {
            return None;
        }
        if client.sandbox_application().requested != sandbox_profile.as_str() {
            return None;
        }
        // Effort is a spawn flag — mismatch requires cold respawn.
        if s.effort.as_deref() != Some(prefs.effort.as_str()) {
            return None;
        }
        // YOLO maps to `--always-approve` at spawn time.
        let prev_yolo = s.policy == PermissionPolicy::AlwaysApprove;
        let next_yolo = next_policy == PermissionPolicy::AlwaysApprove;
        if prev_yolo != next_yolo {
            return None;
        }
        // cwd must match the process (project path or orphan fallback).
        if client.cwd() != cwd {
            return None;
        }
        let pid = s.process_id.clone();
        s.acp.take().map(|c| (pid, c))
    }

    async fn connect_sunsetz(
        self: &Arc<Self>,
        app: AppHandle,
        mut meta: SessionMeta,
    ) -> Result<SessionSnapshot, String> {
        let journal_has_history = store::load_messages(&meta.id).iter().any(|m| {
            (m.role == "user" || m.role == "assistant")
                && !m.content.trim().is_empty()
                && !m.is_error
        });
        let agent_sid = Uuid::new_v4().to_string();
        {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                let _ = s.fsm.handshake_ok();
                s.backend = agent_loop::BACKEND_SUNSETZ.into();
                s.acp = None;
                s.meta.agent_session_id = Some(agent_sid);
                s.needs_history_bootstrap = journal_has_history;
                Self::touch_activity_locked(s);
                meta = s.meta.clone();
            }
        }
        let _ = store::update_session_meta(&meta);
        let snap = self.snapshot();
        Self::emit_state(&app, &snap);
        Ok(snap)
    }

    async fn connect_mock(
        self: &Arc<Self>,
        app: AppHandle,
        mode: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let mode = match mode.as_deref() {
            Some("fail_cli_not_found") => MockConnectMode::FailCliNotFound,
            _ => MockConnectMode::Success,
        };
        tokio::time::sleep(Duration::from_millis(80)).await;
        match mode {
            MockConnectMode::Success => {
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let _ = s.fsm.handshake_ok();
                        s.backend = "mock_acp".into();
                    }
                }
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                Ok(snap)
            }
            MockConnectMode::FailCliNotFound => {
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let _ = s.fsm.connect_failed(AgentError::new(
                            AgentErrorCode::CliNotFound,
                            "Mock: CLI not found (GROK_APP_ACP=mock demo)",
                        ));
                        s.backend = "mock_acp".into();
                    }
                }
                let snap = self.snapshot();
                Self::emit_state(&app, &snap);
                Ok(snap)
            }
        }
    }

    async fn handle_acp_event(self: &Arc<Self>, app: &AppHandle, process_id: &str, ev: AcpEvent) {
        // Route events to the focused live session **or** a background busy session
        // (multi-session parallel streaming). Idle parked agents should not emit.
        let live_context = self.inner.lock().as_ref().and_then(|s| {
            (s.process_id == process_id).then(|| {
                (
                    s.app_session_id.clone(),
                    s.meta.agent_session_id.clone(),
                    s.streaming_message_id.clone(),
                )
            })
        });
        let is_live = live_context.is_some();
        let bg_context = if !is_live {
            self.background.lock().iter().find_map(|(id, s)| {
                (s.process_id == process_id).then(|| {
                    (
                        id.clone(),
                        s.meta.agent_session_id.clone(),
                        s.streaming_message_id.clone(),
                    )
                })
            })
        } else {
            None
        };

        if let Some((session_id, agent_session_id, turn_id)) =
            live_context.as_ref().or(bg_context.as_ref())
        {
            crate::runtime_events::emit(
                app,
                session_id,
                agent_session_id.clone(),
                process_id,
                turn_id.clone(),
                &ev,
            );
        }

        if !is_live {
            if let Some((sid, _, _)) = bg_context {
                self.handle_acp_event_on_background(app, &sid, ev).await;
                return;
            }
            if let AcpEvent::ProcessExited { .. } = &ev {
                let mut parked = self.parked.lock();
                parked.retain(|_, p| p.process_id != process_id);
                let mut bg = self.background.lock();
                bg.retain(|_, s| s.process_id != process_id);
            }
            return;
        }

        match ev {
            AcpEvent::Stream {
                kind,
                text,
                message_id,
                done,
            } => {
                let (app_sid, mid, thought_phase) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        // Drop stream chunks that arrive while no turn is in flight.
                        // On session resume (session/load) the CLI may replay the past
                        // transcript as agent_message_chunk notifications; without this
                        // guard they'd be re-emitted as live session://stream and the
                        // UI would re-type the whole history on every session switch.
                        if !matches!(
                            s.fsm.state(),
                            SessionState::Streaming | SessionState::AwaitingPermission
                        ) {
                            tracing::debug!(
                                "acp stream dropped: fsm={:?} (not in a live turn)",
                                s.fsm.state()
                            );
                            return;
                        }
                        // Stream chunk = progress (I06); not pure silence.
                        Self::touch_stream_progress_locked(s);
                        // Prefer the Runtime id until Host splits the turn around a
                        // tool. Post-tool phases retain their Host UUID.
                        Self::ensure_stream_message_id(s, message_id.as_deref(), kind);
                        // Split thinking whenever it resumes after body text so the UI
                        // can interleave thought ↔ content (not stack all thoughts on top).
                        let thought_phase = match kind {
                            StreamKind::Thought => {
                                let phase = if s.stream_last_was_assistant {
                                    if !s.stream_thought.is_empty() {
                                        s.stream_thought.push_str("\n\n⟪phase⟫\n\n");
                                    }
                                    s.stream_last_was_assistant = false;
                                    "new"
                                } else if s.stream_thought.is_empty() {
                                    "open"
                                } else {
                                    "continue"
                                };
                                s.stream_thought.push_str(&text);
                                phase
                            }
                            StreamKind::Assistant => {
                                s.stream_buf.push_str(&text);
                                s.stream_last_was_assistant = true;
                                "none"
                            }
                        };
                        // I04: throttled mid-stream journal (force on terminal done chunk).
                        let para = is_paragraph_break(&text);
                        Self::maybe_flush_stream_journal(s, done, para);
                        (
                            s.app_session_id.clone(),
                            s.streaming_message_id.clone().unwrap_or_default(),
                            thought_phase,
                        )
                    } else {
                        return;
                    }
                };
                let payload = serde_json::json!({
                    "sessionId": app_sid,
                    "messageId": mid,
                    "text": text,
                    "done": done,
                    "kind": match kind {
                        StreamKind::Assistant => "assistant",
                        StreamKind::Thought => "thought",
                    },
                    "thoughtPhase": thought_phase,
                });
                let _ = app.emit("session://stream", payload);
            }
            AcpEvent::PromptComplete { stop_reason } => {
                let (app_sid, finished, empty_run) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        Self::touch_stream_progress_locked(s);
                        s.deferred_prompt_complete = Some(stop_reason.clone());
                        if is_successful_prompt_complete(&stop_reason) {
                            s.allow_auto_wake = true;
                        }
                        // #52: do not Ready the UI while tools / permission / ask_user / plan
                        // are still open — agent often fires prompt_complete early.
                        match Self::try_finish_deferred_prompt_complete(s) {
                            None => {
                                tracing::info!(
                                    "acp prompt_complete deferred stop={stop_reason} tools={} perm={} plan={} ask={}",
                                    s.open_tool_ids.len(),
                                    s.fsm.state() == SessionState::AwaitingPermission,
                                    s.pending_plan.is_some(),
                                    s.pending_ask_user.is_some(),
                                );
                                (s.app_session_id.clone(), false, None)
                            }
                            Some(empty) => (s.app_session_id.clone(), true, empty),
                        }
                    } else {
                        (String::new(), false, None)
                    }
                };
                Self::emit_empty_run_if_any(app, empty_run);
                let woke = if finished && !app_sid.is_empty() {
                    self.maybe_start_wake_turn(app, &app_sid).await
                } else {
                    false
                };
                if !woke {
                    Self::emit_state(app, &self.snapshot());
                }
            }
            AcpEvent::PermissionRequest {
                rpc_id,
                tool_call_id,
                tool_name,
                title,
                options,
                raw,
            } => {
                let preview = raw.to_string();
                let path_target = extract_path_target(&raw);
                let shell_command = extract_shell_command(&raw);
                let (auto, auto_deny, request, snapshot) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        Self::touch_activity_locked(s);
                        let _ = s.fsm.await_permission();
                        // Use live session policy (updated by chip / settings_set / set_policy).
                        // Do NOT re-read only global settings — project/session scope would break.
                        let root = s.project_path.as_ref().map(std::path::PathBuf::from);
                        let sk = permission_scope_key(
                            &tool_name,
                            &path_target,
                            &shell_command,
                            root.as_deref(),
                            &title,
                        );
                        let auto = may_auto_allow(
                            s.policy,
                            &s.allow_cache,
                            &sk,
                            root.as_deref(),
                            &path_target,
                            &tool_name,
                            &shell_command,
                        );
                        let auto_deny = !auto && may_auto_deny(s.policy);
                        let snapshot = InteractionSnapshotV1::new(
                            &s.app_session_id,
                            &s.process_id,
                            rpc_id,
                            Some(tool_call_id),
                            InteractionPayloadV1::Permission {
                                tool_name,
                                title,
                                preview: preview.chars().take(2000).collect(),
                                scope_key: sk,
                                options,
                            },
                        );
                        let pending = PendingPermission {
                            interaction: snapshot.clone(),
                            host_reply: None,
                        };
                        let request = pending.ui_payload();
                        s.pending_permission = Some(pending);
                        (auto, auto_deny, request, snapshot)
                    } else {
                        return;
                    }
                };
                Self::publish_interaction(app, &snapshot);
                let automatic_option = if auto {
                    pick_option_id(&request.options, "allow_once")
                        .or_else(|| pick_option_id(&request.options, "allow_always"))
                        .or_else(|| pick_option_id(&request.options, "allow_command_always"))
                        .or_else(|| pick_option_id(&request.options, "always_allow_all_sessions"))
                        .or_else(|| pick_option_id(&request.options, "allow"))
                        .map(|option_id| ("allow", option_id))
                } else if auto_deny {
                    Some((
                        "deny",
                        pick_option_id(&request.options, "reject_once")
                            .or_else(|| pick_option_id(&request.options, "reject_always"))
                            .or_else(|| pick_option_id(&request.options, "reject"))
                            .or_else(|| pick_option_id(&request.options, "deny"))
                            .unwrap_or_else(|| "reject".into()),
                    ))
                } else {
                    None
                };

                if let Some((decision, option_id)) = automatic_option {
                    if let Err(error) = self
                        .resolve_permission(
                            app.clone(),
                            rpc_id,
                            decision.to_string(),
                            Some(option_id),
                            None,
                            Some(request.session_id.clone()),
                            Some(request.interaction_id.clone()),
                        )
                        .await
                    {
                        tracing::warn!("automatic permission response failed: {error}");
                        let _ = app.emit("session://permission", &request);
                        Self::emit_state(app, &self.snapshot());
                    }
                } else {
                    let _ = app.emit("session://permission", &request);
                    Self::emit_state(app, &self.snapshot());
                }
            }
            AcpEvent::ToolCall {
                tool_call_id,
                title,
                kind,
                status,
                raw,
            } => {
                let media_path = if status == "completed" {
                    extract_generated_media_path(&raw).filter(|p| is_media_fs_path(p))
                } else {
                    None
                };

                let (detail, path_hint) = extract_tool_ui_fields(&raw);
                let path_out = media_path.clone().or(path_hint).filter(|p| !p.is_empty());
                let (before_snip, after_snip) = extract_tool_content_snippets(&raw);

                // The first observation of each tool creates a hard assistant
                // phase boundary. Persist the running row immediately so its
                // journal index is stable before later terminal updates.
                let (boundary_started, completed_phase_id, boundary_sid) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        Self::touch_stream_progress_locked(s);
                        match Self::begin_tool_boundary(s, &tool_call_id) {
                            Some(completed) => (true, completed, s.app_session_id.clone()),
                            None => (false, None, s.app_session_id.clone()),
                        }
                    } else {
                        (false, None, String::new())
                    }
                };
                if boundary_started {
                    persist_tool_step(
                        &boundary_sid,
                        &tool_call_id,
                        "in_progress",
                        &kind,
                        &title,
                        detail.as_deref(),
                        path_out.as_deref(),
                    );
                    if let Some(message_id) = completed_phase_id {
                        let _ = app.emit(
                            "session://stream",
                            serde_json::json!({
                                "sessionId": boundary_sid,
                                "messageId": message_id,
                                "text": "",
                                "done": true,
                                "kind": "assistant",
                                "thoughtPhase": "none",
                                "phaseBoundary": "tool",
                            }),
                        );
                    }
                }

                if let Some(path) = media_path.as_ref() {
                    let att = attachment_from_path(path);
                    let (app_sid, mid) = {
                        let mut guard = self.inner.lock();
                        if let Some(s) = guard.as_mut() {
                            Self::touch_stream_progress_locked(s);
                            if !s.stream_attachments.iter().any(|a| a.path == att.path) {
                                s.stream_attachments.push(att.clone());
                            }
                            (
                                s.app_session_id.clone(),
                                s.streaming_message_id.clone().unwrap_or_default(),
                            )
                        } else {
                            (String::new(), String::new())
                        }
                    };
                    // Keep event name for backward compat; used for image + video.
                    let _ = app.emit(
                        "session://generated_image",
                        serde_json::json!({
                            "sessionId": app_sid,
                            "messageId": mid,
                            "path": att.path,
                            "name": att.name,
                            "toolCallId": tool_call_id,
                            "kind": if is_video_fs_path(path) { "video" } else { "image" },
                        }),
                    );
                }

                let (app_sid, finished, empty_run) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        // Tool events count as progress so long tools never false-stall (I06).
                        Self::touch_stream_progress_locked(s);
                        if !tool_call_id.is_empty() {
                            if is_terminal_tool_status(&status) {
                                s.open_tool_ids.remove(&tool_call_id);
                            } else {
                                s.open_tool_ids.insert(tool_call_id.clone());
                            }
                        }
                        s.tools_this_turn = s.tools_this_turn.saturating_add(1);
                        // Tools settled → apply deferred prompt_complete if any (#52).
                        let finish = Self::try_finish_deferred_prompt_complete(s);
                        (s.app_session_id.clone(), finish.is_some(), finish.flatten())
                    } else {
                        (String::new(), false, None)
                    }
                };
                Self::emit_empty_run_if_any(app, empty_run);

                // Live tool activity for UI — prefer human call text over bare "tool".
                let live_title = if !title.is_empty() && title.to_ascii_lowercase() != "tool" {
                    title.clone()
                } else if let Some(ref d) = detail {
                    d.clone()
                } else if let Some(ref p) = path_out {
                    p.clone()
                } else if !kind.is_empty() && kind.to_ascii_lowercase() != "tool" {
                    kind.replace('_', " ")
                } else {
                    String::new()
                };
                let _ = app.emit(
                    "session://tool",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "toolCallId": tool_call_id,
                        "title": live_title,
                        "kind": kind,
                        "status": if status.is_empty() { "in_progress" } else { &status },
                        "path": path_out,
                        "detail": detail,
                        // Optional content snippets for the session Changes / diff panel.
                        "before": before_snip,
                        "after": after_snip,
                    }),
                );

                // Update the row in place for every status. On first sight this
                // follows the explicit in_progress insert above; terminal-only
                // Runtime events therefore still get the correct journal order.
                let st = if status.is_empty() {
                    "in_progress"
                } else {
                    status.as_str()
                };
                persist_tool_step(
                    &app_sid,
                    &tool_call_id,
                    st,
                    &kind,
                    &title,
                    detail.as_deref(),
                    path_out.as_deref(),
                );
                if finished {
                    let woke = self.maybe_start_wake_turn(app, &app_sid).await;
                    if !woke {
                        Self::emit_state(app, &self.snapshot());
                    }
                }
            }
            AcpEvent::Plan {
                entries,
                body,
                rpc_id,
                tool_call_id,
            } => {
                let (app_sid, process_id, interaction) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let interaction = rpc_id.map(|id| {
                            let snapshot = InteractionSnapshotV1::new(
                                &s.app_session_id,
                                &s.process_id,
                                id,
                                tool_call_id.clone(),
                                InteractionPayloadV1::Plan {
                                    entries: entries.clone(),
                                    body: body.clone(),
                                },
                            );
                            s.pending_plan = Some(PendingPlan {
                                interaction: snapshot.clone(),
                            });
                            snapshot
                        });
                        (s.app_session_id.clone(), s.process_id.clone(), interaction)
                    } else {
                        (String::new(), String::new(), None)
                    }
                };
                if !app_sid.is_empty() {
                    let record = crate::plan_artifacts::RuntimePlanArtifactRecordV1 {
                        version: 1,
                        session_id: app_sid.clone(),
                        process_id,
                        interaction_id: interaction
                            .as_ref()
                            .map(|snapshot| snapshot.interaction_id.clone()),
                        tool_call_id: tool_call_id.clone(),
                        body: body.clone(),
                        entries: entries.clone(),
                        awaiting_review: rpc_id.is_some(),
                    };
                    match crate::plan_artifacts::record_runtime_plan(record) {
                        Ok(artifact) => {
                            Self::emit_plan_artifact(app, &artifact);
                        }
                        Err(error) => {
                            tracing::warn!(
                                session_id = app_sid,
                                "persist Plan artifact failed: {error}"
                            );
                        }
                    }
                }
                if let Some(interaction) = interaction.as_ref() {
                    Self::publish_interaction(app, interaction);
                }
                Self::emit_plan_compat(
                    app,
                    &app_sid,
                    &entries,
                    &body,
                    rpc_id,
                    &tool_call_id,
                    interaction
                        .as_ref()
                        .map(|snapshot| snapshot.interaction_id.as_str()),
                );
            }
            AcpEvent::AskUserQuestion {
                rpc_id,
                tool_call_id,
                questions,
                raw,
            } => {
                let activity_id = ask_user_activity_id(tool_call_id.as_deref(), rpc_id);
                let question_count = questions.len();
                let (payload, completed_phase_id, app_session_id, interaction) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        Self::touch_stream_progress_locked(s);
                        let completed_phase_id = Self::begin_tool_boundary(s, &activity_id);
                        let interaction = InteractionSnapshotV1::new(
                            &s.app_session_id,
                            &s.process_id,
                            rpc_id,
                            tool_call_id.clone(),
                            InteractionPayloadV1::AskUser {
                                questions: questions.clone(),
                                partial_answers: None,
                            },
                        );
                        s.pending_ask_user = Some(PendingAskUser {
                            interaction: interaction.clone(),
                            rpc_id,
                            tool_call_id,
                            activity_id: activity_id.clone(),
                            questions,
                            partial_answers: None,
                            raw,
                            resolving: false,
                            host_reply: None,
                        });
                        let payload = s
                            .pending_ask_user
                            .as_ref()
                            .map(|pending| pending.ui_payload(&s.app_session_id));
                        (
                            payload,
                            completed_phase_id,
                            s.app_session_id.clone(),
                            Some(interaction),
                        )
                    } else {
                        (None, None, String::new(), None)
                    }
                };
                if let Some(interaction) = interaction.as_ref() {
                    Self::publish_interaction(app, interaction);
                }
                if let Some(payload) = payload {
                    if let Some(Some(message_id)) = completed_phase_id {
                        let _ = app.emit(
                            "session://stream",
                            serde_json::json!({
                                "sessionId": app_session_id,
                                "messageId": message_id,
                                "text": "",
                                "done": true,
                                "kind": "assistant",
                                "thoughtPhase": "none",
                                "phaseBoundary": "ask_user",
                            }),
                        );
                    }
                    record_ask_user_activity(
                        app,
                        &payload.session_id,
                        &activity_id,
                        "in_progress",
                        question_count,
                        None,
                    );
                    let _ = app.emit("session://ask_user", &payload);
                }
            }
            AcpEvent::Error { error } => {
                let (ask_activity, interrupted) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        if !s.provider_retry_aborted {
                            Self::record_turn_error(s, app, &error);
                        }
                        settle_active_skill_uses(
                            s,
                            crate::skill_feedback::SkillUseStatusV1::Failed,
                        );
                        let _ = s.fsm.fail_with(error);
                        let interrupted = interrupt_pending_interactions(s);
                        (take_pending_ask_activity(s), interrupted)
                    } else {
                        (None, InterruptedSessionGates::empty())
                    }
                };
                Self::publish_interrupted_session_gates(app, interrupted);
                if let Some(activity) = ask_activity {
                    record_ask_user_activity(
                        app,
                        &activity.session_id,
                        &activity.activity_id,
                        "failed",
                        activity.question_count,
                        None,
                    );
                }
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::ProcessExited { .. } => {
                let (ask_activity, interrupted) = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        let st = s.fsm.state();
                        if matches!(
                            st,
                            SessionState::Streaming | SessionState::AwaitingPermission
                        ) {
                            // I04: flush partial assistant before cancel marker.
                            Self::maybe_flush_stream_journal(s, true, false);
                            let mid = Uuid::new_v4().to_string();
                            let content = "turn_cancelled|agent_exit".to_string();
                            let _ = store::append_message(
                                &s.app_session_id,
                                ChatMessageStored {
                                    id: mid.clone(),
                                    role: "tool".into(),
                                    content: content.clone(),
                                    thought: None,
                                    created_at: chrono::Utc::now(),
                                    is_error: true,
                                    attachments: None,
                                    marker: Some("turn_cancelled".into()),
                                },
                            );
                            let _ = app.emit(
                                "session://turn_marker",
                                serde_json::json!({
                                    "sessionId": s.app_session_id,
                                    "messageId": mid,
                                    "marker": "turn_cancelled",
                                    "reason": "agent_exit",
                                    "content": content,
                                }),
                            );
                        }
                        // During Connecting, leave error to initialize/connect_failed
                        // (fail_all_pending already surfaces a richer stderr-backed message).
                        let has_err = s.fsm.last_error().is_some();
                        if !has_err
                            && matches!(
                                st,
                                SessionState::Ready
                                    | SessionState::Streaming
                                    | SessionState::AwaitingPermission
                            )
                        {
                            let _ = s.fsm.crash("Agent process exited");
                        }
                        settle_active_skill_uses(
                            s,
                            crate::skill_feedback::SkillUseStatusV1::Interrupted,
                        );
                        s.acp = None;
                        let interrupted = interrupt_pending_interactions(s);
                        (take_pending_ask_activity(s), interrupted)
                    } else {
                        (None, InterruptedSessionGates::empty())
                    }
                };
                Self::publish_interrupted_session_gates(app, interrupted);
                if let Some(activity) = ask_activity {
                    record_ask_user_activity(
                        app,
                        &activity.session_id,
                        &activity.activity_id,
                        "failed",
                        activity.question_count,
                        None,
                    );
                }
                // Also drop any parked entry with this process id (defensive).
                self.parked.lock().retain(|_, p| p.process_id != process_id);
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::State {
                backend,
                agent_session_id,
                model_id,
            } => {
                {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        s.backend = backend;
                        if let Some(id) = agent_session_id {
                            s.meta.agent_session_id = Some(id);
                        }
                        if model_id.is_some() {
                            s.model_id = model_id;
                        }
                    }
                }
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::Stderr { line } => {
                let _ = app.emit("session://stderr", serde_json::json!({ "line": line }));
            }
            AcpEvent::RetryState {
                attempt,
                max_retries,
                reason,
                status,
            } => {
                let cap = max_retries.min(HOST_PROVIDER_MAX_RETRIES).max(1);
                let abort = {
                    let mut guard = self.inner.lock();
                    if let Some(s) = guard.as_mut() {
                        s.provider_retry_attempt = attempt;
                        if s.provider_retry_aborted {
                            false
                        } else {
                            should_abort_provider_retry(attempt, max_retries, &status)
                        }
                    } else {
                        false
                    }
                };

                let _ = app.emit(
                    "session://retry",
                    serde_json::json!({
                        "attempt": attempt,
                        "maxRetries": cap,
                        "reason": reason,
                        "status": status,
                        "aborting": abort,
                    }),
                );

                if abort {
                    let acp = {
                        let mut guard = self.inner.lock();
                        if let Some(s) = guard.as_mut() {
                            if s.provider_retry_aborted {
                                None
                            } else {
                                s.provider_retry_aborted = true;
                                let msg = if reason.trim().is_empty() {
                                    format!(
                                        "Provider request failed after {cap} retries (attempt {attempt})"
                                    )
                                } else {
                                    format!(
                                        "Provider request failed after {cap} retries (attempt {attempt}): {reason}"
                                    )
                                };
                                let err = AgentError::new(AgentErrorCode::NetworkProvider, msg);
                                // Chat-visible error row (must happen before clearing stream ids)
                                Self::record_turn_error(s, app, &err);
                                let _ = s.fsm.fail_with(err);
                                s.acp.clone()
                            }
                        } else {
                            None
                        }
                    };
                    if let Some(acp) = acp {
                        let abort_msg = format!(
                            "provider retries exhausted (host cap {HOST_PROVIDER_MAX_RETRIES})"
                        );
                        acp.abort_pending_prompts(&abort_msg);
                        let _ = acp.cancel().await;
                    }
                    Self::emit_state(app, &self.snapshot());
                }
            }
            AcpEvent::ContextCompact {
                trigger,
                tokens_before,
                tokens_after,
                summary_preview,
                note,
            } => {
                let message_id = Uuid::new_v4().to_string();
                let content = context_compact_content(
                    &trigger,
                    tokens_before,
                    tokens_after,
                    summary_preview.as_deref(),
                    note.as_deref(),
                );
                let (app_sid, completed_phase_id) = {
                    let mut guard = self.inner.lock();
                    let Some(s) = guard.as_mut() else {
                        return;
                    };
                    Self::touch_stream_progress_locked(s);
                    (
                        s.app_session_id.clone(),
                        Self::begin_context_compact_boundary(s),
                    )
                };
                persist_context_compact(&app_sid, &message_id, &content);
                if let Some(completed_phase_id) = completed_phase_id {
                    let _ = app.emit(
                        "session://stream",
                        serde_json::json!({
                            "sessionId": &app_sid,
                            "messageId": completed_phase_id,
                            "text": "",
                            "done": true,
                            "kind": "assistant",
                            "thoughtPhase": "none",
                            "phaseBoundary": "context_compact",
                        }),
                    );
                }
                let _ = app.emit(
                    "session://context_compact",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "messageId": message_id,
                        "trigger": trigger,
                        "tokensBefore": tokens_before,
                        "tokensAfter": tokens_after,
                        "summaryPreview": summary_preview,
                        "note": note,
                        "content": content,
                    }),
                );
            }
            AcpEvent::Usage {
                input_tokens,
                output_tokens,
                cached_read_tokens,
                reasoning_tokens,
                model_calls,
                model_id,
            } => {
                let saved = {
                    let mut guard = self.inner.lock();
                    let Some(s) = guard.as_mut() else {
                        return;
                    };
                    let Some(usage) = build_runtime_context_usage(
                        &s.meta,
                        &s.backend,
                        input_tokens,
                        output_tokens,
                        cached_read_tokens,
                        reasoning_tokens,
                        model_calls,
                        model_id,
                    ) else {
                        tracing::warn!(
                            "context usage: no exact per-inference measurement for session={}",
                            s.app_session_id
                        );
                        return;
                    };
                    s.meta.context_usage = Some(usage.clone());
                    let _ = store::update_session_meta(&s.meta);
                    (s.app_session_id.clone(), usage)
                };
                let _ = app.emit(
                    "session://context_usage",
                    serde_json::json!({
                        "sessionId": saved.0,
                        "usage": saved.1,
                    }),
                );
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::Unknown { method, .. } => {
                tracing::debug!("runtime event retained as unknown method={method}");
            }
        }
    }

    /// Apply ACP events for a session demoted to background (still streaming).
    /// Emits the same `session://*` events with that session's id so the UI can
    /// update caches without focus, and permissions are not applied to the wrong chat.
    async fn handle_acp_event_on_background(
        self: &Arc<Self>,
        app: &AppHandle,
        app_session_id: &str,
        ev: AcpEvent,
    ) {
        match ev {
            AcpEvent::Stream {
                kind,
                text,
                message_id,
                done,
            } => {
                let (app_sid, mid, thought_phase) = {
                    let mut bg = self.background.lock();
                    let Some(s) = bg.get_mut(app_session_id) else {
                        return;
                    };
                    if !matches!(
                        s.fsm.state(),
                        SessionState::Streaming | SessionState::AwaitingPermission
                    ) {
                        return;
                    }
                    Self::touch_stream_progress_locked(s);
                    Self::ensure_stream_message_id(s, message_id.as_deref(), kind);
                    let thought_phase = match kind {
                        StreamKind::Thought => {
                            let phase = if s.stream_last_was_assistant {
                                if !s.stream_thought.is_empty() {
                                    s.stream_thought.push_str("\n\n⟪phase⟫\n\n");
                                }
                                s.stream_last_was_assistant = false;
                                "new"
                            } else if s.stream_thought.is_empty() {
                                "open"
                            } else {
                                "continue"
                            };
                            s.stream_thought.push_str(&text);
                            phase
                        }
                        StreamKind::Assistant => {
                            s.stream_buf.push_str(&text);
                            s.stream_last_was_assistant = true;
                            "none"
                        }
                    };
                    let para = is_paragraph_break(&text);
                    Self::maybe_flush_stream_journal(s, done, para);
                    (
                        s.app_session_id.clone(),
                        s.streaming_message_id.clone().unwrap_or_default(),
                        thought_phase,
                    )
                };
                let _ = app.emit(
                    "session://stream",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "messageId": mid,
                        "text": text,
                        "done": done,
                        "kind": match kind {
                            StreamKind::Assistant => "assistant",
                            StreamKind::Thought => "thought",
                        },
                        "thoughtPhase": thought_phase,
                    }),
                );
            }
            AcpEvent::PromptComplete { stop_reason } => {
                let (finished, empty_run) = {
                    let mut bg = self.background.lock();
                    if let Some(s) = bg.get_mut(app_session_id) {
                        Self::touch_stream_progress_locked(s);
                        s.deferred_prompt_complete = Some(stop_reason.clone());
                        if is_successful_prompt_complete(&stop_reason) {
                            s.allow_auto_wake = true;
                        }
                        match Self::try_finish_deferred_prompt_complete(s) {
                            Some(empty) => (true, empty),
                            None => {
                                tracing::info!(
                                    "background prompt_complete deferred sid={} stop={} tools={} perm={} plan={} ask={}",
                                    s.app_session_id,
                                    stop_reason,
                                    s.open_tool_ids.len(),
                                    s.fsm.state() == SessionState::AwaitingPermission,
                                    s.pending_plan.is_some(),
                                    s.pending_ask_user.is_some(),
                                );
                                (false, None)
                            }
                        }
                    } else {
                        (false, None)
                    }
                };
                Self::emit_empty_run_if_any(app, empty_run);
                let woke = if finished {
                    self.maybe_start_wake_turn(app, app_session_id).await
                } else {
                    false
                };
                if finished && !woke {
                    self.promote_background_ready_to_parked(app_session_id);
                }
                if !woke {
                    // Snapshot is focused live — still emit so sidebar busy flags can refresh.
                    Self::emit_state(app, &self.snapshot());
                }
            }
            AcpEvent::PermissionRequest {
                rpc_id,
                tool_call_id,
                tool_name,
                title,
                options,
                raw,
            } => {
                let preview = raw.to_string();
                let path_target = extract_path_target(&raw);
                let shell_command = extract_shell_command(&raw);
                let (auto, auto_deny, request, snapshot) = {
                    let mut bg = self.background.lock();
                    if let Some(s) = bg.get_mut(app_session_id) {
                        Self::touch_activity_locked(s);
                        let _ = s.fsm.await_permission();
                        let root = s.project_path.as_ref().map(std::path::PathBuf::from);
                        let sk = permission_scope_key(
                            &tool_name,
                            &path_target,
                            &shell_command,
                            root.as_deref(),
                            &title,
                        );
                        let auto = may_auto_allow(
                            s.policy,
                            &s.allow_cache,
                            &sk,
                            root.as_deref(),
                            &path_target,
                            &tool_name,
                            &shell_command,
                        );
                        let auto_deny = may_auto_deny(s.policy) && !auto;
                        let snapshot = InteractionSnapshotV1::new(
                            &s.app_session_id,
                            &s.process_id,
                            rpc_id,
                            Some(tool_call_id),
                            InteractionPayloadV1::Permission {
                                tool_name,
                                title,
                                preview: preview.chars().take(2000).collect(),
                                scope_key: sk,
                                options,
                            },
                        );
                        let pending = PendingPermission {
                            interaction: snapshot.clone(),
                            host_reply: None,
                        };
                        let request = pending.ui_payload();
                        s.pending_permission = Some(pending);
                        (auto, auto_deny, request, snapshot)
                    } else {
                        return;
                    }
                };
                Self::publish_interaction(app, &snapshot);
                let automatic_option = if auto {
                    pick_option_id(&request.options, "allow_once")
                        .or_else(|| pick_option_id(&request.options, "allow_always"))
                        .or_else(|| pick_option_id(&request.options, "allow_command_always"))
                        .or_else(|| pick_option_id(&request.options, "always_allow_all_sessions"))
                        .or_else(|| pick_option_id(&request.options, "allow"))
                        .map(|option_id| ("allow", option_id))
                } else if auto_deny {
                    Some((
                        "deny",
                        pick_option_id(&request.options, "reject_once")
                            .or_else(|| pick_option_id(&request.options, "reject_always"))
                            .or_else(|| pick_option_id(&request.options, "reject"))
                            .or_else(|| pick_option_id(&request.options, "deny"))
                            .unwrap_or_else(|| "reject".into()),
                    ))
                } else {
                    None
                };
                if let Some((decision, option_id)) = automatic_option {
                    if let Err(error) = self
                        .resolve_permission(
                            app.clone(),
                            rpc_id,
                            decision.to_string(),
                            Some(option_id),
                            None,
                            Some(request.session_id.clone()),
                            Some(request.interaction_id.clone()),
                        )
                        .await
                    {
                        tracing::warn!("automatic background permission response failed: {error}");
                        let _ = app.emit("session://permission", &request);
                        let _ = app.emit(
                            "session://background_permission",
                            serde_json::json!({ "sessionId": request.session_id }),
                        );
                        Self::emit_state(app, &self.snapshot());
                    }
                } else {
                    let _ = app.emit("session://permission", &request);
                    let _ = app.emit(
                        "session://background_permission",
                        serde_json::json!({ "sessionId": request.session_id }),
                    );
                    Self::emit_state(app, &self.snapshot());
                }
            }
            AcpEvent::ToolCall {
                tool_call_id,
                title,
                kind,
                status,
                raw,
            } => {
                let media_path = if status == "completed" {
                    extract_generated_media_path(&raw).filter(|path| is_media_fs_path(path))
                } else {
                    None
                };
                let (detail, path_hint) = extract_tool_ui_fields(&raw);
                let path_out = media_path
                    .clone()
                    .or(path_hint)
                    .filter(|path| !path.is_empty());
                let (before_snip, after_snip) = extract_tool_content_snippets(&raw);

                let (boundary_started, completed_phase_id, boundary_sid) = {
                    let mut bg = self.background.lock();
                    if let Some(s) = bg.get_mut(app_session_id) {
                        Self::touch_stream_progress_locked(s);
                        match Self::begin_tool_boundary(s, &tool_call_id) {
                            Some(completed) => (true, completed, s.app_session_id.clone()),
                            None => (false, None, s.app_session_id.clone()),
                        }
                    } else {
                        return;
                    }
                };
                if boundary_started {
                    persist_tool_step(
                        &boundary_sid,
                        &tool_call_id,
                        "in_progress",
                        &kind,
                        &title,
                        detail.as_deref(),
                        path_out.as_deref(),
                    );
                    if let Some(message_id) = completed_phase_id {
                        let _ = app.emit(
                            "session://stream",
                            serde_json::json!({
                                "sessionId": boundary_sid,
                                "messageId": message_id,
                                "text": "",
                                "done": true,
                                "kind": "assistant",
                                "thoughtPhase": "none",
                                "phaseBoundary": "tool",
                            }),
                        );
                    }
                }

                if let Some(path) = media_path.as_ref() {
                    let attachment = attachment_from_path(path);
                    let (session_id, message_id) = {
                        let mut bg = self.background.lock();
                        let Some(s) = bg.get_mut(app_session_id) else {
                            return;
                        };
                        if !s
                            .stream_attachments
                            .iter()
                            .any(|existing| existing.path == attachment.path)
                        {
                            s.stream_attachments.push(attachment.clone());
                        }
                        (
                            s.app_session_id.clone(),
                            s.streaming_message_id.clone().unwrap_or_default(),
                        )
                    };
                    let _ = app.emit(
                        "session://generated_image",
                        serde_json::json!({
                            "sessionId": session_id,
                            "messageId": message_id,
                            "path": attachment.path,
                            "name": attachment.name,
                            "toolCallId": tool_call_id,
                            "kind": if is_video_fs_path(path) { "video" } else { "image" },
                        }),
                    );
                }

                let live_title = if !title.is_empty() {
                    title.clone()
                } else if let Some(ref detail) = detail {
                    detail.clone()
                } else if let Some(ref path) = path_out {
                    path.clone()
                } else {
                    kind.clone()
                };
                let st = if status.is_empty() {
                    "in_progress"
                } else {
                    status.as_str()
                };
                let (app_sid, finished, empty_run) = {
                    let mut bg = self.background.lock();
                    let Some(s) = bg.get_mut(app_session_id) else {
                        return;
                    };
                    Self::touch_stream_progress_locked(s);
                    if !tool_call_id.is_empty() {
                        if is_terminal_tool_status(&status) {
                            s.open_tool_ids.remove(&tool_call_id);
                        } else {
                            s.open_tool_ids.insert(tool_call_id.clone());
                        }
                    }
                    s.tools_this_turn = s.tools_this_turn.saturating_add(1);
                    let finish = Self::try_finish_deferred_prompt_complete(s);
                    (s.app_session_id.clone(), finish.is_some(), finish.flatten())
                };
                Self::emit_empty_run_if_any(app, empty_run);
                let _ = app.emit(
                    "session://tool",
                    serde_json::json!({
                        "sessionId": app_sid,
                        "toolCallId": tool_call_id,
                        "title": live_title,
                        "kind": kind,
                        "status": st,
                        "path": path_out,
                        "detail": detail,
                        "before": before_snip,
                        "after": after_snip,
                    }),
                );
                persist_tool_step(
                    &app_sid,
                    &tool_call_id,
                    st,
                    &kind,
                    &title,
                    detail.as_deref(),
                    path_out.as_deref(),
                );
                if finished {
                    let woke = self.maybe_start_wake_turn(app, app_session_id).await;
                    if !woke {
                        self.promote_background_ready_to_parked(app_session_id);
                        Self::emit_state(app, &self.snapshot());
                    }
                }
            }
            AcpEvent::Plan {
                entries,
                body,
                rpc_id,
                tool_call_id,
            } => {
                let (session_id, process_id, interaction) = {
                    let mut background = self.background.lock();
                    let Some(session) = background.get_mut(app_session_id) else {
                        return;
                    };
                    let interaction = rpc_id.map(|id| {
                        let snapshot = InteractionSnapshotV1::new(
                            &session.app_session_id,
                            &session.process_id,
                            id,
                            tool_call_id.clone(),
                            InteractionPayloadV1::Plan {
                                entries: entries.clone(),
                                body: body.clone(),
                            },
                        );
                        session.pending_plan = Some(PendingPlan {
                            interaction: snapshot.clone(),
                        });
                        snapshot
                    });
                    (
                        session.app_session_id.clone(),
                        session.process_id.clone(),
                        interaction,
                    )
                };
                let record = crate::plan_artifacts::RuntimePlanArtifactRecordV1 {
                    version: 1,
                    session_id: session_id.clone(),
                    process_id,
                    interaction_id: interaction
                        .as_ref()
                        .map(|snapshot| snapshot.interaction_id.clone()),
                    tool_call_id: tool_call_id.clone(),
                    body: body.clone(),
                    entries: entries.clone(),
                    awaiting_review: rpc_id.is_some(),
                };
                match crate::plan_artifacts::record_runtime_plan(record) {
                    Ok(artifact) => {
                        Self::emit_plan_artifact(app, &artifact);
                    }
                    Err(error) => {
                        tracing::warn!(
                            session_id,
                            "persist background Plan artifact failed: {error}"
                        );
                    }
                }
                if let Some(interaction) = interaction.as_ref() {
                    Self::publish_interaction(app, interaction);
                }
                let payload = serde_json::json!({
                    "sessionId": session_id,
                    "entries": entries,
                    "body": body,
                    "rpcId": rpc_id,
                    "toolCallId": tool_call_id,
                    "waiting": rpc_id.is_none(),
                    "interactionId": interaction
                        .as_ref()
                        .map(|snapshot| snapshot.interaction_id.clone()),
                });
                let _ = app.emit("session://plan", &payload);
                let _ = app.emit("session://background_plan", &payload);
            }
            AcpEvent::AskUserQuestion {
                rpc_id,
                tool_call_id,
                questions,
                raw,
            } => {
                let activity_id = ask_user_activity_id(tool_call_id.as_deref(), rpc_id);
                let question_count = questions.len();
                let (payload, completed_phase_id, interaction) = {
                    let mut background = self.background.lock();
                    let Some(session) = background.get_mut(app_session_id) else {
                        return;
                    };
                    Self::touch_stream_progress_locked(session);
                    let completed_phase_id = Self::begin_tool_boundary(session, &activity_id);
                    let interaction = InteractionSnapshotV1::new(
                        &session.app_session_id,
                        &session.process_id,
                        rpc_id,
                        tool_call_id.clone(),
                        InteractionPayloadV1::AskUser {
                            questions: questions.clone(),
                            partial_answers: None,
                        },
                    );
                    session.pending_ask_user = Some(PendingAskUser {
                        interaction: interaction.clone(),
                        rpc_id,
                        tool_call_id,
                        activity_id: activity_id.clone(),
                        questions,
                        partial_answers: None,
                        raw,
                        resolving: false,
                        host_reply: None,
                    });
                    let payload = session
                        .pending_ask_user
                        .as_ref()
                        .map(|pending| pending.ui_payload(&session.app_session_id));
                    (payload, completed_phase_id, Some(interaction))
                };
                if let Some(interaction) = interaction.as_ref() {
                    Self::publish_interaction(app, interaction);
                }
                if let Some(payload) = payload {
                    if let Some(Some(message_id)) = completed_phase_id {
                        let _ = app.emit(
                            "session://stream",
                            serde_json::json!({
                                "sessionId": &payload.session_id,
                                "messageId": message_id,
                                "text": "",
                                "done": true,
                                "kind": "assistant",
                                "thoughtPhase": "none",
                                "phaseBoundary": "ask_user",
                            }),
                        );
                    }
                    record_ask_user_activity(
                        app,
                        &payload.session_id,
                        &activity_id,
                        "in_progress",
                        question_count,
                        None,
                    );
                    let _ = app.emit("session://ask_user", &payload);
                    let _ = app.emit(
                        "session://background_ask_user",
                        serde_json::json!({ "sessionId": payload.session_id }),
                    );
                }
            }
            AcpEvent::ProcessExited { .. } => {
                let (ask_activity, interrupted) = {
                    let mut bg = self.background.lock();
                    if let Some(mut s) = bg.remove(app_session_id) {
                        settle_active_skill_uses(
                            &mut s,
                            crate::skill_feedback::SkillUseStatusV1::Interrupted,
                        );
                        let _ = s.fsm.crash("Agent process exited (background)");
                        s.acp = None;
                        let interrupted = interrupt_pending_interactions(&mut s);
                        (take_pending_ask_activity(&mut s), interrupted)
                    } else {
                        (None, InterruptedSessionGates::empty())
                    }
                };
                Self::publish_interrupted_session_gates(app, interrupted);
                if let Some(activity) = ask_activity {
                    record_ask_user_activity(
                        app,
                        &activity.session_id,
                        &activity.activity_id,
                        "failed",
                        activity.question_count,
                        None,
                    );
                }
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::Error { error } => {
                let (ask_activity, interrupted) = {
                    let mut bg = self.background.lock();
                    if let Some(s) = bg.get_mut(app_session_id) {
                        Self::record_turn_error(s, app, &error);
                        settle_active_skill_uses(
                            s,
                            crate::skill_feedback::SkillUseStatusV1::Failed,
                        );
                        let _ = s.fsm.fail_with(error);
                        let interrupted = interrupt_pending_interactions(s);
                        (take_pending_ask_activity(s), interrupted)
                    } else {
                        (None, InterruptedSessionGates::empty())
                    }
                };
                Self::publish_interrupted_session_gates(app, interrupted);
                if let Some(activity) = ask_activity {
                    record_ask_user_activity(
                        app,
                        &activity.session_id,
                        &activity.activity_id,
                        "failed",
                        activity.question_count,
                        None,
                    );
                }
                self.promote_background_ready_to_parked(app_session_id);
                Self::emit_state(app, &self.snapshot());
            }
            AcpEvent::ContextCompact {
                trigger,
                tokens_before,
                tokens_after,
                summary_preview,
                note,
            } => {
                let message_id = Uuid::new_v4().to_string();
                let content = context_compact_content(
                    &trigger,
                    tokens_before,
                    tokens_after,
                    summary_preview.as_deref(),
                    note.as_deref(),
                );
                let (session_id, completed_phase_id) = {
                    let mut background = self.background.lock();
                    let Some(session) = background.get_mut(app_session_id) else {
                        return;
                    };
                    Self::touch_stream_progress_locked(session);
                    (
                        session.app_session_id.clone(),
                        Self::begin_context_compact_boundary(session),
                    )
                };
                persist_context_compact(&session_id, &message_id, &content);
                if let Some(completed_phase_id) = completed_phase_id {
                    let _ = app.emit(
                        "session://stream",
                        serde_json::json!({
                            "sessionId": &session_id,
                            "messageId": completed_phase_id,
                            "text": "",
                            "done": true,
                            "kind": "assistant",
                            "thoughtPhase": "none",
                            "phaseBoundary": "context_compact",
                        }),
                    );
                }
                let _ = app.emit(
                    "session://context_compact",
                    serde_json::json!({
                        "sessionId": session_id,
                        "messageId": message_id,
                        "trigger": trigger,
                        "tokensBefore": tokens_before,
                        "tokensAfter": tokens_after,
                        "summaryPreview": summary_preview,
                        "note": note,
                        "content": content,
                    }),
                );
            }
            AcpEvent::Usage {
                input_tokens,
                output_tokens,
                cached_read_tokens,
                reasoning_tokens,
                model_calls,
                model_id,
            } => {
                let saved = {
                    let mut bg = self.background.lock();
                    let Some(s) = bg.get_mut(app_session_id) else {
                        return;
                    };
                    let Some(usage) = build_runtime_context_usage(
                        &s.meta,
                        &s.backend,
                        input_tokens,
                        output_tokens,
                        cached_read_tokens,
                        reasoning_tokens,
                        model_calls,
                        model_id,
                    ) else {
                        return;
                    };
                    s.meta.context_usage = Some(usage.clone());
                    let _ = store::update_session_meta(&s.meta);
                    usage
                };
                let _ = app.emit(
                    "session://context_usage",
                    serde_json::json!({
                        "sessionId": app_session_id,
                        "usage": saved,
                    }),
                );
            }
            _ => {
                // ask_user / plan / stderr / retry — still forward with session id when possible
                tracing::debug!("background acp event ignored variant for sid={app_session_id}");
            }
        }
    }

    /// Drop the last user turn (and everything after) on the agent + local journal.
    /// Used before re-sending an edited last user message so the previous assistant
    /// reply is replaced, not stacked.
    ///
    /// Agent path: `x.ai/rewind/execute` (Grok Build extension).
    /// Local path: truncate `messages.json` to keep only messages before the last user row.
    pub async fn rewind_drop_last_user_turn(
        self: &Arc<Self>,
        app: AppHandle,
    ) -> Result<SessionSnapshot, String> {
        let (backend, app_sid, acp, user_prompt_count) = {
            let guard = self.inner.lock();
            let s = guard.as_ref().ok_or("no active session")?;
            if s.fsm.state() == SessionState::Streaming
                || s.fsm.state() == SessionState::AwaitingPermission
            {
                return Err("cannot edit while a turn is running".into());
            }
            let msgs = store::load_messages(&s.app_session_id);
            let user_prompt_count = msgs.iter().filter(|m| m.role == "user").count() as u32;
            if user_prompt_count == 0 {
                return Err("no user message to rewind".into());
            }
            (
                s.backend.clone(),
                s.app_session_id.clone(),
                s.acp.clone(),
                user_prompt_count,
            )
        };

        // Agent: discard last user turn. TUI semantics keep the selected turn and drop after;
        // so for "drop last user" we target the previous turn when count > 1.
        // When count == 1, execute target 0 with best-effort; host journal is the source of truth for UI.
        if backend != "mock_acp" && !AcpClient::use_mock() {
            if let Some(client) = acp {
                let target = user_prompt_count.saturating_sub(1);
                // Prefer rewinding to previous turn (keep 0..n-2, drop n-1..).
                // When only one user turn: try target 0 then clear local journal fully.
                let exec_index = if user_prompt_count <= 1 {
                    0u32
                } else {
                    // Keep through previous user turn → drop last.
                    user_prompt_count - 2
                };
                match client.rewind_execute(exec_index, false).await {
                    Ok(_) => {
                        tracing::info!(
                            target: "session",
                            "rewind_drop_last_user_turn: agent rewound target={exec_index} (user_turns={user_prompt_count})"
                        );
                    }
                    Err(e) => {
                        // Fallback: try targeting the last turn itself (some builds discard at/after index).
                        tracing::warn!(
                            target: "session",
                            error = %e,
                            "rewind_execute({exec_index}) failed; trying last-turn index {target}"
                        );
                        if let Err(e2) = client.rewind_execute(target, false).await {
                            tracing::warn!(
                                target: "session",
                                error = %e2,
                                "agent rewind failed; local journal still truncated"
                            );
                        }
                    }
                }
            }
        }

        // Local journal: keep messages strictly before the last user message.
        store::update_messages(&app_sid, |msgs| {
            let mut cut = msgs.len();
            for (i, m) in msgs.iter().enumerate().rev() {
                if m.role == "user" {
                    cut = i;
                    break;
                }
            }
            msgs.truncate(cut);
            Ok(())
        })?;

        {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                s.meta.updated_at = chrono::Utc::now();
                let _ = store::update_session_meta(&s.meta);
            }
        }
        let snap = self.snapshot();
        Self::emit_state(&app, &snap);
        Ok(snap)
    }

    /// List rewind points for an app session journal (one per user prompt).
    /// Prefer the local journal so the UI timeline always matches what the user sees.
    pub fn list_rewind_points(
        &self,
        session_id: Option<String>,
    ) -> Result<Vec<RewindPointDto>, String> {
        let app_sid = match session_id {
            Some(id) if !id.trim().is_empty() => id,
            _ => {
                let guard = self.inner.lock();
                let s = guard.as_ref().ok_or("no active session")?;
                s.app_session_id.clone()
            }
        };
        // Ensure session exists in the index (or at least has a journal dir).
        let known = store::load_sessions_index().iter().any(|s| s.id == app_sid);
        if !known && store::load_messages(&app_sid).is_empty() {
            return Err(format!("session not found: {app_sid}"));
        }
        Ok(Self::rewind_points_from_journal(&app_sid))
    }

    fn rewind_points_from_journal(app_sid: &str) -> Vec<RewindPointDto> {
        let msgs = store::load_messages(app_sid);
        let mut out = Vec::new();
        let mut idx = 0u32;
        for m in msgs {
            if m.role != "user" {
                continue;
            }
            let raw = m.content.split_whitespace().collect::<Vec<_>>().join(" ");
            let preview = if raw.chars().count() > 80 {
                let truncated: String = raw.chars().take(79).collect();
                format!("{truncated}…")
            } else if raw.is_empty() {
                "…".into()
            } else {
                raw
            };
            out.push(RewindPointDto {
                prompt_index: idx,
                message_id: Some(m.id),
                preview,
            });
            idx = idx.saturating_add(1);
        }
        out
    }

    /// Rewind a session to a user-prompt index (keep that turn, drop after).
    /// Always truncates the local journal. Agent `x.ai/rewind/execute` is best-effort
    /// when this session is the live ACP session.
    pub async fn rewind_to_prompt_index(
        self: &Arc<Self>,
        app: AppHandle,
        target_prompt_index: u32,
        restore_files: bool,
        session_id: Option<String>,
    ) -> Result<RewindExecuteResult, String> {
        let app_sid = match session_id {
            Some(id) if !id.trim().is_empty() => id,
            _ => {
                let guard = self.inner.lock();
                let s = guard.as_ref().ok_or("no active session")?;
                s.app_session_id.clone()
            }
        };

        // Block if *this* session is mid-turn on the live host.
        let (live_match, backend, acp, busy) = {
            let guard = self.inner.lock();
            match guard.as_ref() {
                Some(s) if s.app_session_id == app_sid => {
                    let busy = s.fsm.state() == SessionState::Streaming
                        || s.fsm.state() == SessionState::AwaitingPermission;
                    (true, s.backend.clone(), s.acp.clone(), busy)
                }
                _ => (false, String::new(), None, false),
            }
        };
        if busy {
            return Err("cannot rewind while a turn is running".into());
        }

        let msgs = store::load_messages(&app_sid);
        let user_count = msgs.iter().filter(|m| m.role == "user").count() as u32;
        if user_count == 0 {
            return Err("no user messages to rewind".into());
        }
        if target_prompt_index >= user_count {
            return Err(format!(
                "user prompt index out of range: {target_prompt_index} (have {user_count})"
            ));
        }

        let mut agent_ok = true;
        let mut agent_error: Option<String> = None;

        // Agent path only when this is the live session with a real ACP client.
        if live_match && backend != "mock_acp" && !AcpClient::use_mock() {
            if let Some(client) = acp {
                match client
                    .rewind_execute(target_prompt_index, restore_files)
                    .await
                {
                    Ok(_) => {
                        tracing::info!(
                            target: "session",
                            "rewind_to_prompt_index: agent rewound target={target_prompt_index}"
                        );
                    }
                    Err(e) => {
                        agent_ok = false;
                        agent_error = Some(e.clone());
                        tracing::warn!(
                            target: "session",
                            error = %e,
                            "agent rewind failed; applying local journal truncate only"
                        );
                    }
                }
            } else {
                agent_ok = false;
                agent_error = Some("agent not connected".into());
            }
        } else if !live_match {
            agent_ok = false;
            agent_error = Some("session not live; local journal only".into());
        }

        let kept_count = store::update_messages(&app_sid, |messages| {
            let kept = store::truncate_through_user_prompt(messages, target_prompt_index)?;
            let kept_count = kept.len();
            *messages = kept;
            Ok(kept_count)
        })?;

        // Touch meta updated_at for index sort.
        if let Some(mut meta) = store::load_sessions_index()
            .into_iter()
            .find(|s| s.id == app_sid)
        {
            meta.updated_at = chrono::Utc::now();
            let _ = store::update_session_meta(&meta);
            if live_match {
                let mut guard = self.inner.lock();
                if let Some(s) = guard.as_mut() {
                    if s.app_session_id == app_sid {
                        s.meta.updated_at = meta.updated_at;
                    }
                }
            }
        }

        let snap = self.snapshot();
        Self::emit_state(&app, &snap);
        Ok(RewindExecuteResult {
            snapshot: snap,
            agent_ok,
            agent_error,
            local_ok: true,
            kept_count,
        })
    }

    fn prepare_user_send_on(
        session: &mut LiveSession,
        turn_id: &str,
        text: &str,
        journal_content: &str,
        attachments: Option<Vec<MessageAttachmentStored>>,
        memory: Option<&crate::memory_injection::MemoryInjectionPreparedV1>,
        skill_uses: &[crate::skill_feedback::SkillUseRecordV1],
    ) -> Result<
        (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<Arc<AcpClient>>,
            String,
            Option<String>,
        ),
        String,
    > {
        if let Some(pending_status) = session.pending_skill_settlement {
            settle_active_skill_uses(session, pending_status);
            if session.pending_skill_settlement.is_some() {
                return Err(
                    "SKILL_USE_SETTLEMENT_PENDING: retry durable Skill settlement first".into(),
                );
            }
        }
        if !session.active_skill_uses.is_empty() {
            return Err("SKILL_USE_DELIVERY_UNKNOWN: prior Skill use is not terminal".into());
        }
        if memory
            .as_ref()
            .is_some_and(|prepared| prepared.record.session_id != session.app_session_id)
        {
            return Err("MEMORY_INJECTION_SESSION_MISMATCH".into());
        }
        let mut skill_ids = HashSet::with_capacity(skill_uses.len());
        if skill_uses.iter().any(|record| {
            record.session_id != session.app_session_id
                || record.turn_id != turn_id
                || record.status != crate::skill_feedback::SkillUseStatusV1::Prepared
                || !skill_ids.insert(record.id.as_str())
        }) {
            return Err("SKILL_USE_STALE: prepared Skill evidence does not match this turn".into());
        }
        session.fsm.begin_stream().map_err(|e| e.to_string())?;
        session.allow_auto_wake = false;
        Self::touch_stream_progress_locked(session);
        let mid = Uuid::new_v4().to_string();
        session.streaming_message_id = Some(mid);
        session.stream_buf.clear();
        session.stream_thought.clear();
        session.stream_last_was_assistant = false;
        session.stream_phase_id_locked = false;
        session.stream_attachments.clear();
        session.journal_throttle.reset();
        session.last_stall_emit = None;
        session.open_tool_ids.clear();
        session.seen_tool_ids.clear();
        session.deferred_prompt_complete = None;
        session.provider_retry_attempt = 0;
        session.provider_retry_aborted = false;
        session.tools_this_turn = 0;

        let mut agent_prompt = text.to_string();
        let consumed_history_bootstrap = session.needs_history_bootstrap;
        if consumed_history_bootstrap {
            if let Some(ctx) = build_history_bootstrap(&session.app_session_id) {
                agent_prompt = prepend_host_context_preserving_directives(&agent_prompt, &ctx);
                tracing::info!(
                    "history bootstrap attached ({} chars) for session {}",
                    ctx.len(),
                    session.app_session_id
                );
            }
            session.needs_history_bootstrap = false;
        }
        if let Some(hint) = session_lookup_host_hint(text) {
            agent_prompt = prepend_host_context_preserving_directives(&agent_prompt, &hint);
        }
        if let Some(prepared) = memory {
            agent_prompt = prepend_host_context_preserving_directives(
                &agent_prompt,
                &prepared.prompt_fragment,
            );
        }
        if let Err(error) = store::append_message(
            &session.app_session_id,
            user_journal_message(
                turn_id.to_string(),
                journal_content.to_string(),
                text,
                attachments,
            ),
        ) {
            session.needs_history_bootstrap = consumed_history_bootstrap;
            reset_rejected_turn(session);
            return Err(format!("persist user turn before Runtime send: {error}"));
        }
        session.active_skill_uses = skill_uses.to_vec();
        Ok((
            session.backend.clone(),
            session.app_session_id.clone(),
            session.process_id.clone(),
            session.model_id.clone(),
            session.project_path.clone(),
            session.acp.clone(),
            agent_prompt,
            session.effort.clone(),
        ))
    }

    fn prepare_user_send(
        &self,
        expected_session_id: Option<&str>,
        turn_id: &str,
        text: &str,
        journal_content: &str,
        attachments: Option<Vec<MessageAttachmentStored>>,
        memory: Option<&crate::memory_injection::MemoryInjectionPreparedV1>,
        skill_uses: &[crate::skill_feedback::SkillUseRecordV1],
    ) -> Result<
        (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<Arc<AcpClient>>,
            String,
            Option<String>,
        ),
        String,
    > {
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                let live_matches = expected_session_id
                    .map(|expected| expected == session.app_session_id)
                    .unwrap_or(true);
                if live_matches {
                    return Self::prepare_user_send_on(
                        session,
                        turn_id,
                        text,
                        journal_content,
                        attachments,
                        memory,
                        skill_uses,
                    );
                }
            } else if expected_session_id.is_none() {
                return Err("no active session".into());
            }
        }
        let Some(expected) = expected_session_id else {
            return Err("no active session".into());
        };
        let mut background = self.background.lock();
        if let Some(session) = background.get_mut(expected) {
            return Self::prepare_user_send_on(
                session,
                turn_id,
                text,
                journal_content,
                attachments,
                memory,
                skill_uses,
            );
        }
        Err("SESSION_SEND_STALE: active session changed".into())
    }

    pub async fn send_message_for_session(
        self: &Arc<Self>,
        app: AppHandle,
        expected_session_id: String,
        text: String,
        display_text: Option<String>,
        attachments: Option<Vec<MessageAttachmentStored>>,
    ) -> Result<SessionSnapshot, String> {
        self.send_message_inner(
            app,
            Some(expected_session_id),
            None,
            text,
            display_text,
            attachments,
            None,
            Vec::new(),
            Vec::new(),
        )
        .await
        .map(|(snapshot, _, _)| snapshot)
    }

    pub async fn send_message_v2(
        self: &Arc<Self>,
        app: AppHandle,
        expected_session_id: String,
        turn_id: String,
        text: String,
        display_text: Option<String>,
        attachments: Option<Vec<MessageAttachmentStored>>,
        memory: Option<crate::memory_injection::MemoryInjectionPreparedV1>,
        skill_uses: Vec<crate::skill_feedback::SkillUseRecordV1>,
        explicit_connectors: Vec<String>,
    ) -> Result<SessionSendResultV2, String> {
        let disclosure = memory.as_ref().map(|prepared| prepared.disclosure.clone());
        match self
            .send_message_inner(
                app,
                Some(expected_session_id),
                Some(turn_id),
                text,
                display_text,
                attachments,
                memory.clone(),
                skill_uses,
                explicit_connectors,
            )
            .await
        {
            Ok((snapshot, memory_injection, skill_uses)) => Ok(SessionSendResultV2 {
                version: 2,
                snapshot,
                memory_injection,
                memory_disclosure: disclosure,
                skill_uses,
            }),
            Err(error) => {
                if let Some(prepared) = memory {
                    if let Err(audit_error) =
                        crate::memory_injection::mark_memory_injection_failed_v1(
                            memory_injection_failed_request(
                                &prepared,
                                crate::memory_injection::MemoryInjectionFailureCodeV1::ContextUnavailable,
                            ),
                        )
                    {
                        if !audit_error.contains("STALE_MEMORY_INJECTION")
                            && !audit_error.contains("not pending delivery")
                        {
                            tracing::warn!(
                                "mark rejected memory injection failed id={}: {audit_error}",
                                prepared.record.injection_id
                            );
                        }
                    }
                }
                Err(error)
            }
        }
    }

    async fn send_message_inner(
        self: &Arc<Self>,
        app: AppHandle,
        expected_session_id: Option<String>,
        turn_id: Option<String>,
        text: String,
        display_text: Option<String>,
        attachments: Option<Vec<MessageAttachmentStored>>,
        memory: Option<crate::memory_injection::MemoryInjectionPreparedV1>,
        skill_uses: Vec<crate::skill_feedback::SkillUseRecordV1>,
        explicit_connectors: Vec<String>,
    ) -> Result<
        (
            SessionSnapshot,
            Option<crate::memory_injection::MemoryInjectionRecordV1>,
            Vec<crate::skill_feedback::SkillUseRecordV1>,
        ),
        String,
    > {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("empty message".into());
        }
        let attachments = attachments
            .map(normalize_explicit_attachments)
            .transpose()?;
        // Journal stores UI form when provided (skill chips); agent still receives `text`.
        let journal_content = display_text
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| text.clone());
        let turn_id = turn_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        // If agent is a fresh session/new, wrap recent journal into the prompt once.
        let (backend, app_sid, process_id, model_id, project_path, acp, agent_prompt, effort) =
            self.prepare_user_send(
                expected_session_id.as_deref(),
                &turn_id,
                &text,
                &journal_content,
                attachments,
                memory.as_ref(),
                &skill_uses,
            )?;
        Self::emit_state(&app, &self.snapshot());

        let skill_uses = if skill_uses.is_empty() {
            Vec::new()
        } else {
            let dispatching = skill_uses.clone();
            let transition = tauri::async_runtime::spawn_blocking(move || {
                transition_skill_use_records(
                    &dispatching,
                    crate::skill_feedback::SkillUseStatusV1::Dispatching,
                )
            })
            .await;
            let transitioned = match transition {
                Ok(Ok(records)) => records,
                Ok(Err(error)) => {
                    let rollback = rollback_unwritten_user_turn(&app_sid, &turn_id);
                    self.reset_rejected_session(&app_sid);
                    if !skill_uses.is_empty() {
                        let _ = self.defer_skill_settlement_for_session(
                            &app_sid,
                            skill_uses.clone(),
                            crate::skill_feedback::SkillUseStatusV1::Interrupted,
                        );
                    }
                    Self::emit_state(&app, &self.snapshot());
                    return Err(match rollback {
                        Ok(()) => format!("persist Skill dispatch barrier: {error}"),
                        Err(rollback_error) => format!(
                            "persist Skill dispatch barrier: {error}; rollback user turn: {rollback_error}"
                        ),
                    });
                }
                Err(error) => {
                    let rollback = rollback_unwritten_user_turn(&app_sid, &turn_id);
                    self.reset_rejected_session(&app_sid);
                    if !skill_uses.is_empty() {
                        let _ = self.defer_skill_settlement_for_session(
                            &app_sid,
                            skill_uses.clone(),
                            crate::skill_feedback::SkillUseStatusV1::Interrupted,
                        );
                    }
                    Self::emit_state(&app, &self.snapshot());
                    return Err(match rollback {
                        Ok(()) => format!("Skill dispatch barrier task failed: {error}"),
                        Err(rollback_error) => format!(
                            "Skill dispatch barrier task failed: {error}; rollback user turn: {rollback_error}"
                        ),
                    });
                }
            };
            if !self.update_active_skill_uses_for_session(&app_sid, transitioned.clone()) {
                let rollback = transitioned.clone();
                let rollback_result = tauri::async_runtime::spawn_blocking(move || {
                    transition_skill_use_records(
                        &rollback,
                        crate::skill_feedback::SkillUseStatusV1::Interrupted,
                    )
                })
                .await;
                let _ = rollback_unwritten_user_turn(&app_sid, &turn_id);
                self.reset_rejected_session(&app_sid);
                if !matches!(rollback_result, Ok(Ok(_))) {
                    let restored = self.defer_skill_settlement_for_session(
                        &app_sid,
                        transitioned,
                        crate::skill_feedback::SkillUseStatusV1::Interrupted,
                    );
                    if !restored {
                        tracing::error!(
                            session_id = %app_sid,
                            "Skill dispatch reconciliation remains durable but detached from a live session"
                        );
                    }
                }
                Self::emit_state(&app, &self.snapshot());
                return Err("SKILL_USE_STALE: active turn changed before Runtime dispatch".into());
            }
            transitioned
        };

        let memory = match memory {
            Some(mut prepared) => {
                let mutation = memory_injection_mutation_request(&prepared);
                let dispatched = tauri::async_runtime::spawn_blocking(move || {
                    crate::memory_injection::mark_memory_injection_dispatching_v1(mutation)
                })
                .await;
                let dispatched = match dispatched {
                    Ok(Ok(record)) => record,
                    Ok(Err(error)) => {
                        let rollback = skill_uses.clone();
                        let rollback_result = tauri::async_runtime::spawn_blocking(move || {
                            transition_skill_use_records(
                                &rollback,
                                crate::skill_feedback::SkillUseStatusV1::Interrupted,
                            )
                        })
                        .await;
                        let _ = rollback_unwritten_user_turn(&app_sid, &turn_id);
                        self.reset_rejected_session(&app_sid);
                        if !matches!(rollback_result, Ok(Ok(_))) && !skill_uses.is_empty() {
                            let _ = self.defer_skill_settlement_for_session(
                                &app_sid,
                                skill_uses.clone(),
                                crate::skill_feedback::SkillUseStatusV1::Interrupted,
                            );
                        }
                        Self::emit_state(&app, &self.snapshot());
                        return Err(format!("persist Memory dispatch barrier: {error}"));
                    }
                    Err(error) => {
                        let rollback = skill_uses.clone();
                        let rollback_result = tauri::async_runtime::spawn_blocking(move || {
                            transition_skill_use_records(
                                &rollback,
                                crate::skill_feedback::SkillUseStatusV1::Interrupted,
                            )
                        })
                        .await;
                        let _ = rollback_unwritten_user_turn(&app_sid, &turn_id);
                        self.reset_rejected_session(&app_sid);
                        if !matches!(rollback_result, Ok(Ok(_))) && !skill_uses.is_empty() {
                            let _ = self.defer_skill_settlement_for_session(
                                &app_sid,
                                skill_uses.clone(),
                                crate::skill_feedback::SkillUseStatusV1::Interrupted,
                            );
                        }
                        Self::emit_state(&app, &self.snapshot());
                        return Err(format!("Memory dispatch barrier task failed: {error}"));
                    }
                };
                prepared.record = dispatched;
                Some(prepared)
            }
            None => None,
        };

        if backend == "mock_acp" || AcpClient::use_mock() {
            let applied_skill_uses = if skill_uses.is_empty() {
                Vec::new()
            } else {
                match transition_skill_use_records(
                    &skill_uses,
                    crate::skill_feedback::SkillUseStatusV1::Applied,
                ) {
                    Ok(records) => {
                        let _ =
                            self.update_active_skill_uses_for_session(&app_sid, records.clone());
                        records
                    }
                    Err(error) => {
                        if let Some(prepared) = memory.as_ref() {
                            let _ = crate::memory_injection::mark_memory_injection_failed_v1(
                                memory_injection_failed_request(
                                    prepared,
                                    crate::memory_injection::MemoryInjectionFailureCodeV1::ContextUnavailable,
                                ),
                            );
                        }
                        self.reset_rejected_session(&app_sid);
                        if !skill_uses.is_empty() {
                            let _ = self.defer_skill_settlement_for_session(
                                &app_sid,
                                skill_uses.clone(),
                                crate::skill_feedback::SkillUseStatusV1::Interrupted,
                            );
                        }
                        return Err(format!("record mock Skill delivery: {error}"));
                    }
                }
            };
            let memory_record = if let Some(prepared) = memory.as_ref() {
                match crate::memory_injection::mark_memory_injection_applied_v1(
                    memory_injection_mutation_request(prepared),
                ) {
                    Ok(record) => {
                        if let Err(error) = store::append_message(
                            &app_sid,
                            memory_injection_marker_message(prepared),
                        ) {
                            tracing::warn!(
                                "persist applied memory disclosure marker failed session={app_sid}: {error}"
                            );
                        }
                        Some(record)
                    }
                    Err(error) => {
                        let settlement = transition_skill_use_records(
                            &applied_skill_uses,
                            crate::skill_feedback::SkillUseStatusV1::Interrupted,
                        );
                        self.reset_rejected_session(&app_sid);
                        if settlement.is_err() && !applied_skill_uses.is_empty() {
                            let _ = self.defer_skill_settlement_for_session(
                                &app_sid,
                                applied_skill_uses.clone(),
                                crate::skill_feedback::SkillUseStatusV1::Interrupted,
                            );
                        }
                        Self::emit_state(&app, &self.snapshot());
                        return Err(format!("record mock memory injection delivery: {error}"));
                    }
                }
            } else {
                None
            };
            let message_id = self
                .inner
                .lock()
                .as_ref()
                .filter(|session| session.app_session_id == app_sid)
                .and_then(|s| s.streaming_message_id.clone())
                .or_else(|| {
                    self.background
                        .lock()
                        .get(&app_sid)
                        .and_then(|session| session.streaming_message_id.clone())
                })
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let mgr = Arc::clone(self);
            let app_done = app.clone();
            let handle = mock_acp::spawn_fake_stream(
                app_sid.clone(),
                message_id,
                agent_prompt,
                Duration::from_millis(25),
                move |chunk: StreamChunk| {
                    let _ = app_done.emit(
                        "session://stream",
                        serde_json::json!({
                            "sessionId": chunk.session_id,
                            "messageId": chunk.message_id,
                            "text": chunk.text,
                            "done": chunk.done,
                            "kind": "assistant"
                        }),
                    );
                    let apply_chunk = |s: &mut LiveSession| {
                        SessionManager::touch_stream_progress_locked(s);
                        s.stream_buf.push_str(&chunk.text);
                        // I04: throttle mid-stream; force on terminal done.
                        let para = is_paragraph_break(&chunk.text);
                        SessionManager::maybe_flush_stream_journal(s, chunk.done, para);
                        if chunk.done {
                            settle_active_skill_uses(
                                s,
                                crate::skill_feedback::SkillUseStatusV1::Succeeded,
                            );
                            s.stream_buf.clear();
                            s.stream_thought.clear();
                            s.stream_last_was_assistant = false;
                            s.stream_phase_id_locked = false;
                            s.stream_attachments.clear();
                            s.journal_throttle.reset();
                            s.open_tool_ids.clear();
                            s.last_stall_emit = None;
                            if s.fsm.state() == SessionState::Streaming {
                                let _ = s.fsm.end_stream();
                                s.streaming_message_id = None;
                            }
                        }
                    };
                    let handled_live = {
                        let mut guard = mgr.inner.lock();
                        if let Some(s) = guard
                            .as_mut()
                            .filter(|session| session.app_session_id == chunk.session_id)
                        {
                            apply_chunk(s);
                            true
                        } else {
                            false
                        }
                    };
                    if !handled_live {
                        if let Some(session) = mgr.background.lock().get_mut(&chunk.session_id) {
                            apply_chunk(session);
                        }
                    }
                    if chunk.done {
                        if let Err(error) = crate::automation_scheduler::complete_for_session(
                            &chunk.session_id,
                            true,
                            None,
                        ) {
                            tracing::warn!("complete mock automation claim: {error}");
                        }
                        SessionManager::emit_state(&app_done, &mgr.snapshot());
                    }
                },
            );
            let mut handle = Some(handle);
            {
                let mut live = self.inner.lock();
                if let Some(session) = live
                    .as_mut()
                    .filter(|session| session.app_session_id == app_sid)
                {
                    session.mock_stream = handle.take();
                }
            }
            if let Some(handle) = handle {
                if let Some(session) = self.background.lock().get_mut(&app_sid) {
                    session.mock_stream = Some(handle);
                }
            }
            return Ok((self.snapshot(), memory_record, applied_skill_uses));
        }

        if agent_loop::is_sunsetz_backend(&backend) {
            return self
                .send_sunsetz_turn(
                    app,
                    app_sid,
                    process_id,
                    model_id,
                    effort,
                    project_path,
                    agent_prompt,
                    memory,
                    skill_uses,
                    explicit_connectors,
                )
                .await;
        }

        let acp = acp.ok_or("ACP client missing")?;
        let mgr = Arc::clone(self);
        let app2 = app.clone();
        let automation_session_id = app_sid.clone();
        let (write_ack_tx, write_ack_rx) = if memory.is_some() || !skill_uses.is_empty() {
            let (tx, rx) = tokio::sync::oneshot::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        tokio::spawn(async move {
            let prompt_result = match write_ack_tx {
                Some(write_ack) => {
                    acp.prompt_with_transport_ack(&agent_prompt, write_ack)
                        .await
                }
                None => acp.prompt(&agent_prompt).await,
            };
            match prompt_result {
                Ok(_) => {
                    if let Err(error) = crate::automation_scheduler::complete_for_session(
                        &automation_session_id,
                        true,
                        None,
                    ) {
                        tracing::warn!("complete automation claim: {error}");
                    }
                }
                Err(e) => {
                    let automation_error = e.message.clone();
                    if !mgr.fail_prompt_for_session(&app2, &automation_session_id, &e) {
                        tracing::warn!(
                            "prompt failure arrived after session was removed id={automation_session_id}"
                        );
                    }
                    if let Err(error) = crate::automation_scheduler::complete_for_session(
                        &automation_session_id,
                        false,
                        Some(&automation_error),
                    ) {
                        tracing::warn!("fail automation claim: {error}");
                    }
                    SessionManager::emit_state(&app2, &mgr.snapshot());
                }
            }
        });

        let mut memory_record = None;
        let mut delivered_skill_uses = skill_uses.clone();
        if let Some(write_ack) = write_ack_rx {
            match write_ack.await {
                Ok(TransportWriteAck::Written) => {
                    if !skill_uses.is_empty() {
                        match transition_skill_use_records(
                            &skill_uses,
                            crate::skill_feedback::SkillUseStatusV1::Applied,
                        ) {
                            Ok(records) => {
                                let _ = self.update_active_skill_uses_for_session(
                                    &app_sid,
                                    records.clone(),
                                );
                                delivered_skill_uses = records;
                            }
                            Err(error) => tracing::error!(
                                session_id = %app_sid,
                                "Skill transport succeeded but audit update failed: {error}"
                            ),
                        }
                    }
                    if let Some(prepared) = memory.as_ref() {
                        let record = match crate::memory_injection::mark_memory_injection_applied_v1(
                            memory_injection_mutation_request(prepared),
                        ) {
                            Ok(record) => record,
                            Err(error) => {
                                // Runtime already received the Host-owned prompt.
                                // Keep the dispatch barrier and never suggest a
                                // duplicate merely because the audit write failed.
                                tracing::error!(
                                    "memory injection transport succeeded but audit update failed id={}: {error}",
                                    prepared.record.injection_id
                                );
                                prepared.record.clone()
                            }
                        };
                        if let Err(error) = store::append_message(
                            &prepared.record.session_id,
                            memory_injection_marker_message(prepared),
                        ) {
                            tracing::warn!(
                                "persist applied memory disclosure marker failed session={}: {error}",
                                prepared.record.session_id
                            );
                        }
                        memory_record = Some(record);
                    }
                }
                Ok(TransportWriteAck::Rejected(error)) => {
                    let skill_settlement = if skill_uses.is_empty() {
                        Ok(Vec::new())
                    } else {
                        transition_skill_use_records(
                            &skill_uses,
                            crate::skill_feedback::SkillUseStatusV1::Failed,
                        )
                    };
                    if let Some(prepared) = memory.as_ref() {
                        if let Err(audit_error) =
                            crate::memory_injection::mark_memory_injection_failed_v1(
                                memory_injection_failed_request(
                                    prepared,
                                    crate::memory_injection::MemoryInjectionFailureCodeV1::RuntimeWriteFailed,
                                ),
                            )
                        {
                            tracing::warn!(
                                "mark memory transport failure failed id={}: {audit_error}",
                                prepared.record.injection_id
                            );
                        }
                    }
                    if let Err(marker_error) =
                        record_runtime_write_rejected_marker(&app, &app_sid, &turn_id)
                    {
                        tracing::warn!(
                            session_id = %app_sid,
                            "persist Runtime write rejection marker: {marker_error}"
                        );
                    }
                    self.reset_rejected_session(&app_sid);
                    if let Err(settlement_error) = skill_settlement {
                        let _ = self.defer_skill_settlement_for_session(
                            &app_sid,
                            skill_uses.clone(),
                            crate::skill_feedback::SkillUseStatusV1::Failed,
                        );
                        tracing::warn!(
                            session_id = %app_sid,
                            "defer rejected Skill settlement: {settlement_error}"
                        );
                    }
                    Self::emit_state(&app, &self.snapshot());
                    return Err(format!(
                        "Runtime write failed before delivery: {}",
                        sanitize_error_detail(&error)
                    ));
                }
                Ok(TransportWriteAck::DeliveryUnknown(error)) => {
                    tracing::error!(
                        session_id = %app_sid,
                        "turn delivery unknown: {}",
                        sanitize_error_detail(&error)
                    );
                    return Err("RUNTIME_DELIVERY_UNKNOWN: Runtime write outcome is unknown; automatic retry is disabled".into());
                }
                Err(_) => {
                    return Err("RUNTIME_DELIVERY_UNKNOWN: Runtime write acknowledgement closed; automatic retry is disabled".into());
                }
            }
        }

        Ok((self.snapshot(), memory_record, delivered_skill_uses))
    }

    async fn send_sunsetz_turn(
        self: &Arc<Self>,
        app: AppHandle,
        app_sid: String,
        process_id: String,
        model_id: Option<String>,
        effort: Option<String>,
        project_path: Option<String>,
        agent_prompt: String,
        memory: Option<crate::memory_injection::MemoryInjectionPreparedV1>,
        skill_uses: Vec<crate::skill_feedback::SkillUseRecordV1>,
        explicit_connectors: Vec<String>,
    ) -> Result<
        (
            SessionSnapshot,
            Option<crate::memory_injection::MemoryInjectionRecordV1>,
            Vec<crate::skill_feedback::SkillUseRecordV1>,
        ),
        String,
    > {
        let mut agent_prompt = agent_prompt;
        let mut skill_chars = 0_usize;
        if !skill_uses.is_empty() {
            let selections = skill_uses
                .iter()
                .map(|record| (record.skill.id.clone(), record.skill.tree_hash.clone()))
                .collect::<Vec<_>>();
            let project = project_path.clone();
            let fragments = match tauri::async_runtime::spawn_blocking(move || {
                crate::skill_inventory::load_host_skill_fragments_v1(
                    &selections,
                    project.as_deref(),
                )
            })
            .await
            {
                Ok(Ok(fragments)) => fragments,
                Ok(Err(error)) => {
                    self.reset_rejected_session(&app_sid);
                    return Err(format!("load Host-trusted Skill: {error}"));
                }
                Err(error) => {
                    self.reset_rejected_session(&app_sid);
                    return Err(format!("load Host-trusted Skill task failed: {error}"));
                }
            };
            if !fragments.is_empty() {
                skill_chars = fragments.chars().count();
                agent_prompt =
                    prepend_host_context_preserving_directives(&agent_prompt, &fragments);
            }
        }
        if let Some(automation) =
            crate::automation_scheduler::bound_automation_for_session(&app_sid)
        {
            if !automation.skill_ids.is_empty() {
                let selections = automation
                    .skill_ids
                    .iter()
                    .map(|skill| (skill.id.clone(), skill.tree_hash.clone()))
                    .collect::<Vec<_>>();
                let project = project_path.clone();
                if let Ok(Ok(fragments)) = tauri::async_runtime::spawn_blocking(move || {
                    crate::skill_inventory::load_host_skill_fragments_v1(
                        &selections,
                        project.as_deref(),
                    )
                })
                .await
                {
                    if !fragments.is_empty() {
                        skill_chars = skill_chars.saturating_add(fragments.chars().count());
                        agent_prompt =
                            prepend_host_context_preserving_directives(&agent_prompt, &fragments);
                    }
                }
            }
        }
        let applied_skill_uses = if skill_uses.is_empty() {
            Vec::new()
        } else {
            match transition_skill_use_records(
                &skill_uses,
                crate::skill_feedback::SkillUseStatusV1::Applied,
            ) {
                Ok(records) => {
                    let _ = self.update_active_skill_uses_for_session(&app_sid, records.clone());
                    records
                }
                Err(error) => {
                    self.reset_rejected_session(&app_sid);
                    return Err(format!("record Sunsetz Skill delivery: {error}"));
                }
            }
        };
        let memory_record = if let Some(prepared) = memory.as_ref() {
            match crate::memory_injection::mark_memory_injection_applied_v1(
                memory_injection_mutation_request(prepared),
            ) {
                Ok(record) => Some(record),
                Err(error) => {
                    tracing::warn!(
                        "Sunsetz memory audit update failed id={}: {error}",
                        prepared.record.injection_id
                    );
                    Some(prepared.record.clone())
                }
            }
        } else {
            None
        };

        let (root, trusted) = agent_loop::resolve_trusted_root(project_path.as_deref());
        let endpoint =
            match agent_loop::resolve_inference_credentials(model_id.as_deref().unwrap_or("")) {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    if !self.fail_prompt_for_session(&app, &app_sid, &error) {
                        tracing::warn!(
                            "Sunsetz credential failure after session was removed id={app_sid}"
                        );
                    }
                    Self::emit_state(&app, &self.snapshot());
                    return Ok((self.snapshot(), memory_record, applied_skill_uses));
                }
            };
        let client = match agent_loop::http_client() {
            Ok(client) => client,
            Err(error) => {
                let _ = self.fail_prompt_for_session(&app, &app_sid, &error);
                Self::emit_state(&app, &self.snapshot());
                return Ok((self.snapshot(), memory_record, applied_skill_uses));
            }
        };
        let wakes = self.subagents.lock().await.take_pending_wakes(&app_sid);
        if !wakes.is_empty() {
            agent_prompt = prepend_host_context_preserving_directives(
                &agent_prompt,
                &subagents::wake_prompt(&wakes),
            );
        }
        let stop = Arc::new(AtomicBool::new(false));
        let host_turn_id = Uuid::new_v4().to_string();
        let mut last_usage = None;
        {
            let mut live = self.inner.lock();
            if let Some(session) = live
                .as_mut()
                .filter(|session| session.app_session_id == app_sid)
            {
                session.agent_cancel = Some(Arc::clone(&stop));
                session.host_turn_id = Some(host_turn_id.clone());
                last_usage = session.meta.context_usage.clone();
            } else if let Some(session) = self.background.lock().get_mut(&app_sid) {
                session.agent_cancel = Some(Arc::clone(&stop));
                session.host_turn_id = Some(host_turn_id.clone());
                last_usage = session.meta.context_usage.clone();
            }
        }

        let permission_gate: agent_loop::HostToolPermissionGate = {
            let mgr = Arc::clone(self);
            let app_gate = app.clone();
            let sid = app_sid.clone();
            Arc::new(move |req| {
                let mgr = Arc::clone(&mgr);
                let app_gate = app_gate.clone();
                let sid = sid.clone();
                Box::pin(async move { mgr.request_host_tool_permission(app_gate, sid, req).await })
            })
        };
        let permission_gate_for_child = permission_gate.clone();
        let ask_user_gate: agent_loop::HostAskUserGate = {
            let mgr = Arc::clone(self);
            let app_gate = app.clone();
            let sid = app_sid.clone();
            Arc::new(move |req| {
                let mgr = Arc::clone(&mgr);
                let app_gate = app_gate.clone();
                let sid = sid.clone();
                Box::pin(async move { mgr.request_host_ask_user(app_gate, sid, req).await })
            })
        };
        let mut cfg = agent_loop::AgentTurnConfig {
            endpoint,
            project_root: root,
            trusted,
            history: Vec::new(),
            user_prompt: agent_prompt,
            stop: Arc::clone(&stop),
            client,
            max_tool_rounds: agent_loop::MAX_TOOL_ROUNDS,
            permission_gate: Some(permission_gate),
            ask_user_gate: Some(ask_user_gate),
            reasoning_effort: effort,
            connectors: agent_loop::ConnectorTurn {
                tools: crate::connectors::connected_tool_definitions(),
                write_tools: crate::connectors::connected_write_tools()
                    .into_iter()
                    .collect(),
                explicit: explicit_connectors,
                invoke: Some(Arc::new(|name, arguments| {
                    Box::pin(async move { crate::connectors::invoke_tool(&name, &arguments).await })
                })),
            },
            kind: agent_loop::AgentKind::Parent,
            spawn_depth: 0,
            subagents: agent_loop::SubagentHooks::default(),
            sandbox_profile: crate::runtime_compat::SandboxProfileV1::parse(
                &crate::store::load_settings().sandbox_profile,
            ),
            skill_prompt_chars: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(
                skill_chars,
            )),
            allow_schedule_task: !crate::automation_scheduler::session_has_active_claim(&app_sid),
            allow_skill_save: !crate::automation_scheduler::session_has_active_claim(&app_sid),
        };
        let child_template = agent_loop::AgentTurnConfig {
            subagents: agent_loop::SubagentHooks::default(),
            permission_gate: Some(permission_gate_for_child),
            skill_prompt_chars: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            ..cfg.clone()
        };
        let registry = Arc::clone(&self.subagents);
        let registry_out = Arc::clone(&self.subagents);
        let registry_kill = Arc::clone(&self.subagents);
        let session_for_spawn = app_sid.clone();
        let turn_for_spawn = host_turn_id.clone();
        let on_life: Arc<
            dyn Fn(String, String, String, String, Option<String>, Option<String>) + Send + Sync,
        > = Arc::new(|session, tool_id, status, title, detail, path| {
            persist_tool_step(
                &session,
                &tool_id,
                &status,
                "agent",
                &title,
                detail.as_deref(),
                path.as_deref(),
            );
        });
        let on_finished: Option<subagents::SubagentFinishedFn> = {
            let mgr = Arc::clone(self);
            let app_fin = app.clone();
            Some(Arc::new(
                move |session, tool_id, status, title, kind, agent_id| {
                    let mgr = Arc::clone(&mgr);
                    let app_fin = app_fin.clone();
                    Box::pin(async move {
                        mgr.on_subagent_finished(
                            app_fin, session, tool_id, status, title, kind, agent_id,
                        )
                        .await;
                    })
                },
            ))
        };
        cfg.subagents = agent_loop::SubagentHooks {
            spawn: Some(Arc::new(move |request| {
                let registry = Arc::clone(&registry);
                let template = child_template.clone();
                let session = session_for_spawn.clone();
                let turn = turn_for_spawn.clone();
                let on_life = Arc::clone(&on_life);
                let on_finished = on_finished.clone();
                Box::pin(async move {
                    subagents::spawn_with_registry(
                        registry,
                        template,
                        session,
                        turn,
                        request,
                        on_life,
                        on_finished,
                    )
                    .await
                })
            })),
            output: Some(Arc::new(move |id| {
                let registry = Arc::clone(&registry_out);
                Box::pin(async move { registry.lock().await.output(&id) })
            })),
            kill: Some(Arc::new(move |id| {
                let registry = Arc::clone(&registry_kill);
                Box::pin(async move { registry.lock().await.kill(&id) })
            })),
        };
        let mgr = Arc::clone(self);
        let app_ev = app.clone();
        let pid = process_id.clone();
        let automation_session_id = app_sid.clone();
        tokio::spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let pump = {
                let mgr = Arc::clone(&mgr);
                let app_ev = app_ev.clone();
                let pid = pid.clone();
                tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        mgr.handle_acp_event(&app_ev, &pid, event).await;
                    }
                })
            };
            let turn_failed = Arc::new(AtomicBool::new(false));
            let failed = Arc::clone(&turn_failed);
            match apply_sunsetz_compact(&mut cfg, &automation_session_id, last_usage, &tx).await {
                CompactGate::Continue => {
                    agent_loop::run_turn(cfg, move |event| {
                        if matches!(event, AcpEvent::Error { .. }) {
                            failed.store(true, Ordering::SeqCst);
                        }
                        let _ = tx.send(event);
                    })
                    .await;
                }
                CompactGate::Failed => {
                    turn_failed.store(true, Ordering::SeqCst);
                    drop(tx);
                }
                CompactGate::Finished => drop(tx),
            }
            let _ = pump.await;
            let ok = !stop.load(Ordering::SeqCst) && !turn_failed.load(Ordering::SeqCst);
            if let Err(error) = crate::automation_scheduler::complete_for_session(
                &automation_session_id,
                ok,
                (!ok).then_some("sunsetz turn failed"),
            ) {
                tracing::warn!("complete Sunsetz automation claim: {error}");
            }
            let still_ours = {
                let mut live = mgr.inner.lock();
                if let Some(session) = live
                    .as_mut()
                    .filter(|session| session.app_session_id == automation_session_id)
                {
                    if session
                        .agent_cancel
                        .as_ref()
                        .is_some_and(|flag| Arc::ptr_eq(flag, &stop))
                    {
                        session.agent_cancel = None;
                        true
                    } else {
                        false
                    }
                } else {
                    drop(live);
                    if let Some(session) = mgr.background.lock().get_mut(&automation_session_id) {
                        if session
                            .agent_cancel
                            .as_ref()
                            .is_some_and(|flag| Arc::ptr_eq(flag, &stop))
                        {
                            session.agent_cancel = None;
                            true
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                }
            };
            if still_ours {
                mgr.wake_inflight.lock().remove(&automation_session_id);
                let woke = mgr
                    .maybe_start_wake_turn(&app_ev, &automation_session_id)
                    .await;
                if !woke {
                    SessionManager::emit_state(&app_ev, &mgr.snapshot());
                }
            }
        });
        Ok((self.snapshot(), memory_record, applied_skill_uses))
    }

    async fn on_subagent_finished(
        self: &Arc<Self>,
        app: AppHandle,
        session_id: String,
        tool_call_id: String,
        status: String,
        title: String,
        kind: String,
        agent_id: String,
    ) {
        let tool_status = if status == "cancelled" {
            "cancelled"
        } else if status == "failed" {
            "failed"
        } else {
            "completed"
        };
        let card_title = if title.to_ascii_lowercase().starts_with("subagent ") {
            title.clone()
        } else {
            format!("Subagent {title}")
        };
        persist_tool_step(
            &session_id,
            &tool_call_id,
            tool_status,
            "agent",
            &card_title,
            Some(kind.as_str()),
            Some(agent_id.as_str()),
        );
        let parent_turn_id = self.subagents.lock().await.parent_turn_id(&agent_id);
        let live_turn = self
            .inspect_session_for_wake(&session_id)
            .and_then(|inspect| inspect.host_turn_id);
        let same_turn = match (parent_turn_id.as_deref(), live_turn.as_deref()) {
            (Some(parent), Some(live)) => parent == live,
            _ => false,
        };
        if same_turn {
            if let Some(process_id) = self.process_id_for_session(&session_id) {
                self.handle_acp_event(
                    &app,
                    &process_id,
                    AcpEvent::ToolCall {
                        tool_call_id,
                        title: card_title,
                        kind: "agent".into(),
                        status: tool_status.into(),
                        raw: serde_json::json!({
                            "rawInput": {
                                "id": agent_id,
                                "description": title,
                                "agentType": kind,
                                "background": true,
                            }
                        }),
                    },
                )
                .await;
            }
        }
        let _ = self.maybe_start_wake_turn(&app, &session_id).await;
    }

    fn process_id_for_session(&self, session_id: &str) -> Option<String> {
        if let Some(session) = self
            .inner
            .lock()
            .as_ref()
            .filter(|session| session.app_session_id == session_id)
        {
            return Some(session.process_id.clone());
        }
        self.background
            .lock()
            .get(session_id)
            .map(|session| session.process_id.clone())
    }

    fn inspect_session_for_wake(&self, session_id: &str) -> Option<WakeInspect> {
        let from_live = |session: &LiveSession| WakeInspect {
            backend: session.backend.clone(),
            streaming: session.fsm.state() == SessionState::Streaming,
            deferred_prompt_complete: session.deferred_prompt_complete.is_some(),
            awaiting_permission: session.fsm.state() == SessionState::AwaitingPermission,
            allow_auto_wake: session.allow_auto_wake,
            host_turn_id: session.host_turn_id.clone(),
            process_id: session.process_id.clone(),
            model_id: session.model_id.clone(),
            effort: session.effort.clone(),
            project_path: session.project_path.clone(),
        };
        if let Some(session) = self
            .inner
            .lock()
            .as_ref()
            .filter(|session| session.app_session_id == session_id)
        {
            return Some(from_live(session));
        }
        self.background.lock().get(session_id).map(from_live)
    }

    fn prepare_session_for_wake(&self, session_id: &str) -> bool {
        let apply = |session: &mut LiveSession| -> bool {
            if !agent_loop::is_sunsetz_backend(&session.backend) {
                return false;
            }
            match session.fsm.state() {
                SessionState::Ready => {
                    if session.fsm.begin_stream().is_err() {
                        return false;
                    }
                }
                SessionState::Streaming => {
                    if session.deferred_prompt_complete.is_none() {
                        return false;
                    }
                    let _ = Self::try_finish_deferred_prompt_complete(session);
                    if session.fsm.begin_stream().is_err() {
                        return false;
                    }
                }
                _ => return false,
            }
            session.allow_auto_wake = false;
            session.streaming_message_id = Some(Uuid::new_v4().to_string());
            session.stream_buf.clear();
            session.stream_thought.clear();
            session.stream_last_was_assistant = false;
            session.stream_phase_id_locked = false;
            session.stream_attachments.clear();
            session.journal_throttle.reset();
            session.open_tool_ids.clear();
            session.seen_tool_ids.clear();
            session.tools_this_turn = 0;
            session.deferred_prompt_complete = None;
            session.last_stall_emit = None;
            true
        };
        {
            let mut live = self.inner.lock();
            if let Some(session) = live
                .as_mut()
                .filter(|session| session.app_session_id == session_id)
            {
                return apply(session);
            }
        }
        let mut background = self.background.lock();
        background
            .get_mut(session_id)
            .map(|session| apply(session))
            .unwrap_or(false)
    }

    async fn maybe_start_wake_turn(self: &Arc<Self>, app: &AppHandle, session_id: &str) -> bool {
        if session_id.is_empty() {
            return false;
        }
        {
            let mut inflight = self.wake_inflight.lock();
            if !inflight.insert(session_id.to_string()) {
                return false;
            }
        }
        let started = self.start_wake_turn_inner(app, session_id).await;
        if !started {
            self.wake_inflight.lock().remove(session_id);
        }
        started
    }

    async fn start_wake_turn_inner(self: &Arc<Self>, app: &AppHandle, session_id: &str) -> bool {
        let Some(inspect) = self.inspect_session_for_wake(session_id) else {
            return false;
        };
        if !agent_loop::is_sunsetz_backend(&inspect.backend) {
            return false;
        }
        let (pending, this_turn_running) = {
            let registry = self.subagents.lock().await;
            let pending = registry.pending_wake_count(session_id);
            let this_turn_running = inspect
                .host_turn_id
                .as_deref()
                .map(|turn| registry.running_for_turn(session_id, turn).len())
                .unwrap_or(0);
            (pending, this_turn_running)
        };
        match subagents::decide_auto_wake(
            pending,
            this_turn_running,
            inspect.streaming,
            inspect.deferred_prompt_complete,
            inspect.awaiting_permission,
            inspect.allow_auto_wake,
        ) {
            subagents::AutoWakeDecision::Start => {}
            _ => return false,
        }
        if !self.prepare_session_for_wake(session_id) {
            return false;
        }
        Self::emit_state(app, &self.snapshot());
        let mgr = Arc::clone(self);
        let app = app.clone();
        let session_id = session_id.to_string();
        let process_id = inspect.process_id;
        let model_id = inspect.model_id;
        let effort = inspect.effort;
        let project_path = inspect.project_path;
        let handle = tokio::runtime::Handle::current();
        tauri::async_runtime::spawn_blocking(move || {
            let result = handle.block_on(mgr.send_sunsetz_turn(
                app,
                session_id.clone(),
                process_id,
                model_id,
                effort,
                project_path,
                "Continue the parent task using the background subagent results.".into(),
                None,
                Vec::new(),
                Vec::new(),
            ));
            if let Err(error) = result {
                tracing::warn!("sunsetz wake turn failed session={session_id}: {error}");
                mgr.wake_inflight.lock().remove(&session_id);
            }
        });
        true
    }

    pub async fn subagent_get(&self, id: &str) -> Result<SubagentView, String> {
        self.subagents
            .lock()
            .await
            .get(id)
            .ok_or_else(|| format!("unknown agent `{id}`"))
    }

    pub async fn stop(self: &Arc<Self>, app: AppHandle) -> Result<SessionSnapshot, String> {
        let (acp, ask_activity, interrupted, cancel_sid, cancel_turn) = {
            let mut guard = self.inner.lock();
            let s = guard.as_mut().ok_or("no active session")?;
            if let Some(h) = s.mock_stream.take() {
                h.request_stop();
            }
            if let Some(cancel) = s.agent_cancel.take() {
                cancel.store(true, Ordering::SeqCst);
            }
            let cancel_sid = s.app_session_id.clone();
            let cancel_turn = s.host_turn_id.clone();
            let was_busy = s.fsm.state() == SessionState::Streaming
                || s.fsm.state() == SessionState::AwaitingPermission;
            let partial = s.stream_buf.trim().to_string();
            // Journal a cancel marker so UI history is not left as user-only silence.
            if was_busy {
                // I04: force-flush partial assistant before cancel marker.
                Self::maybe_flush_stream_journal(s, true, false);
                let mid = Uuid::new_v4().to_string();
                let content = if partial.is_empty() {
                    "turn_cancelled|user_stop".to_string()
                } else {
                    format!(
                        "turn_cancelled|user_stop|partial:{}",
                        partial.chars().take(200).collect::<String>()
                    )
                };
                let _ = store::append_message(
                    &s.app_session_id,
                    ChatMessageStored {
                        id: mid.clone(),
                        role: "tool".into(),
                        content: content.clone(),
                        thought: None,
                        created_at: chrono::Utc::now(),
                        is_error: false,
                        attachments: None,
                        marker: Some("turn_cancelled".into()),
                    },
                );
                let _ = app.emit(
                    "session://turn_marker",
                    serde_json::json!({
                        "sessionId": s.app_session_id,
                        "messageId": mid,
                        "marker": "turn_cancelled",
                        "reason": "user_stop",
                        "content": content,
                    }),
                );
            }
            if was_busy {
                let _ = s.fsm.end_stream();
                settle_active_skill_uses(s, crate::skill_feedback::SkillUseStatusV1::Interrupted);
            }
            s.allow_auto_wake = false;
            s.streaming_message_id = None;
            s.stream_buf.clear();
            s.stream_thought.clear();
            s.stream_last_was_assistant = false;
            s.stream_phase_id_locked = false;
            s.stream_attachments.clear();
            s.journal_throttle.reset();
            s.open_tool_ids.clear();
            s.last_stall_emit = None;
            let interrupted = interrupt_pending_interactions(s);
            let ask_activity = take_pending_ask_activity(s);
            (
                s.acp.clone(),
                ask_activity,
                interrupted,
                cancel_sid,
                cancel_turn,
            )
        };
        if let Some(turn_id) = cancel_turn {
            let registry = Arc::clone(&self.subagents);
            tokio::spawn(async move {
                registry.lock().await.cancel_turn(&cancel_sid, &turn_id);
            });
        }
        Self::publish_interrupted_session_gates(&app, interrupted);
        if let Some(activity) = ask_activity {
            record_ask_user_activity(
                &app,
                &activity.session_id,
                &activity.activity_id,
                "cancelled",
                activity.question_count,
                None,
            );
        }
        if let Some(acp) = acp {
            let _ = acp.cancel().await;
        }
        let snap = self.snapshot();
        Self::emit_state(&app, &snap);
        Ok(snap)
    }

    /// Update live Host policy (in-memory). Prefer `apply_permission_policy` for full sync.
    pub fn set_permission_policy(&self, policy: PermissionPolicy) {
        if let Some(s) = self.inner.lock().as_mut() {
            s.policy = policy;
        }
    }

    /// Soft-drop live agent so next send re-spawns with new spawn flags / config.
    /// Keeps `agent_session_id` so reconnect can `session/load`; if load fails,
    /// journal bootstrap still fills the gap.
    /// Soft-respawn the live agent process so the next connect reloads agent-visible
    /// state (MCP mcpServers injection, plugin enable/disable, prefs) without a full
    /// disconnect toast. Public for Extensions / plugin settings mutations.
    pub async fn soft_respawn(&self, app: &AppHandle) {
        let process = {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                if s.acp.is_none() {
                    return;
                }
                let acp = s.acp.take();
                let session_id = s.app_session_id.clone();
                // Prefer resume on next connect; bootstrap only if load fails.
                s.needs_history_bootstrap = false;
                s.fsm.soft_disconnect();
                // New process gets a new id on next connect.
                s.process_id = String::new();
                acp.map(|acp| (session_id, acp))
            } else {
                None
            }
        };
        if let Some((session_id, acp)) = process {
            Self::settle_automation_before_host_kill(
                &session_id,
                "Host intentionally restarted the Runtime process",
            );
            acp.kill().await;
            Self::emit_state(app, &self.snapshot());
        }
    }

    /// A sandbox profile is spawn-critical. Drop the live process and every
    /// mismatched warm process so none can be reused under the new request.
    pub async fn apply_sandbox_profile(&self, app: &AppHandle, profile: &str) {
        let requested = crate::runtime_compat::SandboxProfileV1::parse(profile);
        let live_mismatch = self
            .inner
            .lock()
            .as_ref()
            .and_then(|session| session.acp.as_ref())
            .is_some_and(|client| client.sandbox_application().requested != requested.as_str());
        if live_mismatch {
            self.soft_respawn(app).await;
        }

        let stale = {
            let mut parked = self.parked.lock();
            let ids: Vec<String> = parked
                .iter()
                .filter(|(_, entry)| {
                    entry.acp.sandbox_application().requested != requested.as_str()
                })
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| parked.remove(&id).map(|entry| entry.acp))
                .collect::<Vec<_>>()
        };
        for client in stale {
            client.kill().await;
        }
    }

    /// Apply permission: Host policy + agent-home config + respawn when process flags change.
    pub async fn apply_permission_policy(
        &self,
        app: &AppHandle,
        policy_str: &str,
    ) -> Result<(), String> {
        let policy = PermissionPolicy::parse(policy_str);
        let settings = store::load_settings();
        let _ = crate::agent_prefs::sync_permission_to_agent_profile(
            &settings.session_data_mode,
            policy.as_str(),
        );

        let need_respawn = {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                let prev = s.policy;
                s.policy = policy;
                s.meta.permission_policy = Some(policy.as_str().into());
                let _ = store::update_session_meta(&s.meta);
                // Any policy change can affect agent-side enforcement / --always-approve.
                prev != policy && s.acp.is_some()
            } else {
                false
            }
        };
        if need_respawn {
            self.soft_respawn(app).await;
        }
        Ok(())
    }

    /// Apply model id on the live ACP session (best-effort session/set_model).
    pub async fn set_model(&self, model_id: String) -> Result<(), String> {
        let model_id = model_id.trim().to_string();
        if model_id.is_empty() {
            return Err("model id empty".into());
        }
        // Store composer preference; agent receives channel-resolved id.
        let agent_model = crate::providers::agent_spawn_model_id(&model_id);
        let (target_session_id, acp) = {
            let guard = self.inner.lock();
            match guard.as_ref() {
                Some(session) => (Some(session.app_session_id.clone()), session.acp.clone()),
                None => (None, None),
            }
        };
        if let Some(acp) = acp {
            acp.set_model(&agent_model).await?;
        }
        if let Some(target_session_id) = target_session_id {
            let mut guard = self.inner.lock();
            if let Some(session) = guard.as_mut() {
                // A model RPC can await while the user changes tasks. Never
                // stamp that result onto a different live session.
                if session.app_session_id == target_session_id {
                    session.model_id = Some(model_id.clone());
                    session.meta.model_id = Some(model_id);
                    let _ = store::update_session_meta(&session.meta);
                }
            }
        }
        Ok(())
    }

    /// Apply product mode via session/set_mode; soft-respawn if agent rejects.
    pub async fn apply_product_mode(&self, app: &AppHandle, mode: String) -> Result<(), String> {
        let mode = mode.trim().to_ascii_lowercase();
        if !matches!(mode.as_str(), "agent" | "plan" | "ask") {
            return Err(format!("invalid mode: {mode}"));
        }
        let acp = {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                let same = s.product_mode.as_deref() == Some(mode.as_str());
                s.product_mode = Some(mode.clone());
                s.meta.mode = Some(mode.clone());
                let _ = store::update_session_meta(&s.meta);
                if same {
                    None
                } else {
                    s.acp.clone()
                }
            } else {
                None
            }
        };
        if let Some(acp) = acp {
            if let Err(e) = acp.set_mode(&mode).await {
                tracing::warn!("set_mode failed, soft-respawn: {e}");
                self.soft_respawn(app).await;
            }
        }
        Ok(())
    }

    /// Soft-respawn when MCP enable prefs change so the next connect injects
    /// the updated `mcpServers` set (and agent-home config is re-read).
    pub async fn apply_extensions_mcp_change(&self, app: &AppHandle) {
        let live = {
            let guard = self.inner.lock();
            guard.as_ref().map(|s| s.acp.is_some()).unwrap_or(false)
        };
        if live {
            tracing::info!("extensions: MCP prefs changed — soft-respawn live agent");
            self.soft_respawn(app).await;
        }
    }

    /// Record desired effort. CLI has no mid-session set_effort RPC; soft-drop the
    /// live agent so the next connect re-spawns with `--reasoning-effort`.
    pub async fn set_effort_and_respawn_needed(
        &self,
        app: &AppHandle,
        effort: String,
    ) -> Result<(), String> {
        let effort = effort.trim().to_string();
        if !matches!(effort.as_str(), "high" | "medium" | "low") {
            return Err(format!("invalid effort: {effort}"));
        }
        let need = {
            let mut guard = self.inner.lock();
            if let Some(s) = guard.as_mut() {
                let same = s.effort.as_deref() == Some(effort.as_str());
                s.effort = Some(effort.clone());
                s.meta.effort = Some(effort);
                let _ = store::update_session_meta(&s.meta);
                !same && s.acp.is_some()
            } else {
                false
            }
        };
        if need {
            self.soft_respawn(app).await;
        }
        Ok(())
    }

    pub fn current_context_ids(&self) -> (Option<String>, Option<String>) {
        let guard = self.inner.lock();
        match guard.as_ref() {
            Some(s) => (s.meta.project_id.clone(), Some(s.app_session_id.clone())),
            None => (None, None),
        }
    }

    fn host_permission_options() -> serde_json::Value {
        serde_json::json!([
            { "optionId": "allow_once", "kind": "allow_once", "name": "Allow once" },
            { "optionId": "allow_always", "kind": "allow_always", "name": "Allow for session" },
            { "optionId": "reject_once", "kind": "reject_once", "name": "Reject" }
        ])
    }

    fn host_decision_from_outcome(
        outcome: PermissionOutcome,
    ) -> agent_loop::HostToolPermissionDecision {
        match outcome {
            PermissionOutcome::Cancelled => agent_loop::HostToolPermissionDecision::Cancelled,
            PermissionOutcome::Selected { option_id } => {
                let id = option_id.to_ascii_lowercase();
                if id.contains("reject") || id.contains("deny") {
                    agent_loop::HostToolPermissionDecision::Deny
                } else {
                    agent_loop::HostToolPermissionDecision::Allow
                }
            }
        }
    }

    fn install_host_permission(
        &self,
        app_sid: &str,
        req: &agent_loop::HostToolPermission,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<PermissionOutcome>,
            UiPermissionRequest,
            InteractionSnapshotV1,
            bool,
            bool,
            bool,
        ),
        String,
    > {
        fn install(
            session: &mut LiveSession,
            req: &agent_loop::HostToolPermission,
        ) -> Result<
            (
                tokio::sync::oneshot::Receiver<PermissionOutcome>,
                UiPermissionRequest,
                InteractionSnapshotV1,
                bool,
                bool,
            ),
            String,
        > {
            if session.pending_permission.is_some() {
                return Err("permission already pending".into());
            }
            session.host_rpc_seq = session.host_rpc_seq.saturating_add(1);
            let rpc_id = session.host_rpc_seq;
            let root = session.project_path.as_ref().map(std::path::PathBuf::from);
            let sk = permission_scope_key(
                &req.tool_name,
                &req.path_target,
                &req.command,
                root.as_deref(),
                &req.title,
            );
            let auto = may_auto_allow(
                session.policy,
                &session.allow_cache,
                &sk,
                root.as_deref(),
                &req.path_target,
                &req.tool_name,
                &req.command,
            );
            let auto_deny = !auto && may_auto_deny(session.policy);
            let options = SessionManager::host_permission_options();
            let snapshot = InteractionSnapshotV1::new(
                &session.app_session_id,
                &session.process_id,
                rpc_id,
                Some(req.tool_call_id.clone()),
                InteractionPayloadV1::Permission {
                    tool_name: req.tool_name.clone(),
                    title: req.title.clone(),
                    preview: req.preview.clone(),
                    scope_key: sk,
                    options,
                },
            );
            let (tx, rx) = tokio::sync::oneshot::channel();
            let pending = PendingPermission {
                interaction: snapshot.clone(),
                host_reply: Some(tx),
            };
            let request = pending.ui_payload();
            session.pending_permission = Some(pending);
            let _ = session.fsm.await_permission();
            SessionManager::touch_activity_locked(session);
            Ok((rx, request, snapshot, auto, auto_deny))
        }

        {
            let mut live = self.inner.lock();
            if let Some(session) = live
                .as_mut()
                .filter(|session| session.app_session_id == app_sid)
            {
                let (rx, request, snapshot, auto, auto_deny) = install(session, req)?;
                return Ok((rx, request, snapshot, auto, auto_deny, false));
            }
        }
        let mut background = self.background.lock();
        let session = background
            .get_mut(app_sid)
            .ok_or_else(|| format!("session not active: {app_sid}"))?;
        let (rx, request, snapshot, auto, auto_deny) = install(session, req)?;
        Ok((rx, request, snapshot, auto, auto_deny, true))
    }

    async fn request_host_tool_permission(
        self: &Arc<Self>,
        app: AppHandle,
        app_sid: String,
        req: agent_loop::HostToolPermission,
    ) -> agent_loop::HostToolPermissionDecision {
        let (rx, request, snapshot, auto, auto_deny, background) =
            match self.install_host_permission(&app_sid, &req) {
                Ok(installed) => installed,
                Err(error) => {
                    tracing::warn!("host permission install failed: {error}");
                    return agent_loop::HostToolPermissionDecision::Cancelled;
                }
            };
        Self::publish_interaction(&app, &snapshot);
        let automatic_option = if auto {
            pick_option_id(&request.options, "allow_once")
                .or_else(|| pick_option_id(&request.options, "allow_always"))
                .map(|option_id| ("allow", option_id))
        } else if auto_deny {
            Some((
                "deny",
                pick_option_id(&request.options, "reject_once")
                    .or_else(|| pick_option_id(&request.options, "reject"))
                    .unwrap_or_else(|| "reject_once".into()),
            ))
        } else {
            None
        };
        if let Some((decision, option_id)) = automatic_option {
            if let Err(error) = self
                .resolve_permission(
                    app.clone(),
                    request.rpc_id,
                    decision.to_string(),
                    Some(option_id),
                    None,
                    Some(request.session_id.clone()),
                    Some(request.interaction_id.clone()),
                )
                .await
            {
                tracing::warn!("automatic host permission response failed: {error}");
                let _ = app.emit("session://permission", &request);
                if background {
                    let _ = app.emit(
                        "session://background_permission",
                        serde_json::json!({ "sessionId": request.session_id }),
                    );
                }
                Self::emit_state(&app, &self.snapshot());
            }
        } else {
            let _ = app.emit("session://permission", &request);
            if background {
                let _ = app.emit(
                    "session://background_permission",
                    serde_json::json!({ "sessionId": request.session_id }),
                );
            }
            Self::emit_state(&app, &self.snapshot());
        }
        match rx.await {
            Ok(outcome) => Self::host_decision_from_outcome(outcome),
            Err(_) => agent_loop::HostToolPermissionDecision::Cancelled,
        }
    }

    fn install_host_ask_user(
        &self,
        app_sid: &str,
        req: &agent_loop::HostAskUserRequest,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<AskUserOutcome>,
            UiAskUserRequest,
            InteractionSnapshotV1,
            bool,
        ),
        String,
    > {
        fn install(
            session: &mut LiveSession,
            req: &agent_loop::HostAskUserRequest,
        ) -> Result<
            (
                tokio::sync::oneshot::Receiver<AskUserOutcome>,
                UiAskUserRequest,
                InteractionSnapshotV1,
            ),
            String,
        > {
            if session.pending_ask_user.is_some() {
                return Err("ask_user_question already pending".into());
            }
            let parsed = parse_ask_user_question_params(&req.arguments);
            if parsed.questions.is_empty() {
                return Err("ask_user_question requires a question".into());
            }
            session.host_rpc_seq = session.host_rpc_seq.saturating_add(1);
            let rpc_id = session.host_rpc_seq;
            let tool_call_id = if req.tool_call_id.trim().is_empty() {
                None
            } else {
                Some(req.tool_call_id.clone())
            };
            let activity_id = ask_user_activity_id(tool_call_id.as_deref(), rpc_id);
            let snapshot = InteractionSnapshotV1::new(
                &session.app_session_id,
                &session.process_id,
                rpc_id,
                tool_call_id.clone(),
                InteractionPayloadV1::AskUser {
                    questions: parsed.questions.clone(),
                    partial_answers: None,
                },
            );
            let (tx, rx) = tokio::sync::oneshot::channel();
            let pending = PendingAskUser {
                interaction: snapshot.clone(),
                rpc_id,
                tool_call_id,
                activity_id,
                questions: parsed.questions,
                partial_answers: None,
                raw: req.arguments.clone(),
                resolving: false,
                host_reply: Some(tx),
            };
            let request = pending.ui_payload(&session.app_session_id);
            session.pending_ask_user = Some(pending);
            SessionManager::touch_activity_locked(session);
            Ok((rx, request, snapshot))
        }

        {
            let mut live = self.inner.lock();
            if let Some(session) = live
                .as_mut()
                .filter(|session| session.app_session_id == app_sid)
            {
                let (rx, request, snapshot) = install(session, req)?;
                return Ok((rx, request, snapshot, false));
            }
        }
        let mut background = self.background.lock();
        let session = background
            .get_mut(app_sid)
            .ok_or_else(|| format!("session not active: {app_sid}"))?;
        let (rx, request, snapshot) = install(session, req)?;
        Ok((rx, request, snapshot, true))
    }

    async fn request_host_ask_user(
        self: &Arc<Self>,
        app: AppHandle,
        app_sid: String,
        req: agent_loop::HostAskUserRequest,
    ) -> agent_loop::HostAskUserDecision {
        let (rx, request, snapshot, background) = match self.install_host_ask_user(&app_sid, &req) {
            Ok(installed) => installed,
            Err(error) => {
                tracing::warn!("host ask_user install failed: {error}");
                return agent_loop::HostAskUserDecision::Cancelled;
            }
        };
        Self::publish_interaction(&app, &snapshot);
        let _ = app.emit("session://ask_user", &request);
        if background {
            let _ = app.emit(
                "session://background_ask_user",
                serde_json::json!({ "sessionId": request.session_id }),
            );
        }
        Self::emit_state(&app, &self.snapshot());
        match rx.await {
            Ok(AskUserOutcome::Accepted { answers }) => {
                agent_loop::HostAskUserDecision::Accepted { answers }
            }
            Ok(AskUserOutcome::Cancelled) | Err(_) => agent_loop::HostAskUserDecision::Cancelled,
        }
    }

    fn prepare_permission_resolution(
        &self,
        session_id: Option<&str>,
        rpc_id: Option<u64>,
        interaction_id: Option<&str>,
    ) -> Result<(PermissionResolveTarget, InteractionSnapshotV1), String> {
        fn prepare(
            session: &mut LiveSession,
            rpc_id: Option<u64>,
            interaction_id: Option<&str>,
        ) -> Result<(PermissionResolveTarget, InteractionSnapshotV1), String> {
            let pending = session
                .pending_permission
                .as_mut()
                .ok_or_else(|| "no pending permission request".to_string())?;
            pending.interaction.claim(interaction_id, rpc_id)?;
            let snapshot = pending.interaction.clone();
            let reply = match pending.host_reply.take() {
                Some(tx) => PermissionReplyChannel::Host(tx),
                None => match session.acp.clone() {
                    Some(acp) => PermissionReplyChannel::Acp(acp),
                    None => {
                        pending.interaction.restore_pending();
                        return Err("ACP client missing".into());
                    }
                },
            };
            SessionManager::touch_activity_locked(session);
            Ok((
                PermissionResolveTarget {
                    session_id: session.app_session_id.clone(),
                    process_id: session.process_id.clone(),
                    interaction_id: snapshot.interaction_id.clone(),
                    rpc_id: snapshot.rpc_id,
                    reply: Some(reply),
                },
                snapshot,
            ))
        }

        let requested = session_id.map(str::trim).filter(|value| !value.is_empty());
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if requested.is_none_or(|id| id == session.app_session_id) {
                    return prepare(session, rpc_id, interaction_id);
                }
            } else if requested.is_none() {
                return Err("no session".into());
            }
        }
        let requested = requested.ok_or_else(|| "no pending permission request".to_string())?;
        let mut background = self.background.lock();
        let session = background
            .get_mut(requested)
            .ok_or_else(|| format!("session not active: {requested}"))?;
        prepare(session, rpc_id, interaction_id)
    }

    fn restore_permission_after_write_failure(
        &self,
        target: &PermissionResolveTarget,
    ) -> Option<InteractionSnapshotV1> {
        fn restore(
            session: &mut LiveSession,
            target: &PermissionResolveTarget,
        ) -> Option<InteractionSnapshotV1> {
            if session.process_id != target.process_id {
                return None;
            }
            let pending = session.pending_permission.as_mut()?;
            if pending.interaction.interaction_id != target.interaction_id
                || pending.interaction.rpc_id != target.rpc_id
                || !pending.interaction.restore_pending()
            {
                return None;
            }
            Some(pending.interaction.clone())
        }

        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if session.app_session_id == target.session_id {
                    if let Some(snapshot) = restore(session, target) {
                        return Some(snapshot);
                    }
                }
            }
        }
        let mut background = self.background.lock();
        background
            .get_mut(&target.session_id)
            .and_then(|session| restore(session, target))
    }

    fn clear_resolved_permission(
        &self,
        target: &PermissionResolveTarget,
        cache_scope: Option<&str>,
    ) -> (
        Option<InteractionSnapshotV1>,
        bool,
        Option<(String, String, String)>,
        bool,
    ) {
        fn clear(
            session: &mut LiveSession,
            target: &PermissionResolveTarget,
            cache_scope: Option<&str>,
        ) -> Option<(
            InteractionSnapshotV1,
            bool,
            Option<(String, String, String)>,
        )> {
            if session.process_id != target.process_id {
                return None;
            }
            let pending = session.pending_permission.as_mut()?;
            if pending.interaction.interaction_id != target.interaction_id
                || pending.interaction.rpc_id != target.rpc_id
                || !pending.interaction.resolve()
            {
                return None;
            }
            let snapshot = pending.interaction.clone();
            session.pending_permission = None;
            if let Some(scope) = cache_scope {
                session.allow_cache.allow(scope.to_string());
            }
            if session.fsm.state() == SessionState::AwaitingPermission {
                let _ = session.fsm.permission_resolved_continue();
            }
            let finish = SessionManager::try_finish_deferred_prompt_complete(session);
            Some((snapshot, finish.is_some(), finish.flatten()))
        }

        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if session.app_session_id == target.session_id {
                    if let Some((snapshot, finished, empty)) = clear(session, target, cache_scope) {
                        return (Some(snapshot), finished, empty, false);
                    }
                }
            }
        }
        let mut background = self.background.lock();
        if let Some(session) = background.get_mut(&target.session_id) {
            if let Some((snapshot, finished, empty)) = clear(session, target, cache_scope) {
                return (Some(snapshot), finished, empty, true);
            }
        }
        (None, false, None, false)
    }

    pub async fn resolve_permission(
        self: &Arc<Self>,
        app: AppHandle,
        rpc_id: u64,
        decision: String,
        option_id: Option<String>,
        scope: Option<String>,
        session_id: Option<String>,
        interaction_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let (mut target, resolving) = self.prepare_permission_resolution(
            session_id.as_deref(),
            Some(rpc_id),
            interaction_id.as_deref(),
        )?;
        Self::publish_interaction(&app, &resolving);
        let outcome = match decision.as_str() {
            "cancel" => PermissionOutcome::Cancelled,
            "deny" => PermissionOutcome::Selected {
                option_id: option_id.unwrap_or_else(|| "reject".into()),
            },
            _ => PermissionOutcome::Selected {
                option_id: option_id.unwrap_or_else(|| "allow_once".into()),
            },
        };
        match target.reply.take() {
            Some(PermissionReplyChannel::Acp(acp)) => {
                if let Err(error) = acp.respond_permission(target.rpc_id, outcome).await {
                    if let Some(restored) = self.restore_permission_after_write_failure(&target) {
                        Self::publish_interaction(&app, &restored);
                    }
                    return Err(error);
                }
            }
            Some(PermissionReplyChannel::Host(tx)) => {
                if tx.send(outcome).is_err() {
                    // Turn already cancelled; still clear the pending snapshot below.
                }
            }
            None => return Err("permission reply already consumed".into()),
        }
        let cache_scope = matches!(decision.as_str(), "allow_session" | "allow_for_session")
            .then_some(scope.as_deref())
            .flatten();
        let (resolved, finished, empty_run, was_background) =
            self.clear_resolved_permission(&target, cache_scope);
        if let Some(resolved) = resolved.as_ref() {
            Self::publish_interaction(&app, resolved);
        }
        if finished && was_background {
            self.promote_background_ready_to_parked(&target.session_id);
        }
        let snap = self.snapshot();
        Self::emit_state(&app, &snap);
        Self::emit_empty_run_if_any(&app, empty_run);
        Ok(snap)
    }

    /// Resolve pending `_x.ai/exit_plan_mode` (Approve & build / request changes / abandon).
    ///
    /// `decision`: "approved" | "cancelled" | "abandoned"
    /// Optional `feedback` is sent only with cancelled (revise).
    pub async fn resolve_plan(
        &self,
        app: AppHandle,
        decision: String,
        feedback: Option<String>,
        rpc_id: Option<u64>,
        session_id: Option<String>,
        interaction_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        fn prepare(
            session: &mut LiveSession,
            rpc_id: Option<u64>,
            interaction_id: Option<&str>,
        ) -> Result<(PlanResolveTarget, InteractionSnapshotV1), String> {
            let acp = session
                .acp
                .clone()
                .ok_or_else(|| "ACP client missing".to_string())?;
            let pending = session
                .pending_plan
                .as_mut()
                .ok_or_else(|| "no pending plan approval".to_string())?;
            pending.interaction.claim(interaction_id, rpc_id)?;
            let snapshot = pending.interaction.clone();
            SessionManager::touch_activity_locked(session);
            Ok((
                PlanResolveTarget {
                    session_id: session.app_session_id.clone(),
                    process_id: session.process_id.clone(),
                    interaction_id: snapshot.interaction_id.clone(),
                    rpc_id: snapshot.rpc_id,
                    acp,
                },
                snapshot,
            ))
        }

        let requested = session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let prepared = {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if requested.is_none_or(|id| id == session.app_session_id) {
                    Some(prepare(session, rpc_id, interaction_id.as_deref()))
                } else {
                    None
                }
            } else {
                None
            }
        };
        let (target, resolving) = if let Some(prepared) = prepared {
            prepared?
        } else {
            let requested = requested.ok_or_else(|| "no pending plan approval".to_string())?;
            let mut background = self.background.lock();
            let session = background
                .get_mut(requested)
                .ok_or_else(|| format!("session not active: {requested}"))?;
            prepare(session, rpc_id, interaction_id.as_deref())?
        };
        Self::publish_interaction(&app, &resolving);

        let artifact_decision = decision.clone();
        let artifact_feedback = feedback.clone();
        if let Err(error) = target
            .acp
            .respond_exit_plan_mode(target.rpc_id, &decision, feedback)
            .await
        {
            let restored = {
                let mut live = self.inner.lock();
                live.as_mut().and_then(|session| {
                    if session.app_session_id != target.session_id
                        || session.process_id != target.process_id
                    {
                        return None;
                    }
                    let pending = session.pending_plan.as_mut()?;
                    if pending.interaction.interaction_id != target.interaction_id
                        || !pending.interaction.restore_pending()
                    {
                        return None;
                    }
                    Some(pending.interaction.clone())
                })
            }
            .or_else(|| {
                let mut background = self.background.lock();
                let session = background.get_mut(&target.session_id)?;
                if session.process_id != target.process_id {
                    return None;
                }
                let pending = session.pending_plan.as_mut()?;
                if pending.interaction.interaction_id != target.interaction_id
                    || !pending.interaction.restore_pending()
                {
                    return None;
                }
                Some(pending.interaction.clone())
            });
            if let Some(restored) = restored.as_ref() {
                Self::publish_interaction(&app, restored);
            }
            return Err(error);
        }

        // The durable artifact advances only after Runtime accepted the RPC.
        // A sidecar failure cannot make the already-written RPC safely retryable.
        match crate::plan_artifacts::resolve_plan(
            &target.session_id,
            &target.interaction_id,
            &artifact_decision,
            artifact_feedback.as_deref(),
        ) {
            Ok(artifact) => Self::emit_plan_artifact_pair(&app, &artifact, None),
            Err(error) => {
                tracing::warn!(
                    session_id = %target.session_id,
                    interaction_id = %target.interaction_id,
                    "persist Plan artifact resolution failed: {error}"
                );
            }
        }

        let (resolved, finished, empty_run, was_background) = {
            let mut live = self.inner.lock();
            let live_result = live.as_mut().and_then(|session| {
                if session.app_session_id != target.session_id
                    || session.process_id != target.process_id
                {
                    return None;
                }
                let pending = session.pending_plan.as_mut()?;
                if pending.interaction.interaction_id != target.interaction_id
                    || !pending.interaction.resolve()
                {
                    return None;
                }
                let snapshot = pending.interaction.clone();
                session.pending_plan = None;
                let finish = SessionManager::try_finish_deferred_prompt_complete(session);
                Some((snapshot, finish.is_some(), finish.flatten(), false))
            });
            drop(live);
            if let Some(result) = live_result {
                result
            } else {
                let mut background = self.background.lock();
                let result = background.get_mut(&target.session_id).and_then(|session| {
                    if session.process_id != target.process_id {
                        return None;
                    }
                    let pending = session.pending_plan.as_mut()?;
                    if pending.interaction.interaction_id != target.interaction_id
                        || !pending.interaction.resolve()
                    {
                        return None;
                    }
                    let snapshot = pending.interaction.clone();
                    session.pending_plan = None;
                    let finish = SessionManager::try_finish_deferred_prompt_complete(session);
                    Some((snapshot, finish.is_some(), finish.flatten(), true))
                });
                result.ok_or_else(|| "stale plan resolution".to_string())?
            }
        };
        Self::publish_interaction(&app, &resolved);
        if finished && was_background {
            self.promote_background_ready_to_parked(&target.session_id);
        }
        let snap = self.snapshot();
        Self::emit_state(&app, &snap);
        Self::emit_empty_run_if_any(&app, empty_run);
        Ok(snap)
    }

    /// Return the recoverable questionnaire for a specific App session.
    ///
    /// `None` keeps the legacy/current-session behavior. Background callers
    /// must pass their Sunsetz App session id; Runtime JSON-RPC ids are only
    /// unique within one ACP connection.
    pub fn pending_ask_user(&self, session_id: Option<&str>) -> Option<UiAskUserRequest> {
        let requested = session_id.map(str::trim).filter(|value| !value.is_empty());
        {
            let live = self.inner.lock();
            if let Some(session) = live.as_ref() {
                if requested.is_none_or(|id| id == session.app_session_id) {
                    if let Some(pending) = session.pending_ask_user.as_ref() {
                        return Some(pending.ui_payload(&session.app_session_id));
                    }
                    if requested.is_none() || requested == Some(session.app_session_id.as_str()) {
                        return None;
                    }
                }
            } else if requested.is_none() {
                return None;
            }
        }
        let requested = requested?;
        let background = self.background.lock();
        let session = background.get(requested)?;
        session
            .pending_ask_user
            .as_ref()
            .map(|pending| pending.ui_payload(&session.app_session_id))
    }

    /// Return every recoverable AskUser interaction across the focused and
    /// background sessions. The legacy single-session query remains available.
    ///
    /// Locks are intentionally acquired separately: focus promotion may move a
    /// session from `background` to `inner` while holding them in the opposite
    /// order. A session id is deduplicated and the output is sorted so callers
    /// receive a stable payload even across a concurrent focus transition.
    pub fn pending_interactions(&self) -> Vec<UiAskUserRequest> {
        let mut pending = Vec::new();
        {
            let live = self.inner.lock();
            if let Some(session) = live.as_ref() {
                if let Some(request) = session.pending_ask_user.as_ref() {
                    pending.push(request.ui_payload(&session.app_session_id));
                }
            }
        }
        {
            let background = self.background.lock();
            for session in background.values() {
                if let Some(request) = session.pending_ask_user.as_ref() {
                    pending.push(request.ui_payload(&session.app_session_id));
                }
            }
        }

        pending.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        pending.dedup_by(|left, right| left.session_id == right.session_id);
        pending
    }

    fn active_interaction_snapshots(&self) -> Vec<InteractionSnapshotV1> {
        fn collect(session: &LiveSession, out: &mut Vec<InteractionSnapshotV1>) {
            if let Some(pending) = session.pending_permission.as_ref() {
                out.push(pending.interaction.clone());
            }
            if let Some(pending) = session.pending_plan.as_ref() {
                out.push(pending.interaction.clone());
            }
            if let Some(pending) = session.pending_ask_user.as_ref() {
                out.push(pending.interaction.clone());
            }
        }

        let mut snapshots = Vec::new();
        {
            let live = self.inner.lock();
            if let Some(session) = live.as_ref() {
                collect(session, &mut snapshots);
            }
        }
        {
            let background = self.background.lock();
            for session in background.values() {
                collect(session, &mut snapshots);
            }
        }
        snapshots.sort_by(|left, right| {
            left.session_id
                .cmp(&right.session_id)
                .then(left.created_at.cmp(&right.created_at))
        });
        snapshots
    }

    /// Versioned interaction query. Without a session id it returns only live
    /// pending/resolving interactions; with a session id it also returns the
    /// bounded on-disk audit trail and marks orphaned process-bound rows interrupted.
    pub fn interactions_list(&self, session_id: Option<&str>) -> Vec<InteractionSnapshotV1> {
        let active = self.active_interaction_snapshots();
        let requested = session_id.map(str::trim).filter(|value| !value.is_empty());
        let Some(requested) = requested else {
            return active;
        };

        let active: Vec<_> = active
            .into_iter()
            .filter(|snapshot| snapshot.session_id == requested)
            .collect();
        let active_ids: HashSet<_> = active
            .iter()
            .map(|snapshot| snapshot.interaction_id.as_str())
            .collect();
        let mut rows = crate::interactions::load(requested);
        let mut interrupted = Vec::new();
        for row in rows.iter_mut() {
            if matches!(
                row.status,
                InteractionStatusV1::Pending | InteractionStatusV1::Resolving
            ) && !active_ids.contains(row.interaction_id.as_str())
            {
                row.set_status(InteractionStatusV1::Interrupted);
                interrupted.push(row.clone());
            }
        }
        for row in interrupted {
            if let Err(error) = crate::interactions::record(&row) {
                tracing::warn!("persist interrupted interaction: {error}");
            }
        }
        for snapshot in active {
            if let Some(row) = rows
                .iter_mut()
                .find(|row| row.interaction_id == snapshot.interaction_id)
            {
                *row = snapshot;
            } else {
                rows.push(snapshot);
            }
        }
        rows.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        rows
    }

    fn active_interaction(
        &self,
        session_id: &str,
        interaction_id: &str,
    ) -> Option<InteractionSnapshotV1> {
        self.active_interaction_snapshots()
            .into_iter()
            .find(|snapshot| {
                snapshot.session_id == session_id && snapshot.interaction_id == interaction_id
            })
    }

    pub async fn resolve_interaction_v1(
        self: &Arc<Self>,
        app: AppHandle,
        request: crate::interactions::ResolveInteractionRequestV1,
    ) -> Result<SessionSnapshot, String> {
        let snapshot = self
            .active_interaction(&request.session_id, &request.interaction_id)
            .ok_or_else(|| "interaction is not active".to_string())?;
        match snapshot.payload {
            InteractionPayloadV1::Permission { .. } => {
                self.resolve_permission(
                    app,
                    snapshot.rpc_id,
                    request.decision,
                    request.option_id,
                    request.scope_key,
                    Some(request.session_id),
                    Some(request.interaction_id),
                )
                .await
            }
            InteractionPayloadV1::Plan { .. } => {
                self.resolve_plan(
                    app,
                    request.decision,
                    request.feedback,
                    Some(snapshot.rpc_id),
                    Some(request.session_id),
                    Some(request.interaction_id),
                )
                .await
            }
            InteractionPayloadV1::AskUser { .. } => {
                self.resolve_ask_user(
                    app,
                    request.decision,
                    request.answers,
                    Some(snapshot.rpc_id),
                    Some(request.session_id),
                    Some(request.interaction_id),
                )
                .await
            }
        }
    }

    fn prepare_ask_user_resolution(
        &self,
        session_id: Option<&str>,
        rpc_id: Option<u64>,
        interaction_id: Option<&str>,
        partial_answers: Option<&serde_json::Value>,
    ) -> Result<AskUserResolveTarget, String> {
        fn prepare(
            session: &mut LiveSession,
            rpc_id: Option<u64>,
            interaction_id: Option<&str>,
            partial_answers: Option<&serde_json::Value>,
        ) -> Result<AskUserResolveTarget, String> {
            let pending = session
                .pending_ask_user
                .as_mut()
                .ok_or_else(|| "no pending ask_user_question".to_string())?;
            let pending_rpc_id = claim_pending_ask(pending, interaction_id, rpc_id)?;
            pending.partial_answers = partial_answers.cloned();
            if let InteractionPayloadV1::AskUser {
                partial_answers: stored,
                ..
            } = &mut pending.interaction.payload
            {
                *stored = partial_answers.cloned();
            }
            let reply = match pending.host_reply.take() {
                Some(tx) => AskUserReplyChannel::Host(tx),
                None => match session.acp.clone() {
                    Some(acp) => AskUserReplyChannel::Acp(acp),
                    None => {
                        pending.interaction.restore_pending();
                        pending.resolving = false;
                        return Err("ACP client missing".into());
                    }
                },
            };
            let activity_id = pending.activity_id.clone();
            let question_count = pending.questions.len();
            let snapshot = pending.interaction.clone();
            SessionManager::touch_activity_locked(session);
            Ok(AskUserResolveTarget {
                session_id: session.app_session_id.clone(),
                process_id: session.process_id.clone(),
                rpc_id: pending_rpc_id,
                activity_id,
                question_count,
                reply,
                snapshot,
            })
        }

        let requested = session_id.map(str::trim).filter(|value| !value.is_empty());
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if requested.is_none_or(|id| id == session.app_session_id) {
                    return prepare(session, rpc_id, interaction_id, partial_answers);
                }
            } else if requested.is_none() {
                return Err("no session".into());
            }
        }
        let requested = requested.ok_or_else(|| "no pending ask_user_question".to_string())?;
        let mut background = self.background.lock();
        let session = background
            .get_mut(requested)
            .ok_or_else(|| format!("session not active: {requested}"))?;
        prepare(session, rpc_id, interaction_id, partial_answers)
    }

    fn restore_ask_user_after_write_failure(
        &self,
        target: &AskUserResolveTarget,
    ) -> Option<InteractionSnapshotV1> {
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if session.app_session_id == target.session_id
                    && session.process_id == target.process_id
                {
                    if restore_pending_ask_after_failure(session, &target.process_id, target.rpc_id)
                    {
                        return session
                            .pending_ask_user
                            .as_ref()
                            .map(|pending| pending.interaction.clone());
                    }
                    return None;
                }
            }
        }
        let mut background = self.background.lock();
        if let Some(session) = background.get_mut(&target.session_id) {
            if restore_pending_ask_after_failure(session, &target.process_id, target.rpc_id) {
                return session
                    .pending_ask_user
                    .as_ref()
                    .map(|pending| pending.interaction.clone());
            }
        }
        None
    }

    /// Compare-and-clear a successfully written reverse-request, even if its
    /// session moved between focused and background while the write awaited.
    /// Returns `(resolved_snapshot, finished_deferred_turn, empty_run, was_background)`.
    fn clear_resolved_ask_user(
        &self,
        target: &AskUserResolveTarget,
    ) -> (
        Option<InteractionSnapshotV1>,
        bool,
        Option<(String, String, String)>,
        bool,
    ) {
        {
            let mut live = self.inner.lock();
            if let Some(session) = live.as_mut() {
                if session.app_session_id == target.session_id
                    && session.process_id == target.process_id
                {
                    if clear_pending_ask_after_success(session, &target.process_id, target.rpc_id) {
                        let mut snapshot = target.snapshot.clone();
                        snapshot.set_status(InteractionStatusV1::Resolved);
                        let finish = Self::try_finish_deferred_prompt_complete(session);
                        return (Some(snapshot), finish.is_some(), finish.flatten(), false);
                    }
                    return (None, false, None, false);
                }
            }
        }
        let mut background = self.background.lock();
        if let Some(session) = background.get_mut(&target.session_id) {
            if session.process_id == target.process_id {
                if clear_pending_ask_after_success(session, &target.process_id, target.rpc_id) {
                    let mut snapshot = target.snapshot.clone();
                    snapshot.set_status(InteractionStatusV1::Resolved);
                    let finish = Self::try_finish_deferred_prompt_complete(session);
                    return (Some(snapshot), finish.is_some(), finish.flatten(), true);
                }
            }
        }
        (None, false, None, false)
    }

    /// Resolve pending `_x.ai/ask_user_question` (answers or cancel).
    ///
    /// `decision`: "accepted" | "cancelled"
    /// `answers`: object map of question text → answer string (required for accepted).
    /// `session_id`: optional Sunsetz App session id; omitted for legacy focused-session calls.
    pub async fn resolve_ask_user(
        &self,
        app: AppHandle,
        decision: String,
        answers: Option<serde_json::Value>,
        rpc_id: Option<u64>,
        session_id: Option<String>,
        interaction_id: Option<String>,
    ) -> Result<SessionSnapshot, String> {
        let accepted = matches!(decision.as_str(), "accepted" | "answered" | "accept");
        let accepted_answers = answers.unwrap_or_else(|| serde_json::json!({}));
        let mut target = self.prepare_ask_user_resolution(
            session_id.as_deref(),
            rpc_id,
            interaction_id.as_deref(),
            accepted.then_some(&accepted_answers),
        )?;
        Self::publish_interaction(&app, &target.snapshot);
        let answered_count =
            accepted.then(|| answered_question_count(&accepted_answers, target.question_count));
        let outcome = if accepted {
            AskUserOutcome::Accepted {
                answers: accepted_answers,
            }
        } else {
            AskUserOutcome::Cancelled
        };
        let reply = std::mem::replace(&mut target.reply, AskUserReplyChannel::Taken);
        let write_result = match reply {
            AskUserReplyChannel::Acp(acp) => {
                acp.respond_ask_user_question(target.rpc_id, outcome).await
            }
            AskUserReplyChannel::Host(tx) => tx
                .send(outcome)
                .map_err(|_| "ask_user_question waiter dropped".to_string()),
            AskUserReplyChannel::Taken => Ok(()),
        };
        if let Err(error) = write_result {
            if let Some(restored) = self.restore_ask_user_after_write_failure(&target) {
                Self::publish_interaction(&app, &restored);
            }
            return Err(error);
        }
        let (resolved, finished, empty_run, was_background) = self.clear_resolved_ask_user(&target);
        if let Some(resolved) = resolved.as_ref() {
            Self::publish_interaction(&app, resolved);
            record_ask_user_activity(
                &app,
                &target.session_id,
                &target.activity_id,
                if accepted { "completed" } else { "cancelled" },
                target.question_count,
                answered_count,
            );
        }
        if finished && was_background {
            self.promote_background_ready_to_parked(&target.session_id);
        }
        let snap = self.snapshot();
        Self::emit_state(&app, &snap);
        Self::emit_empty_run_if_any(&app, empty_run);
        Ok(snap)
    }

    async fn disconnect_inner(&self, app: &AppHandle) {
        let (process, ask_activity, interrupted) = {
            let mut guard = self.inner.lock();
            if let Some(mut s) = guard.take() {
                if let Some(h) = s.mock_stream.take() {
                    h.request_stop();
                }
                // I04: flush any in-flight stream before dropping the process.
                Self::maybe_flush_stream_journal(&mut s, true, false);
                let interrupted = interrupt_pending_interactions(&mut s);
                let ask_activity = take_pending_ask_activity(&mut s);
                (
                    s.acp.take().map(|acp| (s.app_session_id.clone(), acp)),
                    ask_activity,
                    interrupted,
                )
            } else {
                (None, None, InterruptedSessionGates::empty())
            }
        };
        Self::publish_interrupted_session_gates(app, interrupted);
        if let Some(activity) = ask_activity {
            record_ask_user_activity(
                app,
                &activity.session_id,
                &activity.activity_id,
                "cancelled",
                activity.question_count,
                None,
            );
        }
        if let Some((session_id, acp)) = process {
            Self::settle_automation_before_host_kill(
                &session_id,
                "Host intentionally disconnected the Runtime process",
            );
            acp.kill().await;
        }
        // Keep parked warm agents — full app teardown can clear them later.
        Self::emit_state(app, &self.snapshot());
    }

    pub async fn disconnect(self: &Arc<Self>, app: AppHandle) -> Result<SessionSnapshot, String> {
        // Drop the focused process only. Parked warm agents stay until idle recycle
        // or capacity eviction so reopening another chat can unpark quickly.
        self.disconnect_inner(&app).await;
        Ok(self.snapshot())
    }

    pub async fn reattach(self: &Arc<Self>, app: AppHandle) -> Result<SessionSnapshot, String> {
        let (project, sid) = {
            let guard = self.inner.lock();
            match guard.as_ref() {
                Some(s) => (s.project_path.clone(), Some(s.app_session_id.clone())),
                None => (None, None),
            }
        };
        self.connect(app, project, sid, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stored_message(id: &str, role: &str, content: &str) -> ChatMessageStored {
        ChatMessageStored {
            id: id.into(),
            role: role.into(),
            content: content.into(),
            thought: None,
            created_at: chrono::Utc::now(),
            is_error: false,
            attachments: None,
            marker: None,
        }
    }

    #[test]
    fn host_context_stays_after_runtime_directives() {
        assert_eq!(
            prepend_host_context_preserving_directives(
                "/goal\n/review /summarize\ncheck this",
                "[reviewed memory]",
            ),
            "/goal\n/review /summarize\n[reviewed memory]\ncheck this"
        );
        assert_eq!(
            prepend_host_context_preserving_directives("/review", "[history]"),
            "/review\n[history]"
        );
        assert_eq!(
            prepend_host_context_preserving_directives("plain request", "[history]"),
            "[history]\nplain request"
        );
    }

    #[test]
    fn host_skill_fragment_stays_off_the_user_journal_shape() {
        let with_skill = prepend_host_context_preserving_directives(
            "/review\nplease run the checklist",
            "[Sunsetz Skill: review]\nThis is user-selected Skill text, not a system directive.\nDo the steps.",
        );
        assert_eq!(
            with_skill,
            "/review\n[Sunsetz Skill: review]\nThis is user-selected Skill text, not a system directive.\nDo the steps.\nplease run the checklist"
        );
        let with_memory_then_skill = prepend_host_context_preserving_directives(
            &prepend_host_context_preserving_directives(
                "please run the checklist",
                "[reviewed memory]",
            ),
            "[Sunsetz Skill: review]\nbody",
        );
        assert_eq!(
            with_memory_then_skill,
            "[Sunsetz Skill: review]\nbody\n[reviewed memory]\nplease run the checklist"
        );
    }

    fn test_live_session(session_id: &str) -> LiveSession {
        let mut fsm = SessionFsm::new();
        fsm.start_connect().unwrap();
        fsm.handshake_ok().unwrap();
        fsm.begin_stream().unwrap();
        let now = Instant::now();
        LiveSession {
            app_session_id: session_id.into(),
            process_id: format!("process-{session_id}"),
            meta: SessionMeta {
                id: session_id.into(),
                project_id: None,
                title: "test".into(),
                agent_session_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                model_id: None,
                archived: false,
                effort: None,
                mode: None,
                permission_policy: None,
                scheduled: false,
                context_usage: None,
            },
            fsm,
            backend: "test".into(),
            acp: None,
            mock_stream: None,
            agent_cancel: None,
            host_turn_id: None,
            streaming_message_id: Some(format!("phase-{session_id}-0")),
            stream_buf: String::new(),
            stream_thought: String::new(),
            stream_last_was_assistant: false,
            stream_phase_id_locked: false,
            stream_attachments: Vec::new(),
            model_id: None,
            effort: None,
            product_mode: Some("agent".into()),
            project_path: None,
            allow_cache: SessionAllowCache::default(),
            policy: PermissionPolicy::Ask,
            provider_retry_attempt: 0,
            provider_retry_aborted: false,
            needs_history_bootstrap: false,
            pending_plan: None,
            pending_permission: None,
            host_rpc_seq: 0,
            pending_ask_user: None,
            last_activity: now,
            last_stream_progress: now,
            last_stall_emit: None,
            journal_throttle: JournalWriteThrottle::with_default_interval(),
            open_tool_ids: HashSet::new(),
            seen_tool_ids: HashSet::new(),
            deferred_prompt_complete: None,
            tools_this_turn: 0,
            active_skill_uses: Vec::new(),
            pending_skill_settlement: None,
            allow_auto_wake: false,
        }
    }

    struct IsolatedHome {
        home: std::path::PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl IsolatedHome {
        fn new(label: &str) -> Self {
            let lock = crate::runtime_compat::lock_test_process_env();
            let home = std::env::temp_dir().join(format!(
                "sunsetz-session-mgr-{label}-{}-{}",
                std::process::id(),
                Uuid::new_v4()
            ));
            std::fs::create_dir_all(&home).unwrap();
            std::env::set_var("SUNSETZ_HOME", &home);
            Self { home, _lock: lock }
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            std::env::remove_var("SUNSETZ_HOME");
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    fn ready_kernel_session(session_id: &str) -> LiveSession {
        let mut live = test_live_session(session_id);
        live.fsm.end_stream().unwrap();
        live.streaming_message_id = None;
        live.backend = agent_loop::BACKEND_SUNSETZ.into();
        live
    }

    #[test]
    fn install_background_kernel_does_not_steal_live() {
        let _home = IsolatedHome::new("bg-install");
        let mgr = SessionManager::new();
        *mgr.inner.lock() = Some(ready_kernel_session("live"));
        let meta = store::create_session(None, Some("Scheduled".into()), true).unwrap();
        mgr.install_background_kernel_session(&meta.id, None)
            .unwrap();
        assert_eq!(
            mgr.inner
                .lock()
                .as_ref()
                .map(|session| session.app_session_id.as_str()),
            Some("live")
        );
        assert!(mgr.background.lock().contains_key(&meta.id));
        assert_eq!(mgr.snapshot().session_id.as_deref(), Some("live"));
        assert!(mgr.snapshot().busy_session_ids.is_empty());
    }

    #[test]
    fn snapshot_lists_background_busy_ids_without_changing_live() {
        let mgr = SessionManager::new();
        *mgr.inner.lock() = Some(ready_kernel_session("live"));
        let mut background = test_live_session("sched");
        background.backend = agent_loop::BACKEND_SUNSETZ.into();
        mgr.background.lock().insert("sched".into(), background);
        let snap = mgr.snapshot();
        assert_eq!(snap.session_id.as_deref(), Some("live"));
        assert_eq!(snap.busy_session_ids, vec!["sched".to_string()]);
    }

    #[test]
    fn prepare_user_send_targets_background_session() {
        let _home = IsolatedHome::new("bg-send");
        let meta = store::create_session(None, Some("Scheduled".into()), true).unwrap();
        let mgr = SessionManager::new();
        *mgr.inner.lock() = Some(ready_kernel_session("live"));
        let mut background = ready_kernel_session(&meta.id);
        background.meta.id = meta.id.clone();
        mgr.background.lock().insert(meta.id.clone(), background);
        let prepared = mgr
            .prepare_user_send(
                Some(&meta.id),
                "turn-1",
                "hello scheduled",
                "hello scheduled",
                None,
                None,
                &[],
            )
            .unwrap();
        assert_eq!(prepared.1, meta.id);
        assert_eq!(
            mgr.inner
                .lock()
                .as_ref()
                .map(|session| session.app_session_id.as_str()),
            Some("live")
        );
        assert_eq!(
            mgr.background
                .lock()
                .get(&meta.id)
                .map(|session| session.fsm.state()),
            Some(SessionState::Streaming)
        );
        assert_eq!(mgr.snapshot().busy_session_ids, vec![meta.id.clone()]);
    }

    #[test]
    fn sunsetz_busy_session_parks_into_background_without_acp() {
        let mgr = SessionManager::new();
        *mgr.inner.lock() = Some(test_live_session("sid-a"));
        mgr.try_park_live().unwrap();
        assert!(mgr.inner.lock().is_none());
        assert!(mgr.background.lock().contains_key("sid-a"));
        assert_eq!(
            mgr.background
                .lock()
                .get("sid-a")
                .map(|session| session.process_id.clone())
                .as_deref(),
            Some("process-sid-a")
        );
    }

    #[test]
    fn sunsetz_ready_session_without_acp_is_not_parked() {
        let mgr = SessionManager::new();
        let mut live = test_live_session("sid-ready");
        live.fsm.end_stream().unwrap();
        live.streaming_message_id = None;
        *mgr.inner.lock() = Some(live);
        mgr.try_park_live().unwrap();
        assert!(mgr.background.lock().is_empty());
        assert_eq!(
            mgr.inner
                .lock()
                .as_ref()
                .map(|session| session.app_session_id.as_str()),
            Some("sid-ready")
        );
    }

    #[test]
    fn prepare_session_for_wake_begins_stream_from_ready() {
        let mgr = SessionManager::new();
        let mut live = test_live_session("sid-wake");
        live.fsm.end_stream().unwrap();
        live.streaming_message_id = None;
        live.backend = agent_loop::BACKEND_SUNSETZ.into();
        live.allow_auto_wake = true;
        *mgr.inner.lock() = Some(live);
        assert!(mgr.prepare_session_for_wake("sid-wake"));
        let session = mgr.inner.lock();
        let session = session.as_ref().unwrap();
        assert_eq!(session.fsm.state(), SessionState::Streaming);
        assert!(session.streaming_message_id.is_some());
        assert!(!session.allow_auto_wake);
    }

    #[test]
    fn steer_ready_session_holds_wake_for_user_send() {
        let mgr = SessionManager::new();
        let mut live = test_live_session("sid-steer");
        live.fsm.end_stream().unwrap();
        live.streaming_message_id = None;
        live.backend = agent_loop::BACKEND_SUNSETZ.into();
        live.allow_auto_wake = false;
        *mgr.inner.lock() = Some(live);
        let inspect = mgr.inspect_session_for_wake("sid-steer").unwrap();
        assert_eq!(
            subagents::decide_auto_wake(
                1,
                0,
                inspect.streaming,
                inspect.deferred_prompt_complete,
                inspect.awaiting_permission,
                inspect.allow_auto_wake,
            ),
            subagents::AutoWakeDecision::HoldForUserSend
        );
    }

    #[test]
    fn deferred_prompt_complete_starts_wake_when_this_turn_children_are_done() {
        let mgr = SessionManager::new();
        let mut live = test_live_session("sid-join");
        live.backend = agent_loop::BACKEND_SUNSETZ.into();
        live.deferred_prompt_complete = Some("end_turn".into());
        live.allow_auto_wake = true;
        live.host_turn_id = Some("t1".into());
        *mgr.inner.lock() = Some(live);
        let inspect = mgr.inspect_session_for_wake("sid-join").unwrap();
        assert_eq!(
            subagents::decide_auto_wake(
                2,
                0,
                inspect.streaming,
                inspect.deferred_prompt_complete,
                inspect.awaiting_permission,
                inspect.allow_auto_wake,
            ),
            subagents::AutoWakeDecision::Start
        );
        assert_eq!(
            subagents::decide_auto_wake(
                2,
                1,
                inspect.streaming,
                inspect.deferred_prompt_complete,
                inspect.awaiting_permission,
                inspect.allow_auto_wake,
            ),
            subagents::AutoWakeDecision::WaitForThisTurn
        );
    }

    fn pending_ask(session_id: &str, rpc_id: u64) -> PendingAskUser {
        let questions = vec![AskUserQuestionItem {
            id: "q-1".into(),
            question: "Choose?".into(),
            options: Vec::new(),
            multi_select: false,
        }];
        PendingAskUser {
            interaction: InteractionSnapshotV1::new(
                session_id,
                &format!("process-{session_id}"),
                rpc_id,
                Some(format!("tool-{session_id}")),
                InteractionPayloadV1::AskUser {
                    questions: questions.clone(),
                    partial_answers: None,
                },
            ),
            rpc_id,
            tool_call_id: Some(format!("tool-{session_id}")),
            activity_id: format!("tool-{session_id}"),
            questions,
            partial_answers: None,
            raw: json!({
                "sessionId": format!("runtime-{session_id}"),
                "futureField": { "kept": true }
            }),
            resolving: false,
            host_reply: None,
        }
    }

    #[test]
    fn attachment_parser_reads_only_absolute_tail_refs() {
        let prompt = "Mention @/tmp/not-an-attachment in prose.\nKeep this line.\n\n@/tmp/My File.txt\n@C:\\Users\\Ada\\Project";
        let attachments = attachments_from_agent_text(prompt);
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].path, "/tmp/My File.txt");
        assert_eq!(attachments[0].name, "My File.txt");
        assert_eq!(attachments[1].path, r"C:\Users\Ada\Project");
        assert_eq!(attachments[1].name, "Project");

        assert!(attachments_from_agent_text("@/tmp/in-body\nthen prose").is_empty());
        assert!(attachments_from_agent_text("body\n\n@relative/path").is_empty());
        let only_tail = attachments_from_agent_text("@/tmp/in-body\n\n@/tmp/tail");
        assert_eq!(only_tail.len(), 1);
        assert_eq!(only_tail[0].path, "/tmp/tail");
    }

    #[test]
    fn user_journal_keeps_display_text_and_persists_deduped_attachments() {
        let message = user_journal_message(
            "user-1".into(),
            "visible chip text".into(),
            "agent text\n\n@/tmp/a.png\n@/tmp/a.png\n@/tmp/folder",
            None,
        );
        assert_eq!(message.content, "visible chip text");
        let attachments = message.attachments.unwrap();
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].path, "/tmp/a.png");
        assert_eq!(attachments[1].path, "/tmp/folder");
    }

    #[test]
    fn explicit_attachment_metadata_is_authoritative_and_deduped() {
        let explicit = vec![
            MessageAttachmentStored {
                path: "/tmp/a.png".into(),
                name: "Frontend label".into(),
                is_dir: true,
            },
            MessageAttachmentStored {
                path: "/tmp/a.png".into(),
                name: "Ignored duplicate".into(),
                is_dir: false,
            },
        ];
        let normalized = normalize_explicit_attachments(explicit).unwrap();
        let message = user_journal_message(
            "user-explicit".into(),
            "visible".into(),
            "agent text\n\n@/tmp/fallback.txt",
            Some(normalized),
        );
        let attachments = message.attachments.unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].path, "/tmp/a.png");
        assert_eq!(attachments[0].name, "Frontend label");
        assert!(attachments[0].is_dir);
    }

    #[test]
    fn explicit_attachment_metadata_rejects_relative_paths() {
        let result = normalize_explicit_attachments(vec![MessageAttachmentStored {
            path: "relative/file.txt".into(),
            name: "file.txt".into(),
            is_dir: false,
        }]);
        assert!(result.is_err());
    }

    #[test]
    fn attachment_parser_classifies_existing_directories() {
        let root = std::env::temp_dir().join(format!("sunsetz-attachment-test-{}", Uuid::new_v4()));
        let directory = root.join("Folder");
        let file = root.join("note.txt");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&file, b"test").unwrap();
        let prompt = format!("body\n\n@{}\n@{}", directory.display(), file.display());
        let attachments = attachments_from_agent_text(&prompt);
        assert_eq!(attachments.len(), 2);
        assert!(attachments[0].is_dir);
        assert!(!attachments[1].is_dir);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn tool_step_upsert_preserves_exact_timeline_slot() {
        assert_eq!(
            tool_step_content("in_progress", "read", "Read file", None, Some("/tmp/a.rs"),),
            "tool_step|in_progress|read|Read file\n\n/tmp/a.rs"
        );
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/acp/assistant_tool_timeline.json"
        ))
        .unwrap();
        let mut messages = Vec::new();
        let mut first_tool_created_at = None;
        for event in fixture["events"].as_array().unwrap() {
            match event["type"].as_str().unwrap() {
                "assistant" => messages.push(stored_message(
                    event["id"].as_str().unwrap(),
                    "assistant",
                    event["content"].as_str().unwrap(),
                )),
                "tool" => {
                    upsert_tool_step_message(
                        &mut messages,
                        event["toolCallId"].as_str().unwrap(),
                        event["status"].as_str().unwrap(),
                        event["kind"].as_str().unwrap(),
                        event["title"].as_str().unwrap(),
                        event["detail"].as_str(),
                        event["path"].as_str(),
                    );
                    let tool = messages
                        .iter()
                        .find(|message| message.id == "tool-call-1")
                        .unwrap();
                    if first_tool_created_at.is_none() {
                        first_tool_created_at = Some(tool.created_at);
                    }
                }
                other => panic!("unexpected fixture event: {other}"),
            }
        }
        let expected_ids = fixture["expect"]["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(messages[1].created_at, first_tool_created_at.unwrap());
        assert_eq!(
            messages[1].content,
            fixture["expect"]["toolContent"].as_str().unwrap()
        );
        assert_eq!(messages[1].marker.as_deref(), Some("tool_step"));
    }

    #[test]
    fn context_compact_keeps_phase_order_from_golden_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/acp/context_compact_timeline.json"
        ))
        .unwrap();
        let mut messages = Vec::new();
        for event in fixture["events"].as_array().unwrap() {
            match event["type"].as_str().unwrap() {
                "assistant" => messages.push(stored_message(
                    event["id"].as_str().unwrap(),
                    "assistant",
                    event["content"].as_str().unwrap(),
                )),
                "contextCompact" => {
                    let content = context_compact_content(
                        event["trigger"].as_str().unwrap(),
                        event["tokensBefore"].as_u64(),
                        event["tokensAfter"].as_u64(),
                        event["summaryPreview"].as_str(),
                        event["note"].as_str(),
                    );
                    upsert_context_compact_message(
                        &mut messages,
                        event["id"].as_str().unwrap(),
                        &content,
                    );
                }
                other => panic!("unexpected fixture event: {other}"),
            }
        }

        let expected_ids = fixture["expect"]["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(
            messages[1].content,
            fixture["expect"]["compactContent"].as_str().unwrap()
        );
        assert_eq!(messages[1].marker.as_deref(), Some("context_compact"));

        let compact_created_at = messages[1].created_at;
        upsert_context_compact_message(
            &mut messages,
            "context-compact-1",
            "context_compact|manual",
        );
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].created_at, compact_created_at);
        assert_eq!(messages[1].content, "context_compact|manual");
    }

    #[test]
    fn ask_user_activity_lifecycle_matches_golden_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/acp/ask_user_activity.json"
        ))
        .unwrap();
        let rpc_id = fixture["rpcId"].as_u64().unwrap();
        let tool_call_id = fixture["toolCallId"].as_str().unwrap();
        let question_count = fixture["questionCount"].as_u64().unwrap() as usize;
        let activity_id = ask_user_activity_id(Some(tool_call_id), rpc_id);
        assert_eq!(
            activity_id,
            fixture["expect"]["activityId"].as_str().unwrap()
        );
        assert_eq!(
            ask_user_activity_id(Some("  "), rpc_id),
            fixture["expect"]["fallbackActivityId"].as_str().unwrap()
        );

        let mut messages = vec![stored_message("assistant-before", "assistant", "Before")];
        upsert_ask_user_activity_message(
            &mut messages,
            &activity_id,
            "in_progress",
            question_count,
            None,
        );
        assert_eq!(
            messages[1].content,
            fixture["expect"]["waitingContent"].as_str().unwrap()
        );
        let first_created_at = messages[1].created_at;
        messages.push(stored_message("assistant-after", "assistant", "After"));

        let answered = answered_question_count(&fixture["answers"], question_count);
        assert_eq!(answered, 2);
        upsert_ask_user_activity_message(
            &mut messages,
            &activity_id,
            "completed",
            question_count,
            Some(answered),
        );
        let expected_ids = fixture["expect"]["timelineIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(
            messages[1].id,
            fixture["expect"]["messageId"].as_str().unwrap()
        );
        assert_eq!(messages[1].created_at, first_created_at);
        assert_eq!(
            messages[1].content,
            fixture["expect"]["completedContent"].as_str().unwrap()
        );
        assert_eq!(messages[1].marker.as_deref(), Some("tool_step"));
        assert!(!messages[1].is_error);

        upsert_ask_user_activity_message(
            &mut messages,
            &activity_id,
            "completed",
            question_count,
            Some(answered_question_count(&json!({}), question_count)),
        );
        assert_eq!(
            messages[1].content,
            fixture["expect"]["allSkippedContent"].as_str().unwrap()
        );

        upsert_ask_user_activity_message(
            &mut messages,
            &activity_id,
            "cancelled",
            question_count,
            None,
        );
        assert_eq!(
            messages[1].content,
            fixture["expect"]["cancelledContent"].as_str().unwrap()
        );
        assert!(!messages[1].is_error);

        upsert_ask_user_activity_message(
            &mut messages,
            &activity_id,
            "failed",
            question_count,
            None,
        );
        assert_eq!(
            messages[1].content,
            fixture["expect"]["failedContent"].as_str().unwrap()
        );
        assert!(messages[1].is_error);
    }

    #[test]
    fn ask_user_write_failure_restores_pending_and_keeps_waiting_activity() {
        let mut session = test_live_session("ask-retry");
        session.pending_ask_user = Some(pending_ask("ask-retry", 23));
        let pending = session.pending_ask_user.as_ref().unwrap();
        let mut messages = Vec::new();
        upsert_ask_user_activity_message(
            &mut messages,
            &pending.activity_id,
            "in_progress",
            pending.questions.len(),
            None,
        );
        let waiting_content = messages[0].content.clone();

        claim_pending_ask(session.pending_ask_user.as_mut().unwrap(), None, Some(23)).unwrap();
        assert!(restore_pending_ask_after_failure(
            &mut session,
            "process-ask-retry",
            23,
        ));
        assert!(session.pending_ask_user.is_some());
        assert!(!session.pending_ask_user.as_ref().unwrap().resolving);
        assert_eq!(messages[0].content, waiting_content);
        assert!(messages[0]
            .content
            .starts_with("tool_step|in_progress|ask_user|"));
    }

    #[test]
    fn pending_ask_terminal_metadata_is_stable_and_clears_dead_request() {
        let mut session = test_live_session("dead-ask");
        let questions = vec![
            AskUserQuestionItem {
                id: "q-1".into(),
                question: "One?".into(),
                options: Vec::new(),
                multi_select: false,
            },
            AskUserQuestionItem {
                id: "q-2".into(),
                question: "Two?".into(),
                options: Vec::new(),
                multi_select: false,
            },
        ];
        session.pending_ask_user = Some(PendingAskUser {
            interaction: InteractionSnapshotV1::new(
                "dead-ask",
                "process-dead-ask",
                71,
                None,
                InteractionPayloadV1::AskUser {
                    questions: questions.clone(),
                    partial_answers: None,
                },
            ),
            rpc_id: 71,
            tool_call_id: None,
            activity_id: ask_user_activity_id(None, 71),
            questions,
            partial_answers: None,
            raw: json!({}),
            resolving: false,
            host_reply: None,
        });

        let activity = take_pending_ask_activity(&mut session).unwrap();
        assert_eq!(
            activity,
            PendingAskActivity {
                session_id: "dead-ask".into(),
                activity_id: "ask-user-71".into(),
                question_count: 2,
            }
        );
        assert!(session.pending_ask_user.is_none());
    }

    #[test]
    fn first_tool_starts_one_locked_host_phase_boundary() {
        let mut session = test_live_session("a");
        let old_id = session.streaming_message_id.clone();
        let boundary = SessionManager::begin_tool_boundary(&mut session, "call-1");
        assert_eq!(boundary, Some(old_id));
        let phase_after_tool = session.streaming_message_id.clone();
        assert!(session.stream_phase_id_locked);
        assert_ne!(phase_after_tool, Some("phase-a-0".into()));

        assert_eq!(
            SessionManager::begin_tool_boundary(&mut session, "call-1"),
            None
        );
        assert_eq!(session.streaming_message_id, phase_after_tool);
    }

    #[test]
    fn context_compact_starts_a_fresh_locked_phase() {
        let mut session = test_live_session("compact");
        let completed_phase_id = session.streaming_message_id.clone();
        session.stream_last_was_assistant = true;

        assert_eq!(
            SessionManager::begin_context_compact_boundary(&mut session),
            completed_phase_id
        );
        assert!(session.stream_phase_id_locked);
        assert_ne!(session.streaming_message_id, completed_phase_id);
        assert!(!session.stream_last_was_assistant);

        session.streaming_message_id = None;
        session.stream_phase_id_locked = false;
        assert_eq!(
            SessionManager::begin_context_compact_boundary(&mut session),
            None
        );
        assert!(session.streaming_message_id.is_none());
        assert!(!session.stream_phase_id_locked);
    }

    #[test]
    fn pending_ask_claim_rejects_stale_and_duplicate_resolves() {
        let mut pending = pending_ask("a", 7);
        let stale = claim_pending_ask(&mut pending, None, Some(8)).unwrap_err();
        assert!(stale.contains("expected 7"));
        assert!(!pending.resolving);

        assert_eq!(claim_pending_ask(&mut pending, None, Some(7)).unwrap(), 7);
        assert!(pending.resolving);
        assert!(claim_pending_ask(&mut pending, None, Some(7))
            .unwrap_err()
            .contains("already resolving"));
    }

    #[test]
    fn pending_ask_is_restored_on_failure_and_compare_cleared_on_success() {
        let mut session = test_live_session("a");
        session.pending_ask_user = Some(pending_ask("a", 7));
        {
            let pending = session.pending_ask_user.as_mut().unwrap();
            claim_pending_ask(pending, None, Some(7)).unwrap();
            pending.partial_answers = Some(json!({ "Choose?": "Keep me" }));
        }

        assert!(!clear_pending_ask_after_success(
            &mut session,
            "different-process",
            7
        ));
        assert!(session.pending_ask_user.is_some());
        assert!(restore_pending_ask_after_failure(
            &mut session,
            "process-a",
            7
        ));
        let restored = session.pending_ask_user.as_ref().unwrap();
        assert!(!restored.resolving);
        assert_eq!(
            restored.ui_payload("a").partial_answers,
            Some(json!({ "Choose?": "Keep me" }))
        );

        claim_pending_ask(session.pending_ask_user.as_mut().unwrap(), None, Some(7)).unwrap();
        // A replacement arriving during the write is not the request we claimed.
        session.pending_ask_user = Some(pending_ask("a", 7));
        assert!(!clear_pending_ask_after_success(
            &mut session,
            "process-a",
            7
        ));
        assert!(session.pending_ask_user.is_some());

        claim_pending_ask(session.pending_ask_user.as_mut().unwrap(), None, Some(7)).unwrap();
        assert!(clear_pending_ask_after_success(
            &mut session,
            "process-a",
            7
        ));
        assert!(session.pending_ask_user.is_none());
    }

    #[test]
    fn pending_ask_query_recovers_live_and_background_payloads() {
        let manager = SessionManager::new();
        let mut live = test_live_session("live");
        live.pending_ask_user = Some(pending_ask("live", 7));
        *manager.inner.lock() = Some(live);

        let mut background = test_live_session("background");
        background.pending_ask_user = Some(pending_ask("background", 7));
        manager
            .background
            .lock()
            .insert("background".into(), background);

        let current = manager.pending_ask_user(None).unwrap();
        assert_eq!(current.session_id, "live");
        assert_eq!(current.rpc_id, 7);
        assert_eq!(current.raw["futureField"]["kept"], true);

        let recovered = manager.pending_ask_user(Some("background")).unwrap();
        assert_eq!(recovered.session_id, "background");
        assert_eq!(recovered.rpc_id, 7);
        assert_eq!(recovered.tool_call_id.as_deref(), Some("tool-background"));
        assert!(manager.pending_ask_user(Some("missing")).is_none());

        let mut background_second = test_live_session("aaa-background");
        background_second.pending_ask_user = Some(pending_ask("aaa-background", 11));
        manager
            .background
            .lock()
            .insert("aaa-background".into(), background_second);

        let all = manager.pending_interactions();
        assert_eq!(
            all.iter()
                .map(|request| request.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["aaa-background", "background", "live"]
        );
        assert_eq!(
            all.iter().map(|request| request.rpc_id).collect::<Vec<_>>(),
            vec![11, 7, 7]
        );
    }

    #[test]
    fn deferred_prompt_complete_waits_for_pending_ask() {
        let mut session = test_live_session("background");
        session.pending_ask_user = Some(pending_ask("background", 9));
        session.deferred_prompt_complete = Some("end_turn".into());
        assert!(SessionManager::try_finish_deferred_prompt_complete(&mut session).is_none());
        assert_eq!(session.fsm.state(), SessionState::Streaming);

        session.pending_ask_user = None;
        assert!(SessionManager::try_finish_deferred_prompt_complete(&mut session).is_some());
        assert_eq!(session.fsm.state(), SessionState::Ready);
    }

    #[test]
    fn memory_dispatch_rejection_after_focus_switch_resets_only_origin_session() {
        let manager = SessionManager::new();
        let focused = test_live_session("focused");
        let focused_message_id = focused.streaming_message_id.clone();
        *manager.inner.lock() = Some(focused);

        let mut origin = test_live_session("origin");
        origin.stream_buf = "must be cleared".into();
        manager.background.lock().insert("origin".into(), origin);

        manager.reset_rejected_session("origin");

        {
            let focused = manager.inner.lock();
            let focused = focused.as_ref().unwrap();
            assert_eq!(focused.app_session_id, "focused");
            assert_eq!(focused.fsm.state(), SessionState::Streaming);
            assert_eq!(focused.streaming_message_id, focused_message_id);
        }

        let background = manager.background.lock();
        let origin = background.get("origin").unwrap();
        assert_eq!(origin.fsm.state(), SessionState::Ready);
        assert!(origin.streaming_message_id.is_none());
        assert!(origin.stream_buf.is_empty());
    }

    #[test]
    fn host_permission_resolve_does_not_require_acp() {
        let manager = SessionManager::new();
        let mut session = test_live_session("host-perm");
        let _ = session.fsm.await_permission();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let snapshot = InteractionSnapshotV1::new(
            "host-perm",
            "process-host-perm",
            7,
            Some("tool-1".into()),
            InteractionPayloadV1::Permission {
                tool_name: "write_file".into(),
                title: "Write a.txt".into(),
                preview: "a.txt (4 bytes)".into(),
                scope_key: "write_file:/tmp/a.txt".into(),
                options: SessionManager::host_permission_options(),
            },
        );
        session.pending_permission = Some(PendingPermission {
            interaction: snapshot,
            host_reply: Some(tx),
        });
        *manager.inner.lock() = Some(session);

        let (mut target, resolving) = manager
            .prepare_permission_resolution(Some("host-perm"), Some(7), None)
            .unwrap();
        assert_eq!(resolving.status, InteractionStatusV1::Resolving);
        match target.reply.take() {
            Some(PermissionReplyChannel::Host(tx)) => {
                tx.send(PermissionOutcome::Selected {
                    option_id: "allow_once".into(),
                })
                .unwrap();
            }
            _ => panic!("expected Host reply channel"),
        }
        let (cleared, finished, _, _) = manager.clear_resolved_permission(&target, None);
        assert!(cleared.is_some());
        assert!(!finished);
        let outcome = rx.blocking_recv().unwrap();
        match outcome {
            PermissionOutcome::Selected { option_id } => assert_eq!(option_id, "allow_once"),
            PermissionOutcome::Cancelled => panic!("host resolve cancelled"),
        }
        assert!(manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .pending_permission
            .is_none());
        assert_eq!(
            manager.inner.lock().as_ref().unwrap().fsm.state(),
            SessionState::Streaming
        );
    }

    #[test]
    fn interrupt_host_permission_drops_oneshot_without_execute() {
        let mut session = test_live_session("host-stop");
        let (tx, rx) = tokio::sync::oneshot::channel();
        session.pending_permission = Some(PendingPermission {
            interaction: InteractionSnapshotV1::new(
                "host-stop",
                "process-host-stop",
                1,
                Some("tool-stop".into()),
                InteractionPayloadV1::Permission {
                    tool_name: "run_command".into(),
                    title: "Run echo".into(),
                    preview: "echo (cwd: .)".into(),
                    scope_key: "run_command:echo".into(),
                    options: SessionManager::host_permission_options(),
                },
            ),
            host_reply: Some(tx),
        });
        let interrupted = interrupt_pending_interactions(&mut session);
        assert_eq!(interrupted.interactions.len(), 1);
        assert_eq!(
            interrupted.interactions[0].status,
            InteractionStatusV1::Interrupted
        );
        assert!(session.pending_permission.is_none());
        assert!(rx.blocking_recv().is_err());
    }

    #[test]
    fn install_host_permission_auto_allow_write_under_accept_edits() {
        let manager = SessionManager::new();
        let mut session = test_live_session("host-auto");
        session.policy = PermissionPolicy::AcceptEdits;
        let root = std::env::temp_dir().join("sunsetz-host-auto-write");
        let _ = std::fs::create_dir_all(&root);
        let inside = root.join("a.rs");
        let _ = std::fs::write(&inside, "fn main() {}");
        session.project_path = Some(root.to_string_lossy().into_owned());
        *manager.inner.lock() = Some(session);

        let (_rx, request, _snapshot, auto, auto_deny, _) = manager
            .install_host_permission(
                "host-auto",
                &agent_loop::HostToolPermission {
                    tool_name: "write_file".into(),
                    title: "Write a.rs".into(),
                    preview: "a.rs (12 bytes)".into(),
                    path_target: inside.to_string_lossy().into_owned(),
                    command: String::new(),
                    tool_call_id: "t1".into(),
                },
            )
            .unwrap();
        assert!(auto);
        assert!(!auto_deny);
        assert_eq!(request.tool_name, "write_file");
        assert_eq!(request.preview, "a.rs (12 bytes)");
        assert!(!request.preview.contains("fn main"));

        let already = manager
            .install_host_permission(
                "host-auto",
                &agent_loop::HostToolPermission {
                    tool_name: "run_command".into(),
                    title: "Run echo".into(),
                    preview: "echo hi (cwd: .)".into(),
                    path_target: String::new(),
                    command: "echo hi".into(),
                    tool_call_id: "t2".into(),
                },
            )
            .unwrap_err();
        assert!(already.contains("already pending"), "{already}");

        manager.inner.lock().as_mut().unwrap().pending_permission = None;
        let (_rx, _request, _snapshot, auto_cmd, auto_deny_cmd, _) = manager
            .install_host_permission(
                "host-auto",
                &agent_loop::HostToolPermission {
                    tool_name: "run_command".into(),
                    title: "Run echo".into(),
                    preview: "echo hi (cwd: .)".into(),
                    path_target: String::new(),
                    command: "echo hi".into(),
                    tool_call_id: "t3".into(),
                },
            )
            .unwrap();
        assert!(!auto_cmd, "AcceptEdits must not auto-allow run_command");
        assert!(!auto_deny_cmd);
        let _ = std::fs::remove_dir_all(&root);
    }
}
