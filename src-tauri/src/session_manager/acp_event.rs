//! ACP event processing: routes agent stream events to the focused live
//! session or a background busy session, updates journal/state, and emits
//! UI events. The single largest chunk of session_manager behavior.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::acp_client::{
    should_abort_provider_retry, AcpEvent, StreamKind,
    HOST_PROVIDER_MAX_RETRIES,
};
use crate::error::{AgentError, AgentErrorCode};
use crate::interactions::{InteractionPayloadV1, InteractionSnapshotV1};
use crate::journal_throttle::is_paragraph_break;
use crate::permission::{
    extract_path_target, extract_shell_command, may_auto_allow, may_auto_deny,
    permission_scope_key, pick_option_id,
};
use crate::session_fsm::SessionState;
use crate::store::{self, ChatMessageStored};
use crate::turn_complete::{
    is_successful_prompt_complete, is_terminal_tool_status,
};

use super::types::*;
use super::SessionManager;

impl SessionManager {
    pub(super) async fn handle_acp_event(self: &Arc<Self>, app: &AppHandle, process_id: &str, ev: AcpEvent) {
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
    pub(super) async fn handle_acp_event_on_background(
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
}
