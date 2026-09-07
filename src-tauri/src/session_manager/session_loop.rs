//! Same-session loop: a bounded, in-memory recurring wake-turn bound to one
//! `sessionId`. Distinct from `crate::automation_scheduler` (disk-backed
//! ledger, ≥15-minute floor, opens a *new* session per run, survives app
//! restart). A loop here is a lighter, shorter-lived mechanism: it lives only
//! in this process's memory, dies with the session/app, floors at 60 seconds,
//! and expires after 7 days even if never cancelled.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::Notify;
use uuid::Uuid;

/// Host-enforced floor. The model's requested interval is clamped up to this,
/// never trusted as-is — a tight loop would otherwise burn tool rounds and
/// tokens waking the parent turn far more often than any real task needs.
pub const MIN_LOOP_INTERVAL_SECS: u64 = 60;
/// A loop that is never cancelled still stops firing after this long.
pub const MAX_LOOP_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Loops per session. Bounded for the same reason as `MAX_RUNNING_COMMAND_JOBS`.
pub const MAX_LOOPS_PER_SESSION: usize = 4;

/// Fired when a loop's tick is due. Async and side-effect-only, matching
/// `MonitorTriggeredFn` — in production this calls `SessionManager::maybe_start_wake_turn`.
pub type LoopTickFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

struct LoopRecord {
    id: String,
    session_id: String,
    prompt: String,
    #[allow(dead_code)]
    interval: Duration,
    tick_pending: bool,
    #[allow(dead_code)]
    ticks_used: u32,
    cancel: Arc<Notify>,
}

/// One due tick, taken alongside subagent/command-job/monitor wakes in the
/// same wake-turn (`take_pending_wakes`'s sibling).
#[derive(Debug, Clone)]
pub struct LoopTickView {
    pub loop_id: String,
    pub prompt: String,
}

#[derive(Default)]
pub struct LoopRegistry {
    loops: HashMap<String, LoopRecord>,
}

impl LoopRegistry {
    pub fn running_count(&self, session_id: &str) -> usize {
        self.loops
            .values()
            .filter(|record| record.session_id == session_id)
            .count()
    }

    pub fn pending_tick_count(&self, session_id: &str) -> usize {
        self.loops
            .values()
            .filter(|record| record.session_id == session_id && record.tick_pending)
            .count()
    }

    pub fn take_pending_ticks(&mut self, session_id: &str) -> Vec<LoopTickView> {
        let mut out = Vec::new();
        for record in self.loops.values_mut() {
            if record.session_id == session_id && record.tick_pending {
                record.tick_pending = false;
                out.push(LoopTickView {
                    loop_id: record.id.clone(),
                    prompt: record.prompt.clone(),
                });
            }
        }
        out
    }

    /// Stop a loop by id, scoped to the requesting session so one session
    /// cannot cancel another's loop. Idempotent-looking on a foreign id: it
    /// reports "unknown" the same as a never-existed id, revealing nothing.
    pub fn cancel(&mut self, session_id: &str, id: &str) -> String {
        match self.loops.get(id) {
            Some(record) if record.session_id == session_id => {
                let record = self.loops.remove(id).expect("checked above");
                record.cancel.notify_waiters();
                json!({ "id": id, "status": "cancelled" }).to_string()
            }
            _ => format!("unknown loop `{id}`"),
        }
    }
}

pub fn wake_prompt(views: &[LoopTickView]) -> String {
    let mut body = String::from("Same-session loop tick:\n");
    let reserve = 160usize;
    let budget = 16_384usize.saturating_sub(reserve);
    for view in views {
        let task = view.prompt.trim();
        let line = format!(
            "- loop {} tick: {}\n",
            view.loop_id,
            if task.is_empty() {
                "continue the recurring task."
            } else {
                task
            }
        );
        if body.chars().count() + line.chars().count() > budget {
            body.push_str("- … additional loop ticks truncated\n");
            break;
        }
        body.push_str(&line);
    }
    body.push_str(
        "Continue the loop's task now. Do not claim you are still waiting for a trigger.",
    );
    body
}

