//! Connection lifecycle: connect / reattach / disconnect for the Sunsetz
//! kernel, legacy ACP backends, and the mock backend, plus background-kernel
//! install and automation claim helpers.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::AppHandle;
use uuid::Uuid;

use crate::acp_client::AcpClient;
use crate::agent_loop;
use crate::cli_probe;
use crate::error::{AgentError, AgentErrorCode};
use crate::journal_throttle::JournalWriteThrottle;
use crate::mock_acp::MockConnectMode;
use crate::permission::{
    PermissionPolicy, SessionAllowCache,
};
use crate::process_limits::{
    can_spawn_process, normalize_max_concurrent,
    process_limit_message,
};
use crate::session_fsm::{SessionFsm, SessionState};
use crate::store::{self, SessionMeta};

use super::types::*;
use super::SessionManager;

impl SessionManager {
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

    pub(super) fn kernel_live_session(
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

    pub(super) fn install_background_kernel_session(
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

    pub(super) async fn connect_inner(
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
    pub(super) fn take_reusable_acp(
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

    pub(super) async fn connect_sunsetz(
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

    pub(super) async fn connect_mock(
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


    pub(super) async fn disconnect_inner(&self, app: &AppHandle) {
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
    use crate::session_manager::test_support::{ready_kernel_session, IsolatedHome};

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
}
