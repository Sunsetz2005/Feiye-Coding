//! Hosted background `run_command` jobs for the Sunsetz kernel.
//!
//! Parent-only. Jobs keep running after the model turn returns an id. Completion
//! sets a pending wake the same way background subagents do. Stop/Steer does not
//! cancel these jobs.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::agent_loop;
use crate::runtime_compat::SandboxProfileV1;

pub const MAX_RUNNING_COMMAND_JOBS: usize = 4;
pub const MAX_WAIT_IDS: usize = 20;
pub const BACKGROUND_COMMAND_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
const OUTPUT_WAKE_CHARS: usize = 4_000;

pub type CommandFinishedFn = Arc<
    dyn Fn(String, String, String, String, String) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Fired on registration and on every terminal status transition so the UI can
/// mirror hosted-job lifecycle (`session://command_job_v1`) without polling.
/// Synchronous and side-effect-only (an `AppHandle::emit`, in production) so
/// tests can inject a no-op instead of needing a live Tauri app.
pub type CommandJobEventFn = Arc<dyn Fn(CommandJobEventPayload) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandJobEventPayload {
    pub session_id: String,
    pub job_id: String,
    pub status: CommandJobStatus,
    pub command: String,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandJobStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl CommandJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone)]
pub struct CommandJobView {
    pub id: String,
    #[allow(dead_code)]
    pub parent_session_id: String,
    pub command: String,
    pub status: CommandJobStatus,
    pub output: String,
    pub summary: String,
}

/// UI-facing hosted-job summary for `session_command_jobs_list_v1`. Deliberately
/// omits `output` — the same "no full text body" convention as `session://permission`
/// previews; a caller that needs the full body still goes through `command_output`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandJobSummaryV1 {
    pub id: String,
    pub status: CommandJobStatus,
    pub command: String,
    pub summary: String,
}

struct CommandJobRecord {
    id: String,
    parent_session_id: String,
    #[allow(dead_code)]
    parent_turn_id: String,
    command: String,
    tool_call_id: String,
    title: String,
    status: CommandJobStatus,
    stop: Arc<AtomicBool>,
    output: String,
    summary: String,
    wake_pending: bool,
}

#[derive(Default)]
pub struct CommandJobRegistry {
    jobs: HashMap<String, CommandJobRecord>,
    slot: Arc<Notify>,
}

impl CommandJobRegistry {
    pub fn running_count(&self) -> usize {
        self.jobs
            .values()
            .filter(|job| matches!(job.status, CommandJobStatus::Running))
            .count()
    }

    pub fn pending_wake_count(&self, session_id: &str) -> usize {
        self.jobs
            .values()
            .filter(|job| job.parent_session_id == session_id && job.wake_pending)
            .count()
    }

    pub fn take_pending_wakes(&mut self, session_id: &str) -> Vec<CommandJobView> {
        let mut out = Vec::new();
        for job in self.jobs.values_mut() {
            if job.parent_session_id == session_id && job.wake_pending {
                job.wake_pending = false;
                out.push(job.view());
            }
        }
        out
    }

    fn view_json(&self, id: &str) -> String {
        let Some(job) = self.jobs.get(id) else {
            return format!("unknown command `{id}`");
        };
        json!({
            "id": id,
            "status": job.status.as_str(),
            "command": job.command,
            "output": job.output,
            "summary": job.summary,
        })
        .to_string()
    }

    pub fn output(&self, id: &str) -> String {
        self.view_json(id)
    }

    pub fn kill(&mut self, id: &str) -> String {
        let Some(job) = self.jobs.get_mut(id) else {
            return format!("unknown command `{id}`");
        };
        if job.status.is_terminal() {
            return self.view_json(id);
        }
        job.stop.store(true, Ordering::SeqCst);
        job.status = CommandJobStatus::Cancelled;
        job.summary = "command cancelled".into();
        self.slot.notify_waiters();
        self.view_json(id)
    }

