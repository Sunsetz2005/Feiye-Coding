//! In-process subagents for the Sunsetz kernel.
//!
//! Children reuse `agent_loop::run_turn` with a filtered tool surface. They do
//! not spawn grok, do not nest, and do not create worktrees.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::acp_client::{AcpEvent, StreamKind};
use crate::agent_loop::{
    self, AgentKind, AgentTurnConfig, SpawnAgentRequest, MAX_RUNNING_SUBAGENTS,
    SUBAGENT_SUMMARY_CHARS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SubagentStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SubagentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentView {
    pub id: String,
    pub parent_session_id: String,
    pub description: String,
    pub agent_type: String,
    pub status: SubagentStatus,
    pub background: bool,
    pub summary: String,
    pub transcript: Vec<TranscriptLine>,
}

#[derive(Clone)]
pub struct SubagentRecord {
    pub id: String,
    pub parent_session_id: String,
    pub parent_turn_id: String,
    pub description: String,
    pub kind: AgentKind,
    pub background: bool,
    pub status: SubagentStatus,
    pub stop: Arc<AtomicBool>,
    pub transcript: Vec<TranscriptLine>,
    pub summary: String,
    pub wake_pending: bool,
}

/// Host callback after a **background** child reaches a terminal status.
pub type SubagentFinishedFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            String,
            String,
            String,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Cap for the model-facing wake prompt (not UI copy).
pub const WAKE_PROMPT_CHARS: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoWakeDecision {
    /// No finished background child is waiting for the parent.
    None,
    /// Parent model is still in `run_turn`; do not interrupt it.
    DeferWhileParentRuns,
    /// This-turn background children (or a permission gate) still block join.
    WaitForThisTurn,
    /// Stop/Steer already ended the UI turn; the next user send consumes summaries.
    HoldForUserSend,
    /// Host should start a parent wake turn now.
    Start,
}

/// Join/wake only this-turn children. Leftover background agents from earlier
/// turns never delay PromptComplete and never ride along with Steer.
pub fn decide_auto_wake(
    pending_wakes: usize,
    this_turn_running: usize,
    parent_streaming: bool,
    deferred_prompt_complete: bool,
    awaiting_permission: bool,
    allow_auto_wake: bool,
) -> AutoWakeDecision {
    if pending_wakes == 0 {
        return AutoWakeDecision::None;
    }
    if this_turn_running > 0 || awaiting_permission {
        return AutoWakeDecision::WaitForThisTurn;
    }
    if parent_streaming && !deferred_prompt_complete {
        return AutoWakeDecision::DeferWhileParentRuns;
    }
    if !allow_auto_wake {
        return AutoWakeDecision::HoldForUserSend;
    }
    AutoWakeDecision::Start
}

pub fn wake_prompt(views: &[SubagentView]) -> String {
    let mut body = String::from("Background subagent results:\n");
    let reserve = 160usize;
    let budget = WAKE_PROMPT_CHARS.saturating_sub(reserve);
    for view in views {
        let summary = view.summary.trim();
        let line = format!(
            "- [{}] {} ({}, id={}): {}\n",
            view.status.as_str(),
            view.description,
            view.agent_type,
            view.id,
            summary
        );
        if body.chars().count() + line.chars().count() > budget {
            body.push_str("- … additional subagent results truncated\n");
            break;
        }
        body.push_str(&line);
    }
    body.push_str(
        "Continue the parent task using these results. Do not claim you are still waiting for them.",
    );
    body
}

impl SubagentRecord {
    fn view(&self) -> SubagentView {
        SubagentView {
            id: self.id.clone(),
            parent_session_id: self.parent_session_id.clone(),
            description: self.description.clone(),
            agent_type: self.kind.as_str().to_string(),
            status: self.status,
            background: self.background,
            summary: self.summary.clone(),
            transcript: self.transcript.clone(),
        }
    }
}

#[derive(Default)]
pub struct SubagentRegistry {
    agents: HashMap<String, SubagentRecord>,
    slot: Arc<Notify>,
}

impl SubagentRegistry {
    pub fn get(&self, id: &str) -> Option<SubagentView> {
        self.agents.get(id).map(SubagentRecord::view)
    }

    pub fn parent_turn_id(&self, id: &str) -> Option<String> {
        self.agents
            .get(id)
            .map(|agent| agent.parent_turn_id.clone())
    }

    pub fn running_count(&self) -> usize {
        self.agents
            .values()
            .filter(|agent| matches!(agent.status, SubagentStatus::Running))
            .count()
    }

    pub fn insert(&mut self, record: SubagentRecord) {
        self.agents.insert(record.id.clone(), record);
    }

    pub fn running_for_turn(&self, session_id: &str, turn_id: &str) -> Vec<String> {
        self.agents
            .values()
            .filter(|agent| {
                agent.parent_session_id == session_id
                    && agent.parent_turn_id == turn_id
                    && matches!(
                        agent.status,
                        SubagentStatus::Running | SubagentStatus::Queued
                    )
            })
            .map(|agent| agent.id.clone())
            .collect()
    }

    pub fn cancel_turn(&mut self, session_id: &str, turn_id: &str) -> usize {
        let mut n = 0;
        for agent in self.agents.values_mut() {
            if agent.parent_session_id == session_id
                && agent.parent_turn_id == turn_id
                && matches!(
                    agent.status,
                    SubagentStatus::Running | SubagentStatus::Queued
                )
            {
                agent.stop.store(true, Ordering::SeqCst);
                agent.status = SubagentStatus::Cancelled;
                agent.summary = "subagent cancelled".into();
                n += 1;
            }
        }
        if n > 0 {
            self.slot.notify_waiters();
        }
        n
    }

    pub fn kill(&mut self, id: &str) -> String {
        let Some(agent) = self.agents.get_mut(id) else {
            return format!("unknown agent `{id}`");
        };
        if matches!(
            agent.status,
            SubagentStatus::Completed | SubagentStatus::Failed | SubagentStatus::Cancelled
        ) {
            return json!({
                "id": id,
                "status": agent.status.as_str(),
                "summary": agent.summary,
            })
            .to_string();
        }
        agent.stop.store(true, Ordering::SeqCst);
        agent.status = SubagentStatus::Cancelled;
        agent.summary = "subagent cancelled".into();
        self.slot.notify_waiters();
        json!({
            "id": id,
            "status": "cancelled",
            "summary": agent.summary,
        })
        .to_string()
    }

    pub fn output(&self, id: &str) -> String {
        let Some(agent) = self.agents.get(id) else {
            return format!("unknown agent `{id}`");
        };
        json!({
            "id": id,
            "status": agent.status.as_str(),
            "agentType": agent.kind.as_str(),
            "description": agent.description,
            "summary": agent.summary,
        })
        .to_string()
    }

    pub fn pending_wake_count(&self, session_id: &str) -> usize {
        self.agents
            .values()
            .filter(|agent| agent.parent_session_id == session_id && agent.wake_pending)
            .count()
    }

    pub fn take_pending_wakes(&mut self, session_id: &str) -> Vec<SubagentView> {
        let mut out = Vec::new();
        for agent in self.agents.values_mut() {
            if agent.parent_session_id == session_id && agent.wake_pending {
                agent.wake_pending = false;
                out.push(agent.view());
            }
        }
        out
    }

    pub fn restore_pending_wakes(&mut self, views: &[SubagentView]) {
        for view in views {
            if let Some(agent) = self.agents.get_mut(&view.id) {
                if matches!(
                    agent.status,
                    SubagentStatus::Completed | SubagentStatus::Failed
                ) {
                    agent.wake_pending = true;
                }
            }
        }
    }
}

pub struct ChildTurnResult {
    pub summary: String,
    pub transcript: Vec<TranscriptLine>,
    pub failed: bool,
    pub cancelled: bool,
}

pub async fn run_child_turn(cfg: AgentTurnConfig, stop: Arc<AtomicBool>) -> ChildTurnResult {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let collector = tokio::spawn(async move {
        let mut transcript = Vec::new();
        let mut summary_parts = Vec::new();
        let mut failed = false;
        while let Some(event) = rx.recv().await {
            match event {
                AcpEvent::Stream {
                    kind,
                    text,
                    done: _,
                    ..
                } if !text.is_empty() => {
                    let line_kind = match kind {
                        StreamKind::Thought => "thought",
                        StreamKind::Assistant => "assistant",
                    };
                    if kind == StreamKind::Assistant {
                        summary_parts.push(text.clone());
                    }
                    push_transcript(&mut transcript, line_kind, &text);
                }
                AcpEvent::ToolCall {
                    title,
                    kind,
                    status,
                    ..
                } => {
                    push_transcript(&mut transcript, "tool", &format!("{status} {kind} {title}"));
                }
                AcpEvent::Error { error } => {
                    failed = true;
                    push_transcript(&mut transcript, "error", &error.message);
                }
                _ => {}
            }
        }
        (transcript, summary_parts, failed)
    });
    agent_loop::run_turn(cfg, move |event| {
        let _ = tx.send(event);
    })
    .await;
    let (transcript, summary_parts, failed) = collector
        .await
        .unwrap_or_else(|_| (Vec::new(), Vec::new(), true));
    let cancelled = stop.load(Ordering::SeqCst);
    let summary = if cancelled {
        "subagent cancelled".into()
    } else {
        cap_summary(&summary_parts)
    };
    ChildTurnResult {
        summary,
        transcript,
        failed: failed && !cancelled,
        cancelled,
    }
}

fn push_transcript(lines: &mut Vec<TranscriptLine>, kind: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let clipped: String = text.chars().take(2_000).collect();
    if lines.len() >= 200 {
        return;
    }
    lines.push(TranscriptLine {
        kind: kind.into(),
        text: clipped,
    });
}

fn cap_summary(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        let piece = part.trim();
        if piece.is_empty() {
            continue;
        }
        if out.chars().count() >= SUBAGENT_SUMMARY_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(piece);
    }
    if out.is_empty() {
        return "(no summary)".into();
    }
    if out.chars().count() > SUBAGENT_SUMMARY_CHARS {
        out = out.chars().take(SUBAGENT_SUMMARY_CHARS).collect();
        out.push_str("\n… truncated");
    }
    out
}

