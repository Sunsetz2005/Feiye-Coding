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

use regex::Regex;
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
/// Wake deliveries a single `monitor` subscription gets before it auto-detaches.
/// Not an OS-resident watch — bounded so a chatty command can't keep waking the
/// parent turn forever; the model can call `monitor` again to re-arm.
const MAX_MONITOR_WAKES: u32 = 20;
/// Matched (or, with no pattern, any) lines buffered per wake delivery.
const MAX_MONITOR_LINES_PER_WAKE: usize = 200;

/// Fired for the parent session when a `monitor` subscription has new lines
/// ready to deliver. Async and side-effect-only, matching `CommandFinishedFn` —
/// in production this calls `SessionManager::maybe_start_wake_turn`.
pub type MonitorTriggeredFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

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

struct MonitorSubscription {
    id: String,
    pattern: Option<Regex>,
    pending_lines: Vec<String>,
    wakes_used: u32,
}

/// One matched-line delivery for a `monitor` subscription, taken alongside the
/// command-job wakes in the same wake-turn (`take_pending_wakes`'s sibling).
#[derive(Debug, Clone)]
pub struct MonitorWakeView {
    pub monitor_id: String,
    pub job_id: String,
    pub command: String,
    pub lines: Vec<String>,
    /// True when this delivery exhausted `MAX_MONITOR_WAKES` and the
    /// subscription was auto-removed — the model must call `monitor` again.
    pub detached: bool,
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
    monitors: Vec<MonitorSubscription>,
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

    /// Subscribe to a running job's output. Rejects unknown or already-terminal
    /// jobs — a finished job will never produce another line, so there is
    /// nothing to wake on. `pattern`, if given, must be a valid regex.
    pub fn start_monitor(&mut self, job_id: &str, pattern: Option<String>) -> Result<String, String> {
        let job = self
            .jobs
            .get_mut(job_id)
            .ok_or_else(|| format!("unknown command `{job_id}`"))?;
        if job.status.is_terminal() {
            return Err(format!(
                "command `{job_id}` already finished ({}); nothing left to monitor",
                job.status.as_str()
            ));
        }
        let regex = match pattern.as_deref().map(str::trim) {
            Some(raw) if !raw.is_empty() => {
                Some(Regex::new(raw).map_err(|error| format!("invalid pattern: {error}"))?)
            }
            _ => None,
        };
        let monitor_id = Uuid::new_v4().to_string();
        job.monitors.push(MonitorSubscription {
            id: monitor_id.clone(),
            pattern: regex,
            pending_lines: Vec::new(),
            wakes_used: 0,
        });
        Ok(monitor_id)
    }

    /// Appends a newly-arrived output line to the job's live buffer (so
    /// `command_output`/`wait_commands` can see partial output mid-run too,
    /// not just after completion) and buffers it for any matching monitor.
    /// Returns true if at least one subscription now has a line to deliver.
    fn record_output_line(&mut self, job_id: &str, line: &str) -> bool {
        let Some(job) = self.jobs.get_mut(job_id) else {
            return false;
        };
        if !job.output.is_empty() {
            job.output.push('\n');
        }
        job.output.push_str(line);
        let mut any_hit = false;
        for monitor in &mut job.monitors {
            let matches = monitor
                .pattern
                .as_ref()
                .map(|re| re.is_match(line))
                .unwrap_or(true);
            if matches && monitor.pending_lines.len() < MAX_MONITOR_LINES_PER_WAKE {
                monitor.pending_lines.push(line.to_string());
                any_hit = true;
            }
        }
        any_hit
    }

    pub fn pending_monitor_wake_count(&self, session_id: &str) -> usize {
        self.jobs
            .values()
            .filter(|job| job.parent_session_id == session_id)
            .flat_map(|job| job.monitors.iter())
            .filter(|monitor| !monitor.pending_lines.is_empty())
            .count()
    }