    fn snapshot_ids(&self, ids: &[String]) -> Vec<CommandJobView> {
        ids.iter()
            .filter_map(|id| self.jobs.get(id).map(CommandJobRecord::view))
            .collect()
    }

    /// Hosted jobs for one session, in a stable (id-sorted) order. Read-only,
    /// used by the UI to restore the composer's hosted-count pill on reconnect.
    pub fn list_for_session(&self, session_id: &str) -> Vec<CommandJobSummaryV1> {
        let mut views: Vec<_> = self
            .jobs
            .values()
            .filter(|job| job.parent_session_id == session_id)
            .map(|job| CommandJobSummaryV1 {
                id: job.id.clone(),
                status: job.status,
                command: job.command.clone(),
                summary: job.summary.clone(),
            })
            .collect();
        views.sort_by(|left, right| left.id.cmp(&right.id));
        views
    }
}

fn emit_command_job_event(
    on_event: &Option<CommandJobEventFn>,
    session_id: &str,
    job_id: &str,
    status: CommandJobStatus,
    command: &str,
    summary: &str,
) {
    if let Some(on_event) = on_event {
        on_event(CommandJobEventPayload {
            session_id: session_id.to_string(),
            job_id: job_id.to_string(),
            status,
            command: command.to_string(),
            summary: summary.to_string(),
        });
    }
}

impl CommandJobRecord {
    fn view(&self) -> CommandJobView {
        CommandJobView {
            id: self.id.clone(),
            parent_session_id: self.parent_session_id.clone(),
            command: self.command.clone(),
            status: self.status,
            output: self.output.clone(),
            summary: self.summary.clone(),
        }
    }
}

pub fn wake_prompt(views: &[CommandJobView]) -> String {
    let mut body = String::from("Background command results:\n");
    let reserve = 160usize;
    let budget = 16_384usize.saturating_sub(reserve);
    for view in views {
        let summary = view.summary.trim();
        let line = format!(
            "- [{}] `{}` (id={}): {}\n",
            view.status.as_str(),
            view.command,
            view.id,
            summary
        );
        if body.chars().count() + line.chars().count() > budget {
            body.push_str("- … additional command results truncated\n");
            break;
        }
        body.push_str(&line);
    }
    body.push_str(
        "Continue the parent task using these results. Do not claim you are still waiting for them.",
    );
    body
}

pub struct StartCommandJobRequest {
    pub command: String,
    pub cwd: PathBuf,
    pub project_root: PathBuf,
    pub sandbox: SandboxProfileV1,
    pub tool_call_id: String,
    pub title: String,
}

