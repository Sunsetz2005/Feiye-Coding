//! Host session manager: Sunsetz agent kernel by default.
//! Legacy `grok agent stdio` ACP is used only when `runtimeBackend=grok_acp`.
//! Mock only if SUNSETZ_ACP=mock.
//!
//! Process policy (I01–I03):
//! - One ACP process per live/parked App session (up to `maxConcurrentAgents`, default 3).
//! - Switching chats parks a Ready process instead of killing it (when under the cap).
//! - Idle processes are soft-recycled after `agentIdleMinutes` (default 30); session meta stays.
//!
//! Streaming performance (I04 / I06):
//! - Mid-stream journal upserts are throttled (≥500ms or paragraph / force).
//! - Pure stream silence past `streamStallSeconds` emits `session://stream_stall`.

mod acp_event;
mod command_jobs;
mod connect;
mod interactions;
mod live;
mod subagents;
#[cfg(test)]
mod test_support;
mod turn;
mod types;

pub use command_jobs::CommandJobSummaryV1;
pub use subagents::SubagentView;
pub use types::{
    RewindExecuteResult, RewindPointDto, SessionSendResultV2, SessionSnapshot, UiAskUserRequest,
};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::acp_client::{AcpClient, AskUserOutcome, PermissionOutcome};
use crate::interactions::InteractionSnapshotV1;

use types::{LiveSession, ParkedAgent, ProcessId};

pub struct SessionManager {
    /// Currently focused live session (UI-bound for send).
    inner: Mutex<Option<LiveSession>>,
    /// Busy sessions still receiving ACP events (streaming / permission).
    /// Keyed by app session id. Enables multi-session parallel streaming.
    background: Mutex<HashMap<String, LiveSession>>,
    /// Warm Ready agents for other App sessions (keyed by app session id).
    parked: Mutex<HashMap<String, ParkedAgent>>,
    /// Serialize connect / park / unpark so openSession prefetch cannot race first send.
    connect_lock: tokio::sync::Mutex<()>,
    /// In-process child agents for the Sunsetz kernel.
    subagents: Arc<tokio::sync::Mutex<subagents::SubagentRegistry>>,
    /// Parent-session background `run_command` jobs.
    command_jobs: Arc<tokio::sync::Mutex<command_jobs::CommandJobRegistry>>,
    /// Session ids with a Host wake turn starting or running.
    wake_inflight: Mutex<HashSet<String>>,
}

enum AskUserReplyChannel {
    Acp(Arc<AcpClient>),
    Host(tokio::sync::oneshot::Sender<AskUserOutcome>),
    Taken,
}

struct AskUserResolveTarget {
    session_id: String,
    process_id: ProcessId,
    rpc_id: u64,
    activity_id: String,
    question_count: usize,
    reply: AskUserReplyChannel,
    snapshot: InteractionSnapshotV1,
}

enum PermissionReplyChannel {
    Acp(Arc<AcpClient>),
    Host(tokio::sync::oneshot::Sender<PermissionOutcome>),
}

struct PermissionResolveTarget {
    session_id: String,
    process_id: ProcessId,
    interaction_id: String,
    rpc_id: u64,
    reply: Option<PermissionReplyChannel>,
}

struct PlanResolveTarget {
    session_id: String,
    process_id: ProcessId,
    interaction_id: String,
    rpc_id: u64,
    acp: Arc<AcpClient>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            background: Mutex::new(HashMap::new()),
            parked: Mutex::new(HashMap::new()),
            connect_lock: tokio::sync::Mutex::new(()),
            subagents: Arc::new(tokio::sync::Mutex::new(
                subagents::SubagentRegistry::default(),
            )),
            command_jobs: Arc::new(tokio::sync::Mutex::new(
                command_jobs::CommandJobRegistry::default(),
            )),
            wake_inflight: Mutex::new(HashSet::new()),
        }
    }
}