pub fn spawn_result_json(
    id: &str,
    status: SubagentStatus,
    kind: AgentKind,
    description: &str,
    summary: &str,
) -> String {
    json!({
        "id": id,
        "status": status.as_str(),
        "agentType": kind.as_str(),
        "description": description,
        "summary": summary,
    })
    .to_string()
}

pub fn child_config(
    parent: &AgentTurnConfig,
    kind: AgentKind,
    prompt: String,
    stop: Arc<AtomicBool>,
) -> AgentTurnConfig {
    let connectors = if kind.allows_connectors() {
        parent.connectors.clone()
    } else {
        agent_loop::ConnectorTurn::default()
    };
    AgentTurnConfig {
        endpoint: parent.endpoint.clone(),
        project_root: parent.project_root.clone(),
        trusted: parent.trusted,
        history: Vec::new(),
        user_prompt: prompt,
        stop,
        client: parent.client.clone(),
        max_tool_rounds: kind.max_tool_rounds(),
        stream_idle: parent.stream_idle,
        permission_gate: parent.permission_gate.clone(),
        ask_user_gate: None,
        connectors,
        reasoning_effort: parent.reasoning_effort.clone(),
        kind,
        spawn_depth: 1,
        subagents: agent_loop::SubagentHooks::default(),
        command_jobs: agent_loop::CommandJobHooks::default(),
        sandbox_profile: parent.sandbox_profile,
        skill_prompt_chars: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        allow_schedule_task: false,
        allow_skill_save: false,
    }
}

