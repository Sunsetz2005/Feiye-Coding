//! Sending and waking: user-send preparation, rewind, the Sunsetz turn
//! runner, subagent/command-job completion hooks, and wake-turn orchestration.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::{
    AcpClient, AcpEvent, TransportWriteAck,
};
use crate::agent_loop;
use crate::journal_throttle::is_paragraph_break;
use crate::mock_acp::{self, StreamChunk};
use crate::session_fsm::SessionState;
use crate::store::{self, MessageAttachmentStored};

use super::types::*;
use super::{command_jobs, subagents, SessionManager, SubagentView};

impl SessionManager {

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

    pub(super) fn rewind_points_from_journal(app_sid: &str) -> Vec<RewindPointDto> {
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

    pub(super) fn prepare_user_send_on(
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

    pub(super) fn prepare_user_send(
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

    pub(super) async fn send_message_inner(
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

    pub(super) async fn send_sunsetz_turn(
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
        let command_wakes = self
            .command_jobs
            .lock()
            .await
            .take_pending_wakes(&app_sid);
        let mut wake_context = String::new();
        if !wakes.is_empty() {
            wake_context.push_str(&subagents::wake_prompt(&wakes));
        }
        if !command_wakes.is_empty() {
            if !wake_context.is_empty() {
                wake_context.push('\n');
            }
            wake_context.push_str(&command_jobs::wake_prompt(&command_wakes));
        }
        if !wake_context.is_empty() {
            agent_prompt =
                prepend_host_context_preserving_directives(&agent_prompt, &wake_context);
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
            stream_idle: crate::stream_stall::stall_duration(Self::stream_stall_seconds_from_settings()),
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
            command_jobs: agent_loop::CommandJobHooks::default(),
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
            command_jobs: agent_loop::CommandJobHooks::default(),
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
        let commands = Arc::clone(&self.command_jobs);
        let commands_out = Arc::clone(&self.command_jobs);
        let commands_wait = Arc::clone(&self.command_jobs);
        let commands_kill = Arc::clone(&self.command_jobs);
        let command_session = app_sid.clone();
        let command_turn = host_turn_id.clone();
        let on_command_finished: Option<command_jobs::CommandFinishedFn> = {
            let mgr = Arc::clone(self);
            let app_fin = app.clone();
            Some(Arc::new(move |session, tool_id, status, title, job_id| {
                let mgr = Arc::clone(&mgr);
                let app_fin = app_fin.clone();
                Box::pin(async move {
                    mgr.on_command_job_finished(app_fin, session, tool_id, status, title, job_id)
                        .await;
                })
            }))
        };
        let on_command_event: Option<command_jobs::CommandJobEventFn> = {
            let app_evt = app.clone();
            Some(Arc::new(move |payload| {
                let _ = app_evt.emit("session://command_job_v1", &payload);
            }))
        };
        cfg.command_jobs = agent_loop::CommandJobHooks {
            start: Some(Arc::new(move |request| {
                let registry = Arc::clone(&commands);
                let session = command_session.clone();
                let turn = command_turn.clone();
                let on_finished = on_command_finished.clone();
                let on_event = on_command_event.clone();
                Box::pin(async move {
                    command_jobs::start_with_registry(
                        registry,
                        session,
                        turn,
                        command_jobs::StartCommandJobRequest {
                            command: request.command,
                            cwd: request.cwd,
                            project_root: request.project_root,
                            sandbox: request.sandbox,
                            tool_call_id: request.tool_call_id,
                            title: request.title,
                        },
                        on_finished,
                        on_event,
                    )
                    .await
                })
            })),
            output: Some(Arc::new(move |id| {
                let registry = Arc::clone(&commands_out);
                Box::pin(async move { command_jobs::output_with_optional_wait(registry, id, None).await })
            })),
            wait: Some(Arc::new(move |ids, wait_any, timeout_ms| {
                let registry = Arc::clone(&commands_wait);
                Box::pin(async move {
                    command_jobs::wait_for_jobs(
                        registry,
                        ids,
                        wait_any,
                        std::time::Duration::from_millis(timeout_ms.max(1)),
                    )
                    .await
                })
            })),
            kill: Some(Arc::new(move |id| {
                let registry = Arc::clone(&commands_kill);
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

    pub(super) async fn on_subagent_finished(
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

    pub(super) async fn on_command_job_finished(
        self: &Arc<Self>,
        app: AppHandle,
        session_id: String,
        tool_call_id: String,
        status: String,
        title: String,
        job_id: String,
    ) {
        let tool_status = if status == "cancelled" {
            "cancelled"
        } else if status == "failed" {
            "failed"
        } else {
            "completed"
        };
        persist_tool_step(
            &session_id,
            &tool_call_id,
            tool_status,
            "execute",
            &title,
            Some(job_id.as_str()),
            None,
        );
        if let Some(process_id) = self.process_id_for_session(&session_id) {
            self.handle_acp_event(
                &app,
                &process_id,
                AcpEvent::ToolCall {
                    tool_call_id,
                    title,
                    kind: "execute".into(),
                    status: tool_status.into(),
                    raw: serde_json::json!({
                        "rawInput": {
                            "id": job_id,
                            "background": true,
                        }
                    }),
                },
            )
            .await;
        }
        let _ = self.maybe_start_wake_turn(&app, &session_id).await;
    }

    /// Read-only hosted-job snapshot for one session (`session_command_jobs_list_v1`).
    /// Used by the UI to restore the composer's hosted-count pill after a reconnect,
    /// without waiting for the next `session://command_job_v1` event.
    pub async fn command_jobs_list(
        &self,
        session_id: &str,
    ) -> Vec<command_jobs::CommandJobSummaryV1> {
        self.command_jobs.lock().await.list_for_session(session_id)
    }

    pub(super) fn process_id_for_session(&self, session_id: &str) -> Option<String> {
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

    pub(super) fn inspect_session_for_wake(&self, session_id: &str) -> Option<WakeInspect> {
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

    pub(super) fn prepare_session_for_wake(&self, session_id: &str) -> bool {
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

    pub(super) async fn maybe_start_wake_turn(self: &Arc<Self>, app: &AppHandle, session_id: &str) -> bool {
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

    pub(super) async fn start_wake_turn_inner(self: &Arc<Self>, app: &AppHandle, session_id: &str) -> bool {
        let Some(inspect) = self.inspect_session_for_wake(session_id) else {
            return false;
        };
        if !agent_loop::is_sunsetz_backend(&inspect.backend) {
            return false;
        }
        let (pending, this_turn_running) = {
            let registry = self.subagents.lock().await;
            let commands = self.command_jobs.lock().await;
            let pending = registry.pending_wake_count(session_id)
                + commands.pending_wake_count(session_id);
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

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::test_support::{ready_kernel_session, test_live_session, IsolatedHome};

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
}