pub async fn start_with_registry(
    registry: Arc<tokio::sync::Mutex<CommandJobRegistry>>,
    session_id: String,
    turn_id: String,
    request: StartCommandJobRequest,
    on_finished: Option<CommandFinishedFn>,
    on_event: Option<CommandJobEventFn>,
) -> String {
    let stop = Arc::new(AtomicBool::new(false));
    let id = Uuid::new_v4().to_string();
    {
        let mut guard = registry.lock().await;
        if guard.running_count() >= MAX_RUNNING_COMMAND_JOBS {
            return format!(
                "too many running commands (max {MAX_RUNNING_COMMAND_JOBS}). Wait or kill one."
            );
        }
        guard.jobs.insert(
            id.clone(),
            CommandJobRecord {
                id: id.clone(),
                parent_session_id: session_id.clone(),
                parent_turn_id: turn_id,
                command: request.command.clone(),
                tool_call_id: request.tool_call_id.clone(),
                title: request.title.clone(),
                status: CommandJobStatus::Running,
                stop: Arc::clone(&stop),
                output: String::new(),
                summary: String::new(),
                wake_pending: false,
            },
        );
    }
    emit_command_job_event(
        &on_event,
        &session_id,
        &id,
        CommandJobStatus::Running,
        &request.command,
        "",
    );
    let run_id = id.clone();
    let registry_run = Arc::clone(&registry);
    let command_label = request.command.clone();
    let on_event_done = on_event.clone();
    let session_done = session_id.clone();
    tokio::spawn(async move {
        let outcome = agent_loop::execute_run_command_timed(
            &request.command,
            &request.cwd,
            Arc::clone(&stop),
            BACKGROUND_COMMAND_TIMEOUT,
            request.sandbox,
            &request.project_root,
        )
        .await;
        let (status, output, summary) = match outcome {
            Ok(agent_loop::RunCommandOutcome::Cancelled) => (
                CommandJobStatus::Cancelled,
                String::new(),
                "command cancelled".to_string(),
            ),
            Ok(agent_loop::RunCommandOutcome::Output(text)) => {
                let summary = bound_wake_output(&text);
                (CommandJobStatus::Completed, text, summary)
            }
            Err(error) => (CommandJobStatus::Failed, error.clone(), error),
        };
        let mut tool_id = request.tool_call_id;
        let mut title = request.title;
        let mut final_status = status;
        let mut final_summary = String::new();
        {
            let mut guard = registry_run.lock().await;
            if let Some(job) = guard.jobs.get_mut(&run_id) {
                if matches!(job.status, CommandJobStatus::Cancelled) && stop.load(Ordering::SeqCst)
                {
                    job.output = output;
                    if job.summary.is_empty() {
                        job.summary = summary;
                    }
                } else {
                    job.status = status;
                    job.output = output;
                    job.summary = summary;
                }
                if matches!(
                    job.status,
                    CommandJobStatus::Completed | CommandJobStatus::Failed
                ) {
                    job.wake_pending = true;
                }
                tool_id = job.tool_call_id.clone();
                title = job.title.clone();
                final_status = job.status;
                final_summary = job.summary.clone();
            }
            guard.slot.notify_waiters();
        }
        emit_command_job_event(
            &on_event_done,
            &session_done,
            &run_id,
            final_status,
            &request.command,
            &final_summary,
        );
        if let Some(on_finished) = on_finished {
            let status_str = {
                let guard = registry_run.lock().await;
                guard
                    .jobs
                    .get(&run_id)
                    .map(|job| job.status.as_str().to_string())
                    .unwrap_or_else(|| status.as_str().to_string())
            };
            on_finished(session_id, tool_id, status_str, title, run_id).await;
        }
    });
    json!({
        "id": id,
        "status": "running",
        "command": command_label,
    })
    .to_string()
}

fn bound_wake_output(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= OUTPUT_WAKE_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(OUTPUT_WAKE_CHARS).collect();
    out.push('…');
    out
}

pub async fn wait_for_jobs(
    registry: Arc<tokio::sync::Mutex<CommandJobRegistry>>,
    ids: Vec<String>,
    wait_any: bool,
    timeout: Duration,
) -> String {
    if ids.is_empty() {
        return "wait_commands requires `ids`".into();
    }
    if ids.len() > MAX_WAIT_IDS {
        return format!("wait_commands accepts at most {MAX_WAIT_IDS} ids");
    }
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        {
            let guard = registry.lock().await;
            let views = guard.snapshot_ids(&ids);
            let known = views.len();
            if known == 0 {
                return format!("unknown command `{}`", ids[0]);
            }
            let terminal = views
                .iter()
                .filter(|view| view.status.is_terminal())
                .count();
            let done = if wait_any {
                terminal > 0
            } else {
                terminal == ids.len()
            };
            if done {
                return json!({
                    "mode": if wait_any { "wait_any" } else { "wait_all" },
                    "jobs": views.iter().map(|view| json!({
                        "id": view.id,
                        "status": view.status.as_str(),
                        "command": view.command,
                        "output": view.output,
                        "summary": view.summary,
                    })).collect::<Vec<_>>(),
                })
                .to_string();
            }
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            let guard = registry.lock().await;
            let views = guard.snapshot_ids(&ids);
            return json!({
                "mode": if wait_any { "wait_any" } else { "wait_all" },
                "timedOut": true,
                "jobs": views.iter().map(|view| json!({
                    "id": view.id,
                    "status": view.status.as_str(),
                    "command": view.command,
                    "output": view.output,
                    "summary": view.summary,
                })).collect::<Vec<_>>(),
            })
            .to_string();
        }
        let notified = {
            let guard = registry.lock().await;
            Arc::clone(&guard.slot)
        };
        tokio::select! {
            _ = notified.notified() => {}
            _ = tokio::time::sleep(remaining) => {}
        }
    }
}