async fn wait_for_slot(registry: &Arc<tokio::sync::Mutex<SubagentRegistry>>, stop: &AtomicBool) {
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        {
            let guard = registry.lock().await;
            if guard.running_count() < MAX_RUNNING_SUBAGENTS {
                return;
            }
            let notify = Arc::clone(&guard.slot);
            drop(guard);
            tokio::select! {
                _ = notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    }
}

/// Host-side spawn used by the parent `run_turn` hook.
pub async fn spawn_with_registry(
    registry: Arc<tokio::sync::Mutex<SubagentRegistry>>,
    parent_cfg: AgentTurnConfig,
    parent_session_id: String,
    parent_turn_id: String,
    request: SpawnAgentRequest,
    on_lifecycle: Arc<
        dyn Fn(String, String, String, String, Option<String>, Option<String>) + Send + Sync,
    >,
    on_finished: Option<SubagentFinishedFn>,
) -> String {
    let kind = match AgentKind::parse_spawn_type(&request.agent_type) {
        Ok(kind) => kind,
        Err(error) => return error,
    };
    let id = Uuid::new_v4().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let record = SubagentRecord {
        id: id.clone(),
        parent_session_id: parent_session_id.clone(),
        parent_turn_id: parent_turn_id.clone(),
        description: request.description.clone(),
        kind,
        background: request.background,
        status: SubagentStatus::Queued,
        stop: Arc::clone(&stop),
        transcript: Vec::new(),
        summary: String::new(),
        wake_pending: false,
    };
    registry.lock().await.insert(record);
    let lifecycle_id = if request.tool_call_id.is_empty() {
        id.clone()
    } else {
        request.tool_call_id.clone()
    };
    on_lifecycle(
        parent_session_id.clone(),
        lifecycle_id.clone(),
        "in_progress".into(),
        request.description.clone(),
        Some(kind.as_str().into()),
        Some(id.clone()),
    );

    let child_cfg = child_config(&parent_cfg, kind, request.prompt, Arc::clone(&stop));
    let run_id = id.clone();
    let background = request.background;
    let registry_run = Arc::clone(&registry);
    let stop_run = Arc::clone(&stop);
    let run = async move {
        wait_for_slot(&registry_run, &stop_run).await;
        if stop_run.load(Ordering::SeqCst) {
            return ChildTurnResult {
                summary: "subagent cancelled".into(),
                transcript: Vec::new(),
                failed: false,
                cancelled: true,
            };
        }
        {
            let mut guard = registry_run.lock().await;
            if let Some(agent) = guard.agents.get_mut(&run_id) {
                agent.status = SubagentStatus::Running;
            }
        }
        let result = run_child_turn(child_cfg, Arc::clone(&stop_run)).await;
        {
            let mut guard = registry_run.lock().await;
            if let Some(agent) = guard.agents.get_mut(&run_id) {
                agent.transcript = result.transcript.clone();
                agent.summary = result.summary.clone();
                agent.status = if result.cancelled {
                    SubagentStatus::Cancelled
                } else if result.failed {
                    SubagentStatus::Failed
                } else {
                    SubagentStatus::Completed
                };
                agent.wake_pending = background && !result.cancelled;
            }
            guard.slot.notify_waiters();
        }
        result
    };

    if request.background {
        let on_life = Arc::clone(&on_lifecycle);
        let life_id = lifecycle_id.clone();
        let desc = request.description.clone();
        let session = parent_session_id.clone();
        let agent_id = id.clone();
        let finished = on_finished.clone();
        tokio::spawn(async move {
            let result = run.await;
            let status = if result.cancelled {
                "cancelled"
            } else if result.failed {
                "failed"
            } else {
                "completed"
            };
            on_life(
                session.clone(),
                life_id.clone(),
                status.into(),
                desc.clone(),
                Some(kind.as_str().into()),
                Some(agent_id.clone()),
            );
            if let Some(finished) = finished {
                finished(
                    session,
                    life_id,
                    status.into(),
                    desc,
                    kind.as_str().into(),
                    agent_id,
                )
                .await;
            }
        });
        return spawn_result_json(&id, SubagentStatus::Running, kind, &request.description, "");
    }

    let result = run.await;
    let status = if result.cancelled {
        SubagentStatus::Cancelled
    } else if result.failed {
        SubagentStatus::Failed
    } else {
        SubagentStatus::Completed
    };
    on_lifecycle(
        parent_session_id,
        lifecycle_id,
        status.as_str().into(),
        request.description.clone(),
        Some(kind.as_str().into()),
        Some(id.clone()),
    );
    spawn_result_json(&id, status, kind, &request.description, &result.summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_loop::LlmEndpoint;
    use serde_json::Value;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn sse_text(parts: &[&str]) -> String {
        let mut out = String::new();
        for (index, part) in parts.iter().enumerate() {
            let finish = if index + 1 == parts.len() {
                json!("stop")
            } else {
                Value::Null
            };
            let payload = json!({
                "choices": [{
                    "delta": { "content": part },
                    "finish_reason": finish,
                }]
            });
            out.push_str("data: ");
            out.push_str(&payload.to_string());
            out.push_str("\n\n");
        }
        out.push_str("data: [DONE]\n\n");
        out
    }

    async fn spawn_mock_llm(responses: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(VecDeque::from(responses)));
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                loop {
                    let n = match socket.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let body = state
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| sse_text(&["missing"]));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}/v1"), handle)
    }

    fn child_cfg(base_url: String, prompt: &str, kind: AgentKind) -> AgentTurnConfig {
        AgentTurnConfig {
            endpoint: LlmEndpoint {
                base_url,
                api_key: "test-key".into(),
                model: "grok-4.5".into(),
            },
            project_root: None,
            trusted: false,
            history: Vec::new(),
            user_prompt: prompt.into(),
            stop: Arc::new(AtomicBool::new(false)),
            client: agent_loop::http_client().unwrap(),
            max_tool_rounds: kind.max_tool_rounds(),
            stream_idle: std::time::Duration::from_secs(120),
            permission_gate: None,
            ask_user_gate: None,
            connectors: agent_loop::ConnectorTurn::default(),
            reasoning_effort: None,
            kind,
            spawn_depth: 1,
            subagents: agent_loop::SubagentHooks::default(),
            command_jobs: agent_loop::CommandJobHooks::default(),
            sandbox_profile: crate::runtime_compat::SandboxProfileV1::Off,
            skill_prompt_chars: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            allow_schedule_task: false,
            allow_skill_save: false,
        }
    }

    #[test]
    fn cancel_turn_only_stops_matching_children() {
        let mut registry = SubagentRegistry::default();
        let stop_a = Arc::new(AtomicBool::new(false));
        let stop_b = Arc::new(AtomicBool::new(false));
        registry.insert(SubagentRecord {
            id: "a".into(),
            parent_session_id: "s1".into(),
            parent_turn_id: "t1".into(),
            description: "one".into(),
            kind: AgentKind::Explore,
            background: false,
            status: SubagentStatus::Running,
            stop: Arc::clone(&stop_a),
            transcript: Vec::new(),
            summary: String::new(),
            wake_pending: false,
        });
        registry.insert(SubagentRecord {
            id: "b".into(),
            parent_session_id: "s1".into(),
            parent_turn_id: "t0".into(),
            description: "older".into(),
            kind: AgentKind::Explore,
            background: true,
            status: SubagentStatus::Running,
            stop: Arc::clone(&stop_b),
            transcript: Vec::new(),
            summary: String::new(),
            wake_pending: false,
        });
        assert_eq!(registry.cancel_turn("s1", "t1"), 1);
        assert!(stop_a.load(Ordering::SeqCst));
        assert!(!stop_b.load(Ordering::SeqCst));
        assert_eq!(
            registry.agents.get("b").unwrap().status,
            SubagentStatus::Running
        );
    }

    #[tokio::test]
    async fn child_turn_collects_summary() {
        let (base, server) = spawn_mock_llm(vec![sse_text(&["found the gate"])]).await;
        let stop = Arc::new(AtomicBool::new(false));
        let mut cfg = child_cfg(base, "search", AgentKind::Explore);
        cfg.stop = Arc::clone(&stop);
        let result = run_child_turn(cfg, stop).await;
        server.abort();
        assert!(
            result.summary.contains("found the gate"),
            "{}",
            result.summary
        );
        assert!(!result.failed);
        assert!(!result.cancelled);
    }

    #[tokio::test]
    async fn spawn_background_returns_running_id() {
        let (base, server) = spawn_mock_llm(vec![sse_text(&["done in bg"])]).await;
        let parent = child_cfg(base, "parent", AgentKind::Parent);
        let registry = Arc::new(tokio::sync::Mutex::new(SubagentRegistry::default()));
        let output = spawn_with_registry(
            Arc::clone(&registry),
            parent,
            "sess".into(),
            "turn".into(),
            SpawnAgentRequest {
                prompt: "look around".into(),
                description: "search repo".into(),
                agent_type: "explore".into(),
                background: true,
                tool_call_id: "call-bg".into(),
            },
            Arc::new(|_a, _b, _c, _d, _e, _f| {}),
            None,
        )
        .await;
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["status"], "running");
        assert_eq!(parsed["agentType"], "explore");
        let id = parsed["id"].as_str().unwrap().to_string();
        for _ in 0..40 {
            let status = registry.lock().await.agents.get(&id).map(|a| a.status);
            if matches!(status, Some(SubagentStatus::Completed)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        server.abort();
        let agent = registry.lock().await.get(&id).unwrap();
        assert_eq!(agent.status, SubagentStatus::Completed);
        assert!(agent.summary.contains("done in bg"), "{}", agent.summary);
        assert_eq!(registry.lock().await.pending_wake_count("sess"), 1);
        let wakes = registry.lock().await.take_pending_wakes("sess");
        assert_eq!(wakes.len(), 1);
        assert_eq!(registry.lock().await.pending_wake_count("sess"), 0);
        registry.lock().await.restore_pending_wakes(&wakes);
        assert_eq!(registry.lock().await.pending_wake_count("sess"), 1);
    }

    #[test]
    fn decide_auto_wake_joins_this_turn_only() {
        assert_eq!(
            decide_auto_wake(0, 0, false, false, false, true),
            AutoWakeDecision::None
        );
        assert_eq!(
            decide_auto_wake(1, 1, true, true, false, true),
            AutoWakeDecision::WaitForThisTurn
        );
        assert_eq!(
            decide_auto_wake(1, 0, true, false, false, true),
            AutoWakeDecision::DeferWhileParentRuns
        );
        assert_eq!(
            decide_auto_wake(1, 0, false, false, false, false),
            AutoWakeDecision::HoldForUserSend
        );
        assert_eq!(
            decide_auto_wake(2, 0, true, true, false, true),
            AutoWakeDecision::Start
        );
        assert_eq!(
            decide_auto_wake(1, 0, false, false, false, true),
            AutoWakeDecision::Start
        );
    }

    #[test]
    fn wake_prompt_is_model_facing_and_bounded() {
        let prompt = wake_prompt(&[SubagentView {
            id: "child-1".into(),
            parent_session_id: "s1".into(),
            description: "search repo".into(),
            agent_type: "explore".into(),
            status: SubagentStatus::Completed,
            background: true,
            summary: "found the gate".into(),
            transcript: Vec::new(),
        }]);
        assert!(prompt.contains("Background subagent results"));
        assert!(prompt.contains("found the gate"));
        assert!(prompt.contains("child-1"));
        assert!(prompt.contains("Continue the parent task"));
        let huge = "x".repeat(WAKE_PROMPT_CHARS);
        let truncated = wake_prompt(&[SubagentView {
            id: "child-2".into(),
            parent_session_id: "s1".into(),
            description: "huge".into(),
            agent_type: "general".into(),
            status: SubagentStatus::Failed,
            background: true,
            summary: huge,
            transcript: Vec::new(),
        }]);
        assert!(truncated.contains("truncated"));
        assert!(truncated.chars().count() <= WAKE_PROMPT_CHARS);
    }

    #[test]
    fn restore_pending_wakes_skips_cancelled() {
        let mut registry = SubagentRegistry::default();
        registry.insert(SubagentRecord {
            id: "done".into(),
            parent_session_id: "s1".into(),
            parent_turn_id: "t0".into(),
            description: "old".into(),
            kind: AgentKind::Explore,
            background: true,
            status: SubagentStatus::Completed,
            stop: Arc::new(AtomicBool::new(false)),
            transcript: Vec::new(),
            summary: "ok".into(),
            wake_pending: false,
        });
        registry.insert(SubagentRecord {
            id: "stopped".into(),
            parent_session_id: "s1".into(),
            parent_turn_id: "t1".into(),
            description: "now".into(),
            kind: AgentKind::Explore,
            background: true,
            status: SubagentStatus::Cancelled,
            stop: Arc::new(AtomicBool::new(true)),
            transcript: Vec::new(),
            summary: "subagent cancelled".into(),
            wake_pending: false,
        });
        registry.restore_pending_wakes(&[
            SubagentView {
                id: "done".into(),
                parent_session_id: "s1".into(),
                description: "old".into(),
                agent_type: "explore".into(),
                status: SubagentStatus::Completed,
                background: true,
                summary: "ok".into(),
                transcript: Vec::new(),
            },
            SubagentView {
                id: "stopped".into(),
                parent_session_id: "s1".into(),
                description: "now".into(),
                agent_type: "explore".into(),
                status: SubagentStatus::Cancelled,
                background: true,
                summary: "subagent cancelled".into(),
                transcript: Vec::new(),
            },
        ]);
        assert_eq!(registry.pending_wake_count("s1"), 1);
        assert!(registry.agents.get("done").unwrap().wake_pending);
        assert!(!registry.agents.get("stopped").unwrap().wake_pending);
    }

    #[test]
    fn explore_child_config_strips_spawn_and_writes() {
        let parent = child_cfg("http://127.0.0.1/v1".into(), "p", AgentKind::Parent);
        let child = child_config(
            &parent,
            AgentKind::Explore,
            "look".into(),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(child.spawn_depth, 1);
        assert!(!child.kind.allows_spawn());
        assert!(!child.kind.allows_write());
        let listed = agent_loop::tool_definitions_for(child.kind, child.spawn_depth).to_string();
        assert!(!listed.contains("spawn_agent"));
        assert!(!listed.contains("write_file"));
        assert!(listed.contains("grep"));
    }
}
