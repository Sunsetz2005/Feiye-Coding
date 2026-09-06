//! Permission / ask-user / plan interaction request and resolution: the Host
//! side of ACP `session/request_permission`, `_x.ai/ask_user_question`, and
//! `_x.ai/exit_plan_mode`, plus the v1 interactions query/resolve surface.

use std::collections::HashSet;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::acp_client::{
    parse_ask_user_question_params,
    AskUserOutcome, PermissionOutcome,
};
use crate::agent_loop;
use crate::interactions::{InteractionPayloadV1, InteractionSnapshotV1, InteractionStatusV1};
use crate::permission::{
    may_auto_allow, may_auto_deny,
    permission_scope_key, pick_option_id,
};
use crate::session_fsm::SessionState;

use super::types::*;
use super::{
    AskUserReplyChannel, AskUserResolveTarget, PermissionReplyChannel, PermissionResolveTarget,
    PlanResolveTarget, SessionManager,
};

impl SessionManager {
    pub(super) fn host_permission_options() -> serde_json::Value {
        serde_json::json!([
            { "optionId": "allow_once", "kind": "allow_once", "name": "Allow once" },
            { "optionId": "allow_always", "kind": "allow_always", "name": "Allow for session" },
            { "optionId": "reject_once", "kind": "reject_once", "name": "Reject" }
        ])
    }

    pub(super) fn host_decision_from_outcome(
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

    pub(super) fn install_host_permission(
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

    pub(super) async fn request_host_tool_permission(
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

    pub(super) fn install_host_ask_user(
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

    pub(super) async fn request_host_ask_user(
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

    pub(super) fn prepare_permission_resolution(
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

    pub(super) fn restore_permission_after_write_failure(
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

    pub(super) fn clear_resolved_permission(
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

    pub(super) fn active_interaction_snapshots(&self) -> Vec<InteractionSnapshotV1> {
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

    pub(super) fn active_interaction(
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

    pub(super) fn prepare_ask_user_resolution(
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

    pub(super) fn restore_ask_user_after_write_failure(
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
    pub(super) fn clear_resolved_ask_user(
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

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::PermissionPolicy;
    use crate::session_manager::test_support::{pending_ask, test_live_session};
    use serde_json::json;

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
