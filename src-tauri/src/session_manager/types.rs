//! Data types and pure helpers for the session manager: snapshots, pending
//! interaction records, live/parked session bookkeeping structs, and journal
//! formatting helpers. No `SessionManager` methods live here.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::{
    AcpClient, AcpEvent,
    AskUserOutcome, AskUserQuestionItem, PermissionOutcome,
};
use crate::agent_loop;
use crate::error::{AgentError, AgentErrorCode};
use crate::interactions::{InteractionPayloadV1, InteractionSnapshotV1, InteractionStatusV1};
use crate::journal_throttle::JournalWriteThrottle;
use crate::mock_acp::MockStreamHandle;
use crate::permission::{
    PermissionPolicy, SessionAllowCache,
};
use crate::session_fsm::{SessionFsm, SessionState};
use crate::store::{self, ChatMessageStored, MessageAttachmentStored, SessionMeta};

pub(super) fn sanitize_error_detail(raw: &str) -> String {
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
pub(super) struct PendingAskUser {
    pub(super) interaction: InteractionSnapshotV1,
    pub(super) rpc_id: u64,
    pub(super) tool_call_id: Option<String>,
    pub(super) activity_id: String,
    pub(super) questions: Vec<AskUserQuestionItem>,
    pub(super) partial_answers: Option<serde_json::Value>,
    pub(super) raw: serde_json::Value,
    /// Prevent two UI retries from writing two replies concurrently.
    pub(super) resolving: bool,
    /// In-process Sunsetz kernel wait. ACP asks leave this empty.
    pub(super) host_reply: Option<tokio::sync::oneshot::Sender<AskUserOutcome>>,
}

impl PendingAskUser {
    pub(super) fn ui_payload(&self, session_id: &str) -> UiAskUserRequest {
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

pub(super) struct PendingPermission {
    pub(super) interaction: InteractionSnapshotV1,
    pub(super) host_reply: Option<tokio::sync::oneshot::Sender<PermissionOutcome>>,
}

impl PendingPermission {
    pub(super) fn ui_payload(&self) -> UiPermissionRequest {
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
pub(super) struct PendingPlan {
    pub(super) interaction: InteractionSnapshotV1,
}

pub(super) fn ask_user_activity_id(tool_call_id: Option<&str>, rpc_id: u64) -> String {
    tool_call_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("ask-user-{rpc_id}"))
}

pub(super) fn ask_user_activity_title(
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

pub(super) fn answered_question_count(answers: &serde_json::Value, question_count: usize) -> usize {
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

pub(super) fn claim_pending_ask(
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

pub(super) fn restore_pending_ask_after_failure(
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

pub(super) fn clear_pending_ask_after_success(
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
pub(super) type ProcessId = String;

pub(super) struct LiveSession {
    pub(super) app_session_id: String,
    /// Stable id for the agent process / event pump (not the App session id).
    pub(super) process_id: ProcessId,
    pub(super) meta: SessionMeta,
    pub(super) fsm: SessionFsm,
    pub(super) backend: String,
    pub(super) acp: Option<Arc<AcpClient>>,
    pub(super) mock_stream: Option<MockStreamHandle>,
    /// Cancels the in-process Sunsetz agent turn.
    pub(super) agent_cancel: Option<Arc<AtomicBool>>,
    /// Id of the current parent kernel turn; subagents spawned here are cancelled with Stop.
    pub(super) host_turn_id: Option<String>,
    pub(super) streaming_message_id: Option<String>,
    /// Accumulated assistant text for current turn (persisted on complete).
    pub(super) stream_buf: String,
    pub(super) stream_thought: String,
    /// Last emitted chunk was assistant body — next thought opens a new phase
    /// so thinking and body can interleave (think → write → think → write).
    pub(super) stream_last_was_assistant: bool,
    /// Host-created phases after a tool boundary keep their UUID even when the
    /// Runtime continues to reuse one messageId for the whole turn.
    pub(super) stream_phase_id_locked: bool,
    /// Image/file paths produced this turn (image_gen / image_edit).
    pub(super) stream_attachments: Vec<MessageAttachmentStored>,
    pub(super) model_id: Option<String>,
    /// Effort applied to the live agent process (from last spawn).
    pub(super) effort: Option<String>,
    /// Product mode: agent | plan | ask (ACP session/set_mode).
    pub(super) product_mode: Option<String>,
    pub(super) project_path: Option<String>,
    pub(super) allow_cache: SessionAllowCache,
    pub(super) policy: PermissionPolicy,
    /// Last provider retry attempt observed this turn (0 = none).
    pub(super) provider_retry_attempt: u32,
    /// Host already aborted this turn after max retries (avoid double cancel).
    pub(super) provider_retry_aborted: bool,
    /// After session/new (load failed), first prompt should carry journal history.
    pub(super) needs_history_bootstrap: bool,
    /// Pending `_x.ai/exit_plan_mode` JSON-RPC id awaiting user Approve / revise.
    pub(super) pending_plan: Option<PendingPlan>,
    /// Pending `session/request_permission` awaiting a Host/UI response.
    pub(super) pending_permission: Option<PendingPermission>,
    /// Synthetic JSON-RPC ids for in-process Host kernel permission waits.
    pub(super) host_rpc_seq: u64,
    /// Pending `_x.ai/ask_user_question` payload awaiting user answers.
    pub(super) pending_ask_user: Option<PendingAskUser>,
    /// Last user/agent activity (send, stream, permission, connect).
    pub(super) last_activity: Instant,
    /// Last stream chunk or tool event (I06 stall watchdog). Permission waits do not update this.
    pub(super) last_stream_progress: Instant,
    /// Last time we emitted `session://stream_stall` for the current silence window.
    pub(super) last_stall_emit: Option<Instant>,
    /// Throttle mid-stream assistant journal upserts (I04).
    pub(super) journal_throttle: JournalWriteThrottle,
    /// Tool calls still pending/in_progress this turn (#52 early prompt_complete).
    pub(super) open_tool_ids: HashSet<String>,
    /// Every tool id already observed this turn. Unlike `open_tool_ids`, ids
    /// remain here after completion so updates never create a second boundary.
    pub(super) seen_tool_ids: HashSet<String>,
    /// `prompt_complete` arrived while tools/gates still open; finish when clear.
    pub(super) deferred_prompt_complete: Option<String>,
    /// Tool events observed during the current turn (empty-run soft signal).
    pub(super) tools_this_turn: u32,
    /// Metadata-only Skill evidence for the active turn. Bodies, prompts and
    /// tool payloads never enter this ledger.
    pub(super) active_skill_uses: Vec<crate::skill_feedback::SkillUseRecordV1>,
    pub(super) pending_skill_settlement: Option<crate::skill_feedback::SkillUseStatusV1>,
    /// Successful parent PromptComplete may auto-start a wake turn.
    /// Stop / Steer and a user `begin_stream` clear this so queued send wins.
    pub(super) allow_auto_wake: bool,
}

pub(super) struct WakeInspect {
    pub(super) backend: String,
    pub(super) streaming: bool,
    pub(super) deferred_prompt_complete: bool,
    pub(super) awaiting_permission: bool,
    pub(super) allow_auto_wake: bool,
    pub(super) host_turn_id: Option<String>,
    pub(super) process_id: String,
    pub(super) model_id: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) project_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingAskActivity {
    pub(super) session_id: String,
    pub(super) activity_id: String,
    pub(super) question_count: usize,
}

pub(super) fn take_pending_ask_activity(session: &mut LiveSession) -> Option<PendingAskActivity> {
    let pending = session.pending_ask_user.take()?;
    Some(PendingAskActivity {
        session_id: session.app_session_id.clone(),
        activity_id: pending.activity_id,
        question_count: pending.questions.len(),
    })
}

pub(super) struct InterruptedSessionGates {
    pub(super) interactions: Vec<InteractionSnapshotV1>,
    pub(super) plan_artifacts: Vec<crate::plan_artifacts::PlanArtifactV1>,
}

impl InterruptedSessionGates {
    pub(super) fn empty() -> Self {
        Self {
            interactions: Vec::new(),
            plan_artifacts: Vec::new(),
        }
    }
}

pub(super) fn interrupt_pending_interactions(session: &mut LiveSession) -> InterruptedSessionGates {
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
pub(super) struct ParkedAgent {
    pub(super) process_id: ProcessId,
    pub(super) app_session_id: String,
    pub(super) meta: SessionMeta,
    pub(super) acp: Arc<AcpClient>,
    pub(super) last_activity: Instant,
    pub(super) model_id: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) product_mode: Option<String>,
    pub(super) project_path: Option<String>,
    pub(super) policy: PermissionPolicy,
    pub(super) needs_history_bootstrap: bool,
    pub(super) backend: String,
}

/// How many journal messages (user+assistant) to carry when session/load fails.
pub(super) const HISTORY_BOOTSTRAP_MAX_MSGS: usize = 16;
/// Cap each message body in the bootstrap block.
pub(super) const HISTORY_BOOTSTRAP_PER_MSG_CHARS: usize = 2_000;
/// Cap total bootstrap text (excluding the new user turn).
pub(super) const HISTORY_BOOTSTRAP_MAX_CHARS: usize = 14_000;

pub(super) fn build_runtime_context_usage(
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

pub(super) enum CompactGate {
    Continue,
    Finished,
    Failed,
}

/// Compact the built-in kernel window before the parent `run_turn`.
pub(super) async fn apply_sunsetz_compact(
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
pub(super) fn build_history_bootstrap(app_session_id: &str) -> Option<String> {
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

pub(super) fn is_agent_directive_line(line: &str) -> bool {
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

pub(super) fn next_prompt_line(prompt: &str, start: usize) -> (&str, usize) {
    let rest = &prompt[start..];
    match rest.find('\n') {
        Some(index) => (&rest[..index], start + index + 1),
        None => (rest, prompt.len()),
    }
}

pub(super) fn leading_agent_directive_end(prompt: &str) -> usize {
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
pub(super) fn prepend_host_context_preserving_directives(prompt: &str, context: &str) -> String {
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
pub(super) const TOOL_CONTENT_SNIPPET_MAX: usize = 200_000;

/// Extract human-visible path + detail from tool_call payload for activity UI.
pub(super) fn extract_tool_ui_fields(raw: &serde_json::Value) -> (Option<String>, Option<String>) {
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

pub(super) fn take_tool_content_str(v: Option<&serde_json::Value>) -> Option<String> {
    let s = v.and_then(|x| x.as_str())?;
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(TOOL_CONTENT_SNIPPET_MAX).collect())
}

/// Optional before/after text for the session diff panel (from rawInput when present).
/// - str_replace / search_replace: old_string → before, new_string → after
/// - write / create_file: contents → after
pub(super) fn extract_tool_content_snippets(raw: &serde_json::Value) -> (Option<String>, Option<String>) {
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
pub(super) fn session_lookup_host_hint(user_text: &str) -> Option<String> {
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

pub(super) fn regex_is_session_uuid(text: &str) -> bool {
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

pub(super) fn is_uuid_str(s: &str) -> bool {
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
pub(super) fn extract_generated_media_path(raw: &serde_json::Value) -> Option<String> {
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

pub(super) fn is_image_fs_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".heic", ".avif",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

pub(super) fn is_video_fs_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".mp4", ".webm", ".mov", ".mkv", ".m4v", ".avi", ".ogv", ".mpeg", ".mpg",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

pub(super) fn is_media_fs_path(path: &str) -> bool {
    is_image_fs_path(path) || is_video_fs_path(path)
}

pub(super) fn attachment_from_path(path: &str) -> MessageAttachmentStored {
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

pub(super) fn is_absolute_attachment_path(path: &str) -> bool {
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

pub(super) fn absolute_attachment_ref(line: &str) -> Option<&str> {
    let path = line.trim().strip_prefix('@')?.trim();
    is_absolute_attachment_path(path).then_some(path)
}

pub(super) fn normalize_explicit_attachments(
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
pub(super) fn attachments_from_agent_text(text: &str) -> Vec<MessageAttachmentStored> {
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

pub(super) fn user_journal_message(
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

pub(super) fn tool_step_content(
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
pub(super) fn upsert_tool_step_message(
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

pub(super) fn context_compact_content(
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

pub(super) fn upsert_context_compact_message(
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

pub(super) fn persist_context_compact(session_id: &str, message_id: &str, content: &str) {
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

pub(super) fn upsert_ask_user_activity_message(
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

pub(super) fn record_ask_user_activity(
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

pub(super) fn memory_injection_mutation_request(
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

pub(super) fn memory_injection_failed_request(
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

pub(super) fn memory_injection_marker_message(
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

pub(super) fn skill_use_transition_requests(
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

pub(super) fn transition_skill_use_records(
    records: &[crate::skill_feedback::SkillUseRecordV1],
    next_status: crate::skill_feedback::SkillUseStatusV1,
) -> Result<Vec<crate::skill_feedback::SkillUseRecordV1>, String> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    crate::skill_feedback::transition_uses_v1(skill_use_transition_requests(records, next_status))
}

pub(super) fn rollback_unwritten_user_turn(session_id: &str, turn_id: &str) -> Result<(), String> {
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

pub(super) fn record_runtime_write_rejected_marker(
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

pub(super) fn settle_active_skill_uses(
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

pub(super) fn reset_rejected_turn(session: &mut LiveSession) {
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::test_support::{pending_ask, stored_message, test_live_session};
    use serde_json::json;

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
}
