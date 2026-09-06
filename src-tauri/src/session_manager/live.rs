//! Session lifecycle bookkeeping: idle/stall watchdogs, activity/stream
//! progress tracking, park/unpark capacity management, snapshotting, event
//! emission, and the small live "control" surface (policy, sandbox, model,
//! product mode, effort).

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::StreamKind;
use crate::agent_loop;
use crate::error::{AgentError, AgentErrorCode};
use crate::interactions::InteractionSnapshotV1;
use crate::journal_throttle::JournalWriteThrottle;
use crate::permission::{
    PermissionPolicy, SessionAllowCache,
};
use crate::process_limits::{
    is_idle_expired, normalize_idle_minutes, normalize_max_concurrent,
    process_limit_message,
};
use crate::session_fsm::{SessionFsm, SessionState};
use crate::store::{self, ChatMessageStored};
use crate::stream_stall::{
    normalize_stream_stall_seconds, should_emit_stall, stream_stall_message,
};
use crate::turn_complete::{
    is_successful_prompt_complete, should_defer_prompt_complete,
};

use super::types::*;
use super::SessionManager;

impl SessionManager {
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

    pub(super) fn touch_activity_locked(s: &mut LiveSession) {
        s.last_activity = Instant::now();
    }

    pub(super) fn settle_automation_before_host_kill(session_id: &str, reason: &str) {
        if let Err(error) =
            crate::automation_scheduler::complete_for_session(session_id, false, Some(reason))
        {
            tracing::warn!(
                session_id,
                "settle automation before intentional Runtime kill: {error}"
            );
        }
    }

    pub(super) fn reset_rejected_session(&self, session_id: &str) {
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

    pub(super) fn update_active_skill_uses_for_session(
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

    pub(super) fn defer_skill_settlement_for_session(
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

    pub(super) fn fail_prompt_for_session(
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
    pub(super) fn empty_run_signal_from_live(
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
    pub(super) fn try_finish_deferred_prompt_complete(
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
    pub(super) fn emit_empty_run_if_any(app: &AppHandle, empty: Option<(String, String, String)>) {
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

    pub(super) fn touch_stream_progress_locked(s: &mut LiveSession) {
        let now = Instant::now();
        s.last_activity = now;
        s.last_stream_progress = now;
        s.last_stall_emit = None;
    }

    pub(super) fn ensure_stream_message_id(
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
    pub(super) fn begin_activity_boundary(s: &mut LiveSession) -> Option<String> {
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

    pub(super) fn begin_tool_boundary(s: &mut LiveSession, tool_call_id: &str) -> Option<Option<String>> {
        if tool_call_id.is_empty() || !s.seen_tool_ids.insert(tool_call_id.to_string()) {
            return None;
        }
        Some(Self::begin_activity_boundary(s))
    }

    pub(super) fn begin_context_compact_boundary(s: &mut LiveSession) -> Option<String> {
        let has_active_phase = s.streaming_message_id.is_some()
            || !s.stream_buf.is_empty()
            || !s.stream_thought.is_empty()
            || !s.stream_attachments.is_empty();
        if !has_active_phase {
            return None;
        }
        Self::begin_activity_boundary(s)
    }

    pub(super) fn stream_stall_seconds_from_settings() -> u32 {
        normalize_stream_stall_seconds(store::load_settings().stream_stall_seconds)
    }

    pub(super) fn emit_stream_stall(app: &AppHandle, session_id: &str, stall_seconds: u32) {
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
    pub(super) fn maybe_flush_stream_journal(s: &mut LiveSession, force: bool, paragraph_break: bool) {
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
    pub(super) fn tick_stream_stall(&self, app: &AppHandle) {
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
    pub(super) fn active_process_count(&self) -> u32 {
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

    pub(super) fn max_concurrent_from_settings() -> u32 {
        normalize_max_concurrent(store::load_settings().max_concurrent_agents)
    }

    pub(super) fn idle_minutes_from_settings() -> u32 {
        normalize_idle_minutes(store::load_settings().agent_idle_minutes)
    }

    pub(super) fn emit_idle_recycled(app: &AppHandle, session_id: &str, reason: &str) {
        let _ = app.emit(
            "session://idle_recycled",
            serde_json::json!({
                "sessionId": session_id,
                "reason": reason,
            }),
        );
    }

    pub(super) fn emit_process_limit(app: &AppHandle, session_id: Option<&str>, max: u32) {
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
    pub(super) fn sweep_dead_parked(&self) -> usize {
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
    pub(super) fn try_park_live(&self) -> Result<(), AgentError> {
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
    pub(super) fn promote_background_ready_to_parked(&self, app_session_id: &str) {
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
    pub(super) fn unpark_to_live(&self, app_session_id: &str) -> Option<LiveSession> {
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
    pub(super) async fn free_parked_for_capacity(&self, app: &AppHandle, need_slots: u32) {
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
    pub(super) async fn tick_idle_recycle(&self, app: &AppHandle) {
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

    pub(super) fn backend_name() -> String {
        agent_loop::current_backend()
    }

    pub(super) fn session_is_busy(session: &LiveSession) -> bool {
        matches!(
            session.fsm.state(),
            SessionState::Connecting | SessionState::Streaming | SessionState::AwaitingPermission
        )
    }

    pub(super) fn busy_session_ids(&self) -> Vec<String> {
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

    pub(super) fn emit_state(app: &AppHandle, snap: &SessionSnapshot) {
        let _ = app.emit("session://state", snap);
    }

    pub(super) fn publish_interaction(app: &AppHandle, snapshot: &InteractionSnapshotV1) {
        if let Err(error) = crate::interactions::record(snapshot) {
            tracing::warn!(
                session = %snapshot.session_id,
                interaction = %snapshot.interaction_id,
                "persist interaction snapshot: {error}"
            );
        }
        let _ = app.emit("session://interaction", snapshot);
    }

    pub(super) fn emit_plan_compat(
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

    pub(super) fn emit_plan_artifact(app: &AppHandle, artifact: &crate::plan_artifacts::PlanArtifactV1) {
        let _ = app.emit("session://plan_artifact", artifact);
    }

    pub(super) fn emit_plan_artifact_pair(
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

    pub(super) fn publish_interrupted_session_gates(app: &AppHandle, interrupted: InterruptedSessionGates) {
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
    pub(super) fn record_turn_error(s: &mut LiveSession, app: &AppHandle, err: &AgentError) {
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

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::test_support::{pending_ask, ready_kernel_session, test_live_session};

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
}