/// Starts (or rejects) a same-session loop and spawns its tick timer. The
/// timer keeps running independent of whether prior ticks were ever drained —
/// each tick just re-arms `tick_pending`, and `decide_auto_wake` (shared with
/// subagents/command jobs/monitor) decides whether to actually interrupt the
/// parent turn right now.
pub fn start_with_registry(
    registry: Arc<tokio::sync::Mutex<LoopRegistry>>,
    session_id: String,
    interval_secs: u64,
    prompt: String,
    on_tick: LoopTickFn,
) -> Pin<Box<dyn Future<Output = String> + Send>> {
    Box::pin(async move {
        let interval = Duration::from_secs(interval_secs.max(MIN_LOOP_INTERVAL_SECS));
        let id = Uuid::new_v4().to_string();
        let cancel = Arc::new(Notify::new());
        {
            let mut guard = registry.lock().await;
            if guard.running_count(&session_id) >= MAX_LOOPS_PER_SESSION {
                return format!(
                    "too many loops for this session (max {MAX_LOOPS_PER_SESSION}). Cancel one first."
                );
            }
            guard.loops.insert(
                id.clone(),
                LoopRecord {
                    id: id.clone(),
                    session_id: session_id.clone(),
                    prompt: prompt.clone(),
                    interval,
                    tick_pending: false,
                    ticks_used: 0,
                    cancel: Arc::clone(&cancel),
                },
            );
        }
        let run_id = id.clone();
        let run_session = session_id.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + MAX_LOOP_TTL;
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    registry.lock().await.loops.remove(&run_id);
                    break;
                }
                let sleep_for = interval.min(remaining);
                tokio::select! {
                    _ = tokio::time::sleep(sleep_for) => {}
                    _ = cancel.notified() => { break; }
                }
                if tokio::time::Instant::now() >= deadline {
                    registry.lock().await.loops.remove(&run_id);
                    break;
                }
                let alive = {
                    let mut guard = registry.lock().await;
                    match guard.loops.get_mut(&run_id) {
                        Some(record) => {
                            record.tick_pending = true;
                            record.ticks_used += 1;
                            true
                        }
                        None => false,
                    }
                };
                if !alive {
                    break;
                }
                on_tick(run_session.clone()).await;
            }
        });
        json!({
            "id": id,
            "status": "running",
            "intervalSecs": interval.as_secs(),
        })
        .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tick_counter() -> (LoopTickFn, Arc<AtomicU32>) {
        let hits = Arc::new(AtomicU32::new(0));
        let counted = Arc::clone(&hits);
        let on_tick: LoopTickFn = Arc::new(move |_session| {
            let counted = Arc::clone(&counted);
            Box::pin(async move {
                counted.fetch_add(1, Ordering::SeqCst);
            })
        });
        (on_tick, hits)
    }

    #[tokio::test]
    async fn start_clamps_interval_to_the_60s_floor() {
        let registry = Arc::new(tokio::sync::Mutex::new(LoopRegistry::default()));
        let (on_tick, _hits) = tick_counter();
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            5,
            "watch the build".into(),
            on_tick,
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        assert_eq!(parsed["intervalSecs"], 60);
        let id = parsed["id"].as_str().unwrap().to_string();
        let cancelled = registry.lock().await.cancel("sess", &id);
        let cancel_json: serde_json::Value = serde_json::from_str(&cancelled).unwrap();
        assert_eq!(cancel_json["status"], "cancelled");
    }

    #[tokio::test]
    async fn ticks_accumulate_and_drain_as_pending() {
        let registry = Arc::new(tokio::sync::Mutex::new(LoopRegistry::default()));
        let (on_tick, hits) = tick_counter();
        // Directly insert a short-interval record (bypassing the 60s floor)
        // so the test doesn't need to wait a full minute for a real tick.
        let id = "loop-1".to_string();
        let cancel = Arc::new(Notify::new());
        registry.lock().await.loops.insert(
            id.clone(),
            LoopRecord {
                id: id.clone(),
                session_id: "sess".into(),
                prompt: "poll ci".into(),
                interval: Duration::from_millis(20),
                tick_pending: false,
                ticks_used: 0,
                cancel: Arc::clone(&cancel),
            },
        );
        let run_id = id.clone();
        let run_registry = Arc::clone(&registry);
        tokio::spawn(async move {
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let mut guard = run_registry.lock().await;
                if let Some(record) = guard.loops.get_mut(&run_id) {
                    record.tick_pending = true;
                    record.ticks_used += 1;
                } else {
                    break;
                }
                drop(guard);
                on_tick("sess".into()).await;
            }
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while hits.load(Ordering::SeqCst) < 3 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        assert_eq!(registry.lock().await.pending_tick_count("sess"), 1);
        let taken = registry.lock().await.take_pending_ticks("sess");
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].loop_id, "loop-1");
        assert_eq!(taken[0].prompt, "poll ci");
        assert_eq!(registry.lock().await.pending_tick_count("sess"), 0);
    }

    #[tokio::test]
    async fn cancel_is_scoped_to_the_owning_session() {
        let registry = Arc::new(tokio::sync::Mutex::new(LoopRegistry::default()));
        let (on_tick, _hits) = tick_counter();
        let started = start_with_registry(
            Arc::clone(&registry),
            "sess-a".into(),
            60,
            String::new(),
            on_tick,
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        let foreign = registry.lock().await.cancel("sess-b", &id);
        assert!(foreign.contains("unknown loop"));
        let owner = registry.lock().await.cancel("sess-a", &id);
        let owner_json: serde_json::Value = serde_json::from_str(&owner).unwrap();
        assert_eq!(owner_json["status"], "cancelled");
    }

    #[tokio::test]
    async fn start_rejects_beyond_the_per_session_cap() {
        let registry = Arc::new(tokio::sync::Mutex::new(LoopRegistry::default()));
        for _ in 0..MAX_LOOPS_PER_SESSION {
            let (on_tick, _hits) = tick_counter();
            let started = start_with_registry(
                Arc::clone(&registry),
                "sess".into(),
                60,
                String::new(),
                on_tick,
            )
            .await;
            let parsed: serde_json::Value = serde_json::from_str(&started).unwrap();
            assert_eq!(parsed["status"], "running", "{parsed:?}");
        }
        let (on_tick, _hits) = tick_counter();
        let rejected = start_with_registry(
            Arc::clone(&registry),
            "sess".into(),
            60,
            String::new(),
            on_tick,
        )
        .await;
        assert!(rejected.contains("too many loops"), "{rejected}");
    }
}
