//! Shared test-only helpers used across `session_manager` submodule test
//! suites (connect, live, turn, interactions). Not compiled outside `cfg(test)`.
#![cfg(test)]

use std::collections::HashSet;
use std::time::Instant;

use serde_json::json;
use uuid::Uuid;

use crate::acp_client::AskUserQuestionItem;
use crate::agent_loop;
use crate::interactions::{InteractionPayloadV1, InteractionSnapshotV1};
use crate::journal_throttle::JournalWriteThrottle;
use crate::permission::{PermissionPolicy, SessionAllowCache};
use crate::session_fsm::SessionFsm;
use crate::store::{ChatMessageStored, SessionMeta};

use super::types::{LiveSession, PendingAskUser};

pub(super) fn stored_message(id: &str, role: &str, content: &str) -> ChatMessageStored {
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

pub(super) fn test_live_session(session_id: &str) -> LiveSession {
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

pub(super) struct IsolatedHome {
    home: std::path::PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl IsolatedHome {
    pub(super) fn new(label: &str) -> Self {
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

pub(super) fn ready_kernel_session(session_id: &str) -> LiveSession {
    let mut live = test_live_session(session_id);
    live.fsm.end_stream().unwrap();
    live.streaming_message_id = None;
    live.backend = agent_loop::BACKEND_SUNSETZ.into();
    live
}

pub(super) fn pending_ask(session_id: &str, rpc_id: u64) -> PendingAskUser {
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