    /// Drains every subscription with buffered lines for one session, one
    /// delivery (`MonitorWakeView`) per subscription. A subscription that hits
    /// `MAX_MONITOR_WAKES` on this delivery is removed (`detached: true`).
    pub fn take_pending_monitor_wakes(&mut self, session_id: &str) -> Vec<MonitorWakeView> {
        let mut out = Vec::new();
        for job in self.jobs.values_mut() {
            if job.parent_session_id != session_id {
                continue;
            }
            let command = job.command.clone();
            let job_id = job.id.clone();
            job.monitors.retain_mut(|monitor| {
                if monitor.pending_lines.is_empty() {
                    return true;
                }
                let lines = std::mem::take(&mut monitor.pending_lines);
                monitor.wakes_used += 1;
                let detached = monitor.wakes_used >= MAX_MONITOR_WAKES;
                out.push(MonitorWakeView {
                    monitor_id: monitor.id.clone(),
                    job_id: job_id.clone(),
                    command: command.clone(),
                    lines,
                    detached,
                });
                !detached
            });
        }
        out
    }
}

pub fn monitor_wake_prompt(views: &[MonitorWakeView]) -> String {
    let mut body = String::from("Background command monitor output:\n");
    let reserve = 160usize;
    let budget = 16_384usize.saturating_sub(reserve);
    for view in views {
        let joined = view.lines.join("\n");
        let line = format!(
            "- monitor {} on `{}` (id={}){}:\n{}\n",
            view.monitor_id,
            view.command,
            view.job_id,
            if view.detached {
                " [wake budget exhausted, detached — call monitor again to re-arm]"
            } else {
                ""
            },
            joined
        );
        if body.chars().count() + line.chars().count() > budget {
            body.push_str("- … additional monitor output truncated\n");
            break;
        }
        body.push_str(&line);
    }
    body.push_str("Continue the parent task using this output. Do not claim you are still waiting for it.");
    body
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
    on_monitor_wake: Option<MonitorTriggeredFn>,
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
                monitors: Vec::new(),
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
    let on_line: agent_loop::CommandLineFn = {
        let registry_line = Arc::clone(&registry);
        let line_job_id = id.clone();
        let line_session = session_id.clone();
        Arc::new(move |line: String| {
            let registry = Arc::clone(&registry_line);
            let job_id = line_job_id.clone();
            let session = line_session.clone();
            let on_wake = on_monitor_wake.clone();
            Box::pin(async move {
                let triggered = registry.lock().await.record_output_line(&job_id, &line);
                if triggered {
                    if let Some(on_wake) = on_wake {
                        on_wake(session).await;
                    }
                }
            })
        })
    };
    tokio::spawn(async move {
        let outcome = agent_loop::execute_run_command_timed(
            &request.command,
            &request.cwd,
            Arc::clone(&stop),
            BACKGROUND_COMMAND_TIMEOUT,
            request.sandbox,
            &request.project_root,
            Some(on_line),
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
            None,
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
            None,
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
            None,
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

    fn staggered_lines_command() -> &'static str {
        if cfg!(windows) {
            "echo start & ping -n 1 127.0.0.1 >NUL & echo match-me & ping -n 1 127.0.0.1 >NUL & echo done"
        } else {
            "echo start; sleep 0.2; echo match-me; sleep 0.2; echo done"
        }
    }

    #[tokio::test]
    async fn monitor_wakes_only_on_matching_lines() {
        let root = temp_root("bg-monitor-pattern");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            "turn".into(),
            StartCommandJobRequest {
                command: staggered_lines_command().into(),
                cwd: root.clone(),
                project_root: root.clone(),
                sandbox: SandboxProfileV1::Off,
                tool_call_id: "tool-1".into(),
                title: "Watch".into(),
            },
            None,
            None,
            None,
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        // Subscribe while the job is still running (the first `echo` + sleep
        // has not resolved yet); a terminal job is rejected by `start_monitor`.
        let monitor_id = registry
            .lock()
            .await
            .start_monitor(&id, Some("match".into()))
            .expect("monitor should attach to a running job");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while registry.lock().await.pending_monitor_wake_count("sess") == 0
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let wakes = registry.lock().await.take_pending_monitor_wakes("sess");
        assert_eq!(wakes.len(), 1, "{wakes:?}");
        assert_eq!(wakes[0].monitor_id, monitor_id);
        assert_eq!(wakes[0].job_id, id);
        assert_eq!(wakes[0].lines, vec!["match-me".to_string()]);
        assert!(!wakes[0].detached);
        wait_for_jobs(Arc::clone(&registry), vec![id], true, Duration::from_secs(5)).await;
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn monitor_without_pattern_wakes_on_any_line() {
        let root = temp_root("bg-monitor-any");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            "turn".into(),
            StartCommandJobRequest {
                command: staggered_lines_command().into(),
                cwd: root.clone(),
                project_root: root.clone(),
                sandbox: SandboxProfileV1::Off,
                tool_call_id: "tool-1".into(),
                title: "Watch".into(),
            },
            None,
            None,
            None,
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        registry
            .lock()
            .await
            .start_monitor(&id, None)
            .expect("monitor should attach to a running job");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while registry.lock().await.pending_monitor_wake_count("sess") == 0
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let wakes = registry.lock().await.take_pending_monitor_wakes("sess");
        assert_eq!(wakes.len(), 1, "{wakes:?}");
        assert_eq!(wakes[0].lines[0], "start");
        wait_for_jobs(Arc::clone(&registry), vec![id], true, Duration::from_secs(5)).await;
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn start_monitor_rejects_unknown_and_terminal_jobs() {
        let root = temp_root("bg-monitor-reject");
        let registry = Arc::new(tokio::sync::Mutex::new(CommandJobRegistry::default()));
        let unknown = registry.lock().await.start_monitor("nope", None);
        assert!(unknown.unwrap_err().contains("unknown command"));
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
            None,
            None,
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        wait_for_jobs(Arc::clone(&registry), vec![id.clone()], true, Duration::from_secs(5)).await;
        let terminal = registry.lock().await.start_monitor(&id, None);
        assert!(
            terminal.as_ref().unwrap_err().contains("already finished"),
            "{terminal:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn monitor_auto_detaches_after_wake_budget_exhausted() {
        let mut registry = CommandJobRegistry::default();
        let job_id = "job-1".to_string();
        registry.jobs.insert(
            job_id.clone(),
            CommandJobRecord {
                id: job_id.clone(),
                parent_session_id: "sess".into(),
                parent_turn_id: "turn".into(),
                command: "watch".into(),
                tool_call_id: "tool".into(),
                title: "Watch".into(),
                status: CommandJobStatus::Running,
                stop: Arc::new(AtomicBool::new(false)),
                output: String::new(),
                summary: String::new(),
                wake_pending: false,
                monitors: Vec::new(),
            },
        );
        let monitor_id = registry.start_monitor(&job_id, None).unwrap();
        for i in 0..MAX_MONITOR_WAKES {
            assert!(registry.record_output_line(&job_id, &format!("line-{i}")));
            let mut wakes = registry.take_pending_monitor_wakes("sess");
            assert_eq!(wakes.len(), 1, "iteration {i}: {wakes:?}");
            let wake = wakes.remove(0);
            assert_eq!(wake.monitor_id, monitor_id);
            let expect_detached = i + 1 == MAX_MONITOR_WAKES;
            assert_eq!(wake.detached, expect_detached, "iteration {i}");
        }
        // Budget exhausted: the subscription was removed on the last delivery,
        // so further lines have nothing left to notify.
        assert!(!registry.record_output_line(&job_id, "line-after-detach"));
        assert_eq!(registry.pending_monitor_wake_count("sess"), 0);
    }
}