pub async fn output_with_optional_wait(
    registry: Arc<tokio::sync::Mutex<CommandJobRegistry>>,
    id: String,
    timeout_ms: Option<u64>,
) -> String {
    if let Some(ms) = timeout_ms {
        return wait_for_jobs(
            registry,
            vec![id],
            true,
            Duration::from_millis(ms.max(1)),
        )
        .await;
    }
    registry.lock().await.output(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    fn temp_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sunsetz-command-jobs-{label}-{}",
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[tokio::test]
    async fn background_sleep_returns_id_then_completes() {
        let root = temp_root("bg-sleep");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let finished = Arc::new(AtomicU32::new(0));
        let hits = Arc::clone(&finished);
        let on_finished: CommandFinishedFn = Arc::new(move |_s, _t, _st, _title, _id| {
            hits.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {})
        });
        let hang = if cfg!(windows) {
            "ping -n 2 127.0.0.1 >NUL"
        } else {
            "sleep 0.2"
        };
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            "turn".into(),
            StartCommandJobRequest {
                command: hang.into(),
                cwd: root.clone(),
                project_root: root.clone(),
                sandbox: SandboxProfileV1::Off,
                tool_call_id: "tool-1".into(),
                title: "Run sleep".into(),
            },
            Some(on_finished),
            None,
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        assert_eq!(parsed["status"], "running");
        let waited = wait_for_jobs(
            Arc::clone(&registry),
            vec![id.clone()],
            true,
            Duration::from_secs(5),
        )
        .await;
        let wait_json: serde_json::Value = serde_json::from_str(&waited).unwrap();
        assert_eq!(wait_json["jobs"][0]["status"], "completed");
        assert_eq!(finished.load(Ordering::SeqCst), 1);
        assert_eq!(registry.lock().await.pending_wake_count("sess"), 1);
        let taken = registry.lock().await.take_pending_wakes("sess");
        assert_eq!(taken.len(), 1);
        assert_eq!(registry.lock().await.pending_wake_count("sess"), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn kill_stops_running_job() {
        let root = temp_root("bg-kill");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let hang = if cfg!(windows) {
            "ping -n 30 127.0.0.1 >NUL"
        } else {
            "sleep 30"
        };
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            "turn".into(),
            StartCommandJobRequest {
                command: hang.into(),
                cwd: root.clone(),
                project_root: root.clone(),
                sandbox: SandboxProfileV1::Off,
                tool_call_id: "tool-1".into(),
                title: "Run sleep".into(),
            },
            None,
            None,
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        let killed = registry.lock().await.kill(&id);
        let kill_json: serde_json::Value = serde_json::from_str(&killed).unwrap();
        assert_eq!(kill_json["status"], "cancelled");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn event_sink() -> (
        CommandJobEventFn,
        Arc<std::sync::Mutex<Vec<CommandJobEventPayload>>>,
    ) {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let on_event: CommandJobEventFn = Arc::new(move |payload| {
            captured.lock().unwrap().push(payload);
        });
        (on_event, events)
    }

    #[tokio::test]
    async fn success_path_emits_running_then_completed() {
        let root = temp_root("bg-events-success");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let (on_event, events) = event_sink();
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            "turn".into(),
            StartCommandJobRequest {
                command: "echo hi".into(),
                cwd: root.clone(),
                project_root: root.clone(),
                sandbox: SandboxProfileV1::Off,
                tool_call_id: "tool-1".into(),
                title: "Echo".into(),
            },
            None,
            Some(on_event),
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        wait_for_jobs(
            Arc::clone(&registry),
            vec![id.clone()],
            true,
            Duration::from_secs(5),
        )
        .await;
        let seen = events.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "expected running + completed events, got {seen:?}");
        assert_eq!(seen[0].job_id, id);
        assert_eq!(seen[0].status, CommandJobStatus::Running);
        assert_eq!(seen[1].status, CommandJobStatus::Completed);
        assert_eq!(seen[1].session_id, "sess");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn failure_path_emits_failed_event() {
        let root = temp_root("bg-events-failure");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let (on_event, events) = event_sink();
        // A non-zero exit still resolves as `Output` (`execute_run_command_timed`
        // just appends "exit N" to the text) — an empty command is what actually
        // fails, rejected by `plan_run_command` before any process spawns.
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            "turn".into(),
            StartCommandJobRequest {
                command: "".into(),
                cwd: root.clone(),
                project_root: root.clone(),
                sandbox: SandboxProfileV1::Off,
                tool_call_id: "tool-1".into(),
                title: "Fail".into(),
            },
            None,
            Some(on_event),
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        wait_for_jobs(
            Arc::clone(&registry),
            vec![id],
            true,
            Duration::from_secs(5),
        )
        .await;
        let seen = events.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].status, CommandJobStatus::Failed);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn cancel_path_emits_cancelled_event() {
        let root = temp_root("bg-events-cancel");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let (on_event, events) = event_sink();
        let hang = if cfg!(windows) {
            "ping -n 30 127.0.0.1 >NUL"
        } else {
            "sleep 30"
        };
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            "turn".into(),
            StartCommandJobRequest {
                command: hang.into(),
                cwd: root.clone(),
                project_root: root.clone(),
                sandbox: SandboxProfileV1::Off,
                tool_call_id: "tool-1".into(),
                title: "Run sleep".into(),
            },
            None,
            Some(on_event),
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        registry.lock().await.kill(&id);
        // `kill()` marks the registry cancelled synchronously, so `wait_for_jobs`
        // (which only polls registry status) can return before the spawned task
        // notices `stop` and reaches the completion block that emits the second
        // event. Poll the event sink itself instead of the registry.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while events.lock().unwrap().len() < 2 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let seen = events.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "expected running + cancelled events, got {seen:?}");
        assert_eq!(seen[1].status, CommandJobStatus::Cancelled);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn list_for_session_scopes_by_session_and_ignores_others() {
        let root = temp_root("bg-list");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let hang = if cfg!(windows) {
            "ping -n 30 127.0.0.1 >NUL"
        } else {
            "sleep 30"
        };
        for session in ["sess-a", "sess-a", "sess-b"] {
            start_with_registry(
                Arc::clone(&registry),
                session.into(),
                "turn".into(),
                StartCommandJobRequest {
                    command: hang.into(),
                    cwd: root.clone(),
                    project_root: root.clone(),
                    sandbox: SandboxProfileV1::Off,
                    tool_call_id: "tool-1".into(),
                    title: "Run sleep".into(),
                },
                None,
                None,
            )
            .await;
        }
        let guard = registry.lock().await;
        assert_eq!(guard.list_for_session("sess-a").len(), 2);
        assert_eq!(guard.list_for_session("sess-b").len(), 1);
        assert_eq!(guard.list_for_session("sess-c").len(), 0);
        drop(guard);
        // Collect ids into an owned Vec in its own `let` first: a MutexGuard
        // created in a `for` loop's head expression is kept alive for the
        // whole loop body (temporaries in a for-loop scrutinee live until the
        // loop ends), so locking again inside the loop would deadlock.
        let ids: Vec<String> = registry.lock().await.jobs.keys().cloned().collect();
        for id in ids {
            registry.lock().await.kill(&id);
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
