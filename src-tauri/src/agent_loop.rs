//! Sunsetz-owned agent kernel.
//!
//! The product runtime is this in-process loop, not a spawned `grok agent stdio`
//! process. It sends the user prompt to an OpenAI-compatible chat endpoint using
//! existing providers/secrets, executes Host-owned `read_file`, `list_directory`,
//! `grep`, `write_file`, `search_replace`, and `run_command` inside a trusted
//! project root, and emits the
//! same `AcpEvent` surface the session UI already consumes.
//!
//! Writes and commands wait on the Host permission dock (or an explicit
//! auto-allow policy) before they run. Reads stay immediate inside a trusted
//! root. `run_command` isolation follows Host `sandboxProfile`: off, Linux
//! bubblewrap, macOS sandbox-exec, or Windows AppContainer.
//!
//! Project instructions are a bounded, trusted-root file attach. Memory and
//! Skill fragments are prepended by the session Host, not this loop.
//! Connected marketplace connectors inject extra tools for this turn; they do
//! not require a trusted project. Context compact (`/compact` and auto-compact)
//! is owned by `context_compact` and the session Host; this loop reports last-
//! inference occupancy. Out of scope this slice: Hermes skill runner, cron.
//! The Grok ACP adapter stays behind an explicit legacy flag.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
#[cfg(test)]
use tokio::io::AsyncWriteExt;

use crate::acp_client::{parse_ask_user_question_params, AcpEvent, StreamKind};
use crate::stream_stall::{stall_duration, DEFAULT_STREAM_STALL_SECONDS};
use crate::error::{AgentError, AgentErrorCode};
use crate::fs_browser;
use crate::process_util;
use crate::providers::{self, ActiveRoute};
use crate::runtime_compat;
use crate::store::{self, ChatMessageStored};

pub const BACKEND_SUNSETZ: &str = "sunsetz";
pub const BACKEND_GROK_ACP: &str = "grok_agent_stdio";
pub const BACKEND_MOCK: &str = "mock_acp";
pub const SETTING_LEGACY_GROK_ACP: &str = "grok_acp";
pub const OFFICIAL_OPENAI_BASE_URL: &str = "https://api.x.ai/v1";

pub const MAX_TOOL_ROUNDS: u32 = 16;
pub const IDENTICAL_TOOL_STREAK_LIMIT: u32 = 3;
pub const TOOL_BUDGET_STOP_REASON: &str = "max_tool_rounds";
pub const MAX_PROJECT_INSTRUCTION_CHARS: usize = 16_000;
pub const PROJECT_INSTRUCTION_FILES: &[&str] = &[
    "AGENTS.md",
    "Sunsetz.md",
    ".sunsetz/instructions.md",
    "CLAUDE.md",
];
const MAX_FILE_CHARS: usize = 32_768;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_GREP_MATCHES: usize = 200;
const MAX_GREP_FILES: usize = 2_000;
const MAX_GREP_FILE_BYTES: u64 = 1_048_576;
const MAX_GREP_LINE_CHARS: usize = 240;

const GREP_SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".next",
    "__pycache__",
    ".sunsetz",
    "vendor",
];

const HTTP_CONNECT_TIMEOUT_SECS: u64 = 30;
const COMMAND_TIMEOUT_SECS: u64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct HostToolPermission {
    pub tool_name: String,
    pub title: String,
    pub preview: String,
    pub path_target: String,
    pub command: String,
    pub tool_call_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostToolPermissionDecision {
    Allow,
    Deny,
    Cancelled,
}

pub type HostToolPermissionGate = Arc<
    dyn Fn(HostToolPermission) -> Pin<Box<dyn Future<Output = HostToolPermissionDecision> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone)]
pub struct HostAskUserRequest {
    pub tool_call_id: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub enum HostAskUserDecision {
    Accepted { answers: Value },
    Cancelled,
}

pub type HostAskUserGate = Arc<
    dyn Fn(HostAskUserRequest) -> Pin<Box<dyn Future<Output = HostAskUserDecision> + Send>>
        + Send
        + Sync,
>;

pub type ConnectorInvokeFn =
    Arc<dyn Fn(String, Value) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

#[derive(Clone, Default)]
pub struct ConnectorTurn {
    pub tools: Vec<Value>,
    pub write_tools: HashSet<String>,
    pub explicit: Vec<String>,
    pub invoke: Option<ConnectorInvokeFn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Parent,
    Explore,
    Plan,
    General,
}

impl Default for AgentKind {
    fn default() -> Self {
        Self::Parent
    }
}

impl AgentKind {
    pub fn parse_spawn_type(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "general" | "general-purpose" | "general_purpose" => Ok(Self::General),
            "explore" | "research" => Ok(Self::Explore),
            "plan" | "planner" => Ok(Self::Plan),
            other => Err(format!(
                "unknown agent_type `{other}` (use explore, plan, or general)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parent => "parent",
            Self::Explore => "explore",
            Self::Plan => "plan",
            Self::General => "general",
        }
    }

    pub fn max_tool_rounds(self) -> u32 {
        match self {
            Self::Explore | Self::Plan => 8,
            _ => MAX_TOOL_ROUNDS,
        }
    }

    pub fn allows_write(self) -> bool {
        matches!(self, Self::Parent | Self::General)
    }

    pub fn allows_command(self) -> bool {
        matches!(self, Self::Parent | Self::General)
    }

    pub fn allows_connectors(self) -> bool {
        matches!(self, Self::Parent | Self::General)
    }

    pub fn allows_spawn(self) -> bool {
        matches!(self, Self::Parent)
    }
}

#[derive(Debug, Clone)]
pub struct SpawnAgentRequest {
    pub prompt: String,
    pub description: String,
    pub agent_type: String,
    pub background: bool,
    pub tool_call_id: String,
}

pub type SpawnAgentFn =
    Arc<dyn Fn(SpawnAgentRequest) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;
pub type AgentLookupFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

#[derive(Clone, Default)]
pub struct SubagentHooks {
    pub spawn: Option<SpawnAgentFn>,
    pub output: Option<AgentLookupFn>,
    pub kill: Option<AgentLookupFn>,
}

#[derive(Debug, Clone)]
pub struct StartCommandJobRequest {
    pub command: String,
    pub cwd: PathBuf,
    pub project_root: PathBuf,
    pub sandbox: runtime_compat::SandboxProfileV1,
    pub tool_call_id: String,
    pub title: String,
}

pub type StartCommandJobFn = Arc<
    dyn Fn(StartCommandJobRequest) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync,
>;
pub type WaitCommandsFn = Arc<
    dyn Fn(Vec<String>, bool, u64) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync,
>;
pub type StartMonitorFn = Arc<
    dyn Fn(String, Option<String>) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync,
>;

#[derive(Clone, Default)]
pub struct CommandJobHooks {
    pub start: Option<StartCommandJobFn>,
    pub output: Option<AgentLookupFn>,
    pub wait: Option<WaitCommandsFn>,
    pub kill: Option<AgentLookupFn>,
    pub monitor: Option<StartMonitorFn>,
}

pub const SUBAGENT_SUMMARY_CHARS: usize = 8_192;
pub const MAX_RUNNING_SUBAGENTS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmEndpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Clone)]
pub struct AgentTurnConfig {
    pub endpoint: LlmEndpoint,
    pub project_root: Option<PathBuf>,
    pub trusted: bool,
    pub history: Vec<Value>,
    pub user_prompt: String,
    pub stop: Arc<AtomicBool>,
    pub client: reqwest::Client,
    pub max_tool_rounds: u32,
    /// Silence between SSE chunks before the kernel fails the completion.
    /// The Host stall prompt uses the same settings window.
    pub stream_idle: Duration,
    pub permission_gate: Option<HostToolPermissionGate>,
    pub ask_user_gate: Option<HostAskUserGate>,
    pub connectors: ConnectorTurn,
    pub reasoning_effort: Option<String>,
    pub kind: AgentKind,
    pub spawn_depth: u32,
    pub subagents: SubagentHooks,
    pub command_jobs: CommandJobHooks,
    pub sandbox_profile: runtime_compat::SandboxProfileV1,
    pub skill_prompt_chars: Arc<AtomicUsize>,
    pub allow_schedule_task: bool,
    pub allow_skill_save: bool,
}

pub fn default_runtime_backend() -> String {
    BACKEND_SUNSETZ.into()
}

pub fn parse_runtime_backend(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "grok_acp" | "grok_agent_stdio" => SETTING_LEGACY_GROK_ACP.into(),
        "mock_acp" | "mock" => BACKEND_MOCK.into(),
        _ => BACKEND_SUNSETZ.into(),
    }
}

pub fn is_sunsetz_backend(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(BACKEND_SUNSETZ)
}

pub fn is_legacy_grok_backend(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "grok_acp" | "grok_agent_stdio"
    )
}

pub const OVERRIDE_NONE: &str = "none";
pub const OVERRIDE_ENV_MOCK: &str = "sunsetz_acp";
pub const OVERRIDE_ENV_BACKEND: &str = "sunsetz_runtime_backend";

/// Saved preference vs the kernel this process actually uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendResolution {
    pub stored: String,
    pub effective: String,
    pub override_source: String,
}

/// Resolve the product kernel vs legacy ACP adapter vs in-process mock.
pub fn resolve_backend(settings_backend: &str, env_backend: Option<&str>, mock: bool) -> String {
    resolve_backend_report(settings_backend, env_backend, mock).effective
}

pub fn resolve_backend_report(
    settings_backend: &str,
    env_backend: Option<&str>,
    mock: bool,
) -> BackendResolution {
    let stored = parse_runtime_backend(settings_backend);
    let env = env_backend.map(str::trim).filter(|value| !value.is_empty());
    let (effective, override_source) = if mock {
        (BACKEND_MOCK.into(), OVERRIDE_ENV_MOCK.into())
    } else if let Some(env) = env {
        let parsed = parse_runtime_backend(env);
        let effective = if parsed == SETTING_LEGACY_GROK_ACP {
            BACKEND_GROK_ACP.into()
        } else if parsed == BACKEND_MOCK {
            BACKEND_MOCK.into()
        } else {
            BACKEND_SUNSETZ.into()
        };
        (effective, OVERRIDE_ENV_BACKEND.into())
    } else if stored == SETTING_LEGACY_GROK_ACP {
        (BACKEND_GROK_ACP.into(), OVERRIDE_NONE.into())
    } else if stored == BACKEND_MOCK {
        (BACKEND_MOCK.into(), OVERRIDE_NONE.into())
    } else {
        (BACKEND_SUNSETZ.into(), OVERRIDE_NONE.into())
    };
    BackendResolution {
        stored,
        effective,
        override_source,
    }
}

pub fn current_backend_report() -> BackendResolution {
    resolve_backend_report(
        &store::load_settings().runtime_backend,
        std::env::var(runtime_compat::PRODUCT_RUNTIME_BACKEND_ENV)
            .ok()
            .as_deref(),
        crate::acp_client::AcpClient::use_mock(),
    )
}

pub fn current_backend() -> String {
    current_backend_report().effective
}

pub fn use_sunsetz_kernel() -> bool {
    is_sunsetz_backend(&current_backend())
}

pub fn use_legacy_grok_acp() -> bool {
    is_legacy_grok_backend(&current_backend())
}

pub fn http_client() -> Result<reqwest::Client, AgentError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
        .no_proxy()
        .build()
        .map_err(|error| {
            AgentError::new(
                AgentErrorCode::NetworkProvider,
                format!("HTTP client: {error}"),
            )
        })
}

pub fn resolve_inference_credentials(composer_model: &str) -> Result<LlmEndpoint, AgentError> {
    match providers::active_route() {
        ActiveRoute::Custom { id } => {
            let list = providers::list_custom_providers()
                .map_err(|error| AgentError::new(AgentErrorCode::ConnectFailed, error))?;
            let provider = list.providers.iter().find(|p| p.id == id).ok_or_else(|| {
                AgentError::new(
                    AgentErrorCode::AuthFailed,
                    format!("Custom provider `{id}` is missing"),
                )
            })?;
            let api_key = providers::resolve_stored_key(Some(&id));
            if api_key.trim().is_empty() {
                return Err(AgentError::new(
                    AgentErrorCode::AuthFailed,
                    "Custom provider is missing an API key",
                ));
            }
            Ok(LlmEndpoint {
                base_url: provider.base_url.clone(),
                api_key,
                model: provider.model.clone(),
            })
        }
        ActiveRoute::Official => {
            let secrets = store::load_secrets();
            if let Some(api_key) = secrets
                .official_api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Ok(LlmEndpoint {
                    base_url: OFFICIAL_OPENAI_BASE_URL.into(),
                    api_key: api_key.to_string(),
                    model: official_model_id(composer_model),
                })
            } else if let (Some(base), Some(api_key)) = (
                secrets
                    .relay_base_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                secrets
                    .relay_api_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
            ) {
                let model = secrets
                    .default_model
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| official_model_id(composer_model));
                Ok(LlmEndpoint {
                    base_url: providers::normalize_openai_base_url(base, "chat_completions"),
                    api_key: api_key.to_string(),
                    model,
                })
            } else {
                Err(AgentError::new(
                    AgentErrorCode::AuthFailed,
                    "No API key configured. Add an official key or a custom provider in Settings.",
                ))
            }
        }
    }
}

fn official_model_id(composer_model: &str) -> String {
    let model = composer_model.trim();
    if model.is_empty()
        || providers::is_custom_provider_id(model)
        || model == providers::OFFICIAL_DEFAULT_MODEL
    {
        providers::OFFICIAL_CATALOG_MODEL.into()
    } else {
        model.into()
    }
}

pub fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

pub fn resolve_trusted_root(project_path: Option<&str>) -> (Option<PathBuf>, bool) {
    let Some(raw) = project_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (None, false);
    };
    let path = PathBuf::from(raw);
    if !path.is_dir() {
        return (Some(path), false);
    }
    let trusted = store::load_projects()
        .iter()
        .any(|project| project.trusted && paths_equal(&PathBuf::from(&project.path), &path));
    (Some(path), trusted)
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub fn chat_history_from_journal(messages: &[ChatMessageStored]) -> Vec<Value> {
    crate::context_compact::history_for_model(messages, None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedProjectInstruction {
    pub relative_path: String,
    pub truncated: bool,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInstructionInspectV1 {
    pub version: u8,
    pub relative_path: Option<String>,
    pub truncated: bool,
    pub character_count: usize,
}

pub fn inspect_project_instruction(
    project_root: Option<&Path>,
    trusted: bool,
) -> ProjectInstructionInspectV1 {
    match load_project_instruction(project_root, trusted) {
        Some(loaded) => ProjectInstructionInspectV1 {
            version: 1,
            relative_path: Some(loaded.relative_path),
            truncated: loaded.truncated,
            character_count: loaded.body.chars().count(),
        },
        None => ProjectInstructionInspectV1 {
            version: 1,
            relative_path: None,
            truncated: false,
            character_count: 0,
        },
    }
}

pub fn load_project_instruction(
    project_root: Option<&Path>,
    trusted: bool,
) -> Option<LoadedProjectInstruction> {
    if !trusted {
        return None;
    }
    let root = project_root?;
    for relative in PROJECT_INSTRUCTION_FILES {
        match read_project_instruction_file(root, relative) {
            Ok(Some(loaded)) => return Some(loaded),
            Ok(None) => continue,
            Err(_) => continue,
        }
    }
    None
}

fn read_project_instruction_file(
    root: &Path,
    relative: &str,
) -> Result<Option<LoadedProjectInstruction>, String> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute() {
        return Ok(None);
    }
    let mut current = root.to_path_buf();
    for component in relative_path.components() {
        current.push(component);
        let meta = match std::fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(_) => return Ok(None),
        };
        if meta.file_type().is_symlink() {
            return Ok(None);
        }
    }
    let meta = std::fs::symlink_metadata(&current)
        .map_err(|_| "project instruction is unreadable".to_string())?;
    if !meta.is_file() {
        return Ok(None);
    }
    let bytes =
        std::fs::read(&current).map_err(|_| "project instruction is unreadable".to_string())?;
    let text =
        String::from_utf8(bytes).map_err(|_| "project instruction is not UTF-8".to_string())?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let chars: Vec<char> = text.chars().collect();
    let truncated = chars.len() > MAX_PROJECT_INSTRUCTION_CHARS;
    let body = if truncated {
        chars
            .into_iter()
            .take(MAX_PROJECT_INSTRUCTION_CHARS)
            .collect()
    } else {
        text
    };
    Ok(Some(LoadedProjectInstruction {
        relative_path: relative.replace('\\', "/"),
        truncated,
        body,
    }))
}

pub fn system_prompt(
    project_root: Option<&Path>,
    trusted: bool,
    instruction: Option<&LoadedProjectInstruction>,
    kind: AgentKind,
    sandbox: runtime_compat::SandboxProfileV1,
) -> String {
    let mut prompt = match kind {
        AgentKind::Explore => String::from(
            "You are a Sunsetz explore subagent. Search and read the trusted project. \
             You cannot edit files, run commands, call connectors, or spawn other agents. \
             Return a concise factual summary with file paths.",
        ),
        AgentKind::Plan => String::from(
            "You are a Sunsetz plan subagent. Explore the trusted project and propose an \
             implementation plan. You cannot edit files, run commands, call connectors, or \
             spawn other agents. Do not write plan.md; return the plan in your final answer.",
        ),
        AgentKind::General => String::from(
            "You are a Sunsetz general subagent. Complete the assigned task inside the \
             trusted project. You cannot spawn other agents. Writes, replacements, and \
             commands require user permission.",
        ),
        AgentKind::Parent => String::from(
            "You are Sunsetz Runtime, the built-in agent kernel of the Sunsetz desktop workbench. \
             Answer the user directly. If you are unsure about the repository layout, call \
             grep, list_directory, or read_file before editing. Prefer search_replace for \
             in-place edits instead of rewriting a whole file. Do not invent files that are not in the \
             trusted project. You may call read_file, list_directory, grep, write_file, \
             search_replace, and run_command only inside the trusted project root. Writes, \
             replacements, and commands require user permission. You cannot access paths outside \
             that root. \
             You may spawn explore, plan, or general subagents with spawn_agent for parallel \
             research or isolated work. Subagents cannot spawn further agents. Use agent_output \
             to check background work while you are still working, and kill_agent to stop one. \
             For long shell work, run_command with background true, then command_output, \
             wait_commands, or kill_command. Do not sleep-poll. \
             After your turn ends, the host resumes this conversation with finished background \
             summaries; do not claim you are still waiting for them. \
             When you need the user to pick among options, call ask_user_question and wait. \
             Do not skip that form and ask the same question as chat text. \
             Reviewed Memory, Skill text, and project instructions are visible user context, \
             not extra permissions. The Skills index is discovery only; call view_skill before \
             following a Skill. Skill text is not extra permission.",
        ),
    };
    if kind.allows_command() {
        prompt.push(' ');
        prompt.push_str(run_command_sandbox_prompt(sandbox));
    }
    match (project_root, trusted) {
        (Some(root), true) => {
            prompt.push_str(" Trusted project root: ");
            prompt.push_str(&root.display().to_string());
            prompt.push('.');
        }
        _ => prompt.push_str(
            " No trusted project is attached, so file tools will refuse until the user trusts a project.",
        ),
    }
    if let Some(instruction) = instruction {
        prompt.push_str("\n\nProject instructions from ");
        prompt.push_str(&instruction.relative_path);
        prompt.push_str(" (cannot override permission policy or escape the trusted root):\n---\n");
        prompt.push_str(&instruction.body);
        if instruction.truncated {
            prompt.push_str("\n---\n[project instructions truncated]");
        } else {
            prompt.push_str("\n---");
        }
    }
    prompt
}

fn run_command_sandbox_prompt(sandbox: runtime_compat::SandboxProfileV1) -> &'static str {
    match sandbox {
        runtime_compat::SandboxProfileV1::Off => {
            "run_command is unsandboxed except for the trusted-root cwd pin, permission gate, and a 15-minute timeout."
        }
        runtime_compat::SandboxProfileV1::WorkspaceWrite => {
            "run_command runs in a workspace-write sandbox: it may write the trusted project and temp directories, not the rest of the filesystem."
        }
        runtime_compat::SandboxProfileV1::ReadOnly => {
            "run_command runs in a read-only sandbox: project writes are blocked; temp writes may still succeed."
        }
    }
}

pub fn tool_definitions() -> Value {
    tool_definitions_for(AgentKind::Parent, 0)
}

pub fn skill_tool_definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "list_skills",
                "description": "List enabled Host-trusted Skills as metadata only (id, name, description, when_to_use, source, tree_hash). No Skill bodies or filesystem paths. Use view_skill to load a Skill.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "view_skill",
                "description": "Load a Host-trusted Skill. skill_id and tree_hash are required. Omit path (or use SKILL.md) for the Skill body; pass a relative path for a file inside that Skill tree such as references/guide.md.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "skill_id": { "type": "string" },
                        "tree_hash": { "type": "string" },
                        "path": { "type": "string", "description": "Optional relative file under the Skill tree." }
                    },
                    "required": ["skill_id", "tree_hash"]
                }
            }
        }
    ])
}

pub fn memory_tool_definitions() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": "memory",
            "description": "Manage Sunsetz auto memory. Actions: add, replace, remove, list. Targets: notes (environment/project facts) or user_profile (user preferences). replace/remove match a unique old_text substring. Overflow returns an error instead of dropping entries. Do not store secrets.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string" },
                    "target": { "type": "string" },
                    "content": { "type": "string" },
                    "old_text": { "type": "string" }
                },
                "required": ["action"]
            }
        }
    }])
}

pub fn tool_definitions_for(kind: AgentKind, spawn_depth: u32) -> Value {
    let mut tools = vec![
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a UTF-8 text file inside the trusted project root. Path is relative to that root.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative path under the trusted project root." }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "List files and directories inside the trusted project root. Path is relative; omit or use . for the root.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative directory under the trusted project root." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "grep",
                "description": "Search UTF-8 files inside the trusted project root with a regular expression. Path is a relative file or directory; omit or use . for the root. Skips .git, node_modules, target, and other build directories. Read-only; no permission prompt.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Regular expression to search for." },
                        "path": { "type": "string", "description": "Relative file or directory under the trusted project root." },
                        "case_insensitive": { "type": "boolean", "description": "Match without regard to case." }
                    },
                    "required": ["pattern"]
                }
            }
        }),
    ];
    if kind.allows_write() {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Create or overwrite a UTF-8 text file inside the trusted project root. Path is relative to that root. Requires permission.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative file path under the trusted project root." },
                        "content": { "type": "string", "description": "UTF-8 text to write." }
                    },
                    "required": ["path", "content"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "search_replace",
                "description": "Replace exact text in an existing UTF-8 file inside the trusted project root. Path is relative. Requires permission; AcceptEdits may auto-allow in-root replacements. Prefer this over rewriting the whole file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative file path under the trusted project root." },
                        "old_string": { "type": "string", "description": "Exact text to find." },
                        "new_string": { "type": "string", "description": "Replacement text." },
                        "replace_all": { "type": "boolean", "description": "Replace every occurrence. If false, old_string must match exactly once." }
                    },
                    "required": ["path", "old_string", "new_string"]
                }
            }
        }));
    }
    if kind.allows_command() {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Run a shell command with cwd pinned to a directory inside the trusted project root. Isolation follows the Host sandbox profile (off, workspace_write, or read_only). Requires permission; AcceptEdits does not auto-allow this tool. 15-minute timeout.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Exact command string to run." },
                        "cwd": { "type": "string", "description": "Optional relative directory under the trusted project root. Defaults to the root." },
                        "background": { "type": "boolean", "description": "If true, start the command and return an id immediately. Parent session only." }
                    },
                    "required": ["command"]
                }
            }
        }));
    }
    if kind == AgentKind::Parent && spawn_depth == 0 {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "ask_user_question",
                "description": "Ask the user a multiple-choice or free-text question in the Sunsetz workbench form. Use this instead of writing the question into chat when you need a choice. Wait for the form result.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "questions": {
                            "type": "array",
                            "description": "One or more questions to show in the form.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "header": { "type": "string", "description": "Short category label; not the question text." },
                                    "question": { "type": "string" },
                                    "options": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "id": { "type": "string" },
                                                "label": { "type": "string" },
                                                "description": { "type": "string" }
                                            }
                                        }
                                    },
                                    "multiSelect": { "type": "boolean" }
                                },
                                "required": ["question"]
                            }
                        },
                        "question": { "type": "string", "description": "Single-question form." },
                        "options": { "type": "array" },
                        "multiSelect": { "type": "boolean" }
                    }
                }
            }
        }));
    }
    if kind.allows_spawn() && spawn_depth == 0 {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "spawn_agent",
                "description": "Start a child agent with its own context. Types: explore (read-only research), plan (read-only plan), general (full tools). Set background true to return an id immediately.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "Full task prompt for the child." },
                        "description": { "type": "string", "description": "Short 3-5 word label." },
                        "agent_type": { "type": "string", "description": "explore, plan, or general." },
                        "background": { "type": "boolean", "description": "If true, return an id immediately." }
                    },
                    "required": ["prompt", "description"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "agent_output",
                "description": "Get status or the final summary of a child agent by id.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Child agent id returned by spawn_agent." }
                    },
                    "required": ["id"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "kill_agent",
                "description": "Stop a running child agent by id.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Child agent id." }
                    },
                    "required": ["id"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "command_output",
                "description": "Get status and bounded output of a background command by id. Optional timeout_ms waits for completion.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Command id returned by run_command with background true." },
                        "timeout_ms": { "type": "integer", "description": "Wait up to this many milliseconds for the command to finish." }
                    },
                    "required": ["id"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "wait_commands",
                "description": "Wait for background commands. ids is required. mode is wait_all (default) or wait_any. Default timeout_ms is 30000. Do not sleep-poll.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "ids": { "type": "array", "items": { "type": "string" } },
                        "mode": { "type": "string" },
                        "timeout_ms": { "type": "integer" }
                    },
                    "required": ["ids"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "kill_command",
                "description": "Stop a background command by id.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Command id." }
                    },
                    "required": ["id"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "monitor",
                "description": "Subscribe to a running background command's output. Wakes this session (no reply needed now) when a new line arrives matching the optional regex pattern, or any new line if pattern is omitted. Detaches automatically after a bounded number of wakes or when the command ends. Not for polling; do not call repeatedly.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Command id returned by run_command with background true." },
                        "pattern": { "type": "string", "description": "Optional regular expression; only matching lines trigger a wake." }
                    },
                    "required": ["id"]
                }
            }
        }));
    }
    Value::Array(tools)
}

fn tool_path_arg(arguments: &Value) -> String {
    arguments
        .get("path")
        .or_else(|| arguments.get("relative_path"))
        .or_else(|| arguments.get("relativePath"))
        .and_then(Value::as_str)
        .unwrap_or(".")
        .trim()
        .to_string()
}

fn tool_write_path_arg(arguments: &Value) -> String {
    arguments
        .get("path")
        .or_else(|| arguments.get("relative_path"))
        .or_else(|| arguments.get("relativePath"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn tool_content_arg(arguments: &Value) -> Result<String, String> {
    match arguments.get("content") {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err("write_file requires UTF-8 `content`".into()),
        None => Err("write_file requires UTF-8 `content`".into()),
    }
}

fn tool_string_field(arguments: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(Value::String(text)) = arguments.get(*key) {
            return Some(text.clone());
        }
    }
    None
}

fn tool_bool_field(arguments: &Value, keys: &[&str]) -> bool {
    for key in keys {
        match arguments.get(*key) {
            Some(Value::Bool(value)) => return *value,
            Some(Value::String(text)) => {
                let lower = text.trim().to_ascii_lowercase();
                if lower == "true" || lower == "1" || lower == "yes" {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn tool_pattern_arg(arguments: &Value) -> String {
    tool_string_field(arguments, &["pattern", "query"])
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn tool_old_string_arg(arguments: &Value) -> Result<String, String> {
    tool_string_field(
        arguments,
        &["old_string", "oldString", "old_str", "previous"],
    )
    .ok_or_else(|| "search_replace requires `old_string`".into())
}

fn tool_new_string_arg(arguments: &Value) -> Result<String, String> {
    tool_string_field(arguments, &["new_string", "newString", "new_str"])
        .ok_or_else(|| "search_replace requires `new_string`".into())
}

fn is_host_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "list_directory"
            | "grep"
            | "write_file"
            | "search_replace"
            | "run_command"
            | "spawn_agent"
            | "agent_output"
            | "kill_agent"
            | "command_output"
            | "wait_commands"
            | "kill_command"
            | "monitor"
            | "ask_user_question"
            | "list_skills"
            | "view_skill"
            | "memory"
            | "schedule_task"
            | "skill_save"
    )
}

fn is_spawn_tool(name: &str) -> bool {
    matches!(name, "spawn_agent" | "agent_output" | "kill_agent")
}

fn is_command_job_tool(name: &str) -> bool {
    matches!(
        name,
        "command_output" | "wait_commands" | "kill_command" | "monitor"
    )
}

fn tool_timeout_ms(arguments: &Value) -> Option<u64> {
    arguments
        .get("timeout_ms")
        .or_else(|| arguments.get("timeoutMs"))
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
        })
        .filter(|ms| *ms > 0)
}

fn tool_id_list(arguments: &Value) -> Vec<String> {
    if let Some(ids) = arguments.get("ids").and_then(Value::as_array) {
        return ids
            .iter()
            .filter_map(|value| value.as_str())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
    }
    let id = tool_agent_id_arg(arguments);
    if id.is_empty() {
        Vec::new()
    } else {
        vec![id]
    }
}

fn tool_agent_id_arg(arguments: &Value) -> String {
    tool_string_field(arguments, &["id", "agent_id", "agentId"])
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn parse_spawn_request(arguments: &Value) -> Result<SpawnAgentRequest, String> {
    let prompt = tool_string_field(arguments, &["prompt", "task"])
        .unwrap_or_default()
        .trim()
        .to_string();
    if prompt.is_empty() {
        return Err("spawn_agent requires `prompt`".into());
    }
    let description = tool_string_field(arguments, &["description", "label"])
        .unwrap_or_else(|| "subagent".into())
        .trim()
        .chars()
        .take(80)
        .collect();
    let agent_type = tool_string_field(arguments, &["agent_type", "agentType", "subagent_type"])
        .unwrap_or_else(|| "general".into());
    let kind = AgentKind::parse_spawn_type(&agent_type)?;
    Ok(SpawnAgentRequest {
        prompt,
        description,
        agent_type: kind.as_str().to_string(),
        background: tool_bool_field(arguments, &["background", "run_in_background"]),
        tool_call_id: String::new(),
    })
}

fn tool_command_arg(arguments: &Value) -> String {
    arguments
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn tool_cwd_arg(arguments: &Value) -> String {
    arguments
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .trim()
        .to_string()
}

fn display_rel(relative: &str) -> String {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        ".".into()
    } else {
        trimmed.trim_start_matches("./").to_string()
    }
}

fn is_absolute_input(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    Path::new(trimmed).is_absolute() || trimmed.starts_with('/') || trimmed.starts_with('\\')
}

fn ensure_within_trusted_root(root: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let root_canon = root
        .canonicalize()
        .map_err(|error| format!("trusted project root is not accessible: {error}"))?;
    if candidate.exists() {
        let canon = candidate
            .canonicalize()
            .map_err(|error| format!("path is not accessible: {error}"))?;
        if !canon.starts_with(&root_canon) {
            return Err("path escapes trusted project root".into());
        }
        return Ok(canon);
    }
    if !candidate.starts_with(root) && !candidate.starts_with(&root_canon) {
        return Err("path escapes trusted project root".into());
    }
    Ok(candidate.to_path_buf())
}

pub fn execute_read_file(root: &Path, relative: &str) -> Result<String, String> {
    let joined = fs_browser::lexical_join(root, relative)?;
    let path = ensure_within_trusted_root(root, &joined)?;
    if !path.is_file() {
        return Err(format!("not a file: {}", relative.trim()));
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("read_file: {error}"))?;
    if bytes.contains(&0) {
        return Err(format!("binary file ({} bytes); not shown", bytes.len()));
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if text.chars().count() > MAX_FILE_CHARS {
        text = text.chars().take(MAX_FILE_CHARS).collect();
        text.push_str("\n… truncated");
    }
    Ok(text)
}

pub fn execute_list_directory(root: &Path, relative: &str) -> Result<String, String> {
    let joined = fs_browser::lexical_join(root, relative)?;
    let path = ensure_within_trusted_root(root, &joined)?;
    if !path.is_dir() {
        return Err(format!("not a directory: {}", relative.trim()));
    }
    let root_str = root.to_string_lossy().into_owned();
    let mut entries = fs_browser::list_dir(&root_str, relative)?;
    entries.truncate(MAX_LIST_ENTRIES);
    serde_json::to_string_pretty(&entries).map_err(|error| error.to_string())
}

fn skip_grep_dir(name: &str) -> bool {
    GREP_SKIP_DIRS.iter().any(|skip| *skip == name)
}

fn grep_relative(root: &Path, file: &Path) -> String {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    file.strip_prefix(root)
        .unwrap_or(&file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn grep_one_file(
    root: &Path,
    file: &Path,
    regex: &regex::Regex,
    matches: &mut Vec<String>,
    truncated: &mut bool,
) {
    if matches.len() >= MAX_GREP_MATCHES {
        *truncated = true;
        return;
    }
    let Ok(meta) = std::fs::symlink_metadata(file) else {
        return;
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
        return;
    }
    if meta.len() > MAX_GREP_FILE_BYTES {
        return;
    }
    let Ok(bytes) = std::fs::read(file) else {
        return;
    };
    if bytes.contains(&0) {
        return;
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return;
    };
    let rel = grep_relative(root, file);
    for (index, line) in text.lines().enumerate() {
        if !regex.is_match(line) {
            continue;
        }
        let mut shown: String = line.chars().take(MAX_GREP_LINE_CHARS).collect();
        if line.chars().count() > MAX_GREP_LINE_CHARS {
            shown.push('…');
        }
        matches.push(format!("{rel}:{}:{shown}", index + 1));
        if matches.len() >= MAX_GREP_MATCHES {
            *truncated = true;
            return;
        }
    }
}

fn grep_walk_dir(
    root: &Path,
    dir: &Path,
    regex: &regex::Regex,
    matches: &mut Vec<String>,
    truncated: &mut bool,
    files_scanned: &mut usize,
) {
    if matches.len() >= MAX_GREP_MATCHES || *files_scanned >= MAX_GREP_FILES {
        *truncated = true;
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        if matches.len() >= MAX_GREP_MATCHES || *files_scanned >= MAX_GREP_FILES {
            *truncated = true;
            break;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let name = entry.file_name();
            if skip_grep_dir(&name.to_string_lossy()) {
                continue;
            }
            dirs.push(path);
            continue;
        }
        if meta.is_file() {
            *files_scanned += 1;
            grep_one_file(root, &path, regex, matches, truncated);
        }
    }
    for child in dirs {
        if matches.len() >= MAX_GREP_MATCHES || *files_scanned >= MAX_GREP_FILES {
            *truncated = true;
            break;
        }
        grep_walk_dir(root, &child, regex, matches, truncated, files_scanned);
    }
}

pub fn execute_grep(root: &Path, arguments: &Value) -> Result<String, String> {
    let pattern = tool_pattern_arg(arguments);
    if pattern.is_empty() {
        return Err("empty grep pattern".into());
    }
    if pattern.chars().count() > 512 {
        return Err("grep pattern is too long".into());
    }
    let case_insensitive =
        tool_bool_field(arguments, &["case_insensitive", "caseInsensitive", "i"]);
    let regex = regex::RegexBuilder::new(&pattern)
        .case_insensitive(case_insensitive)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|error| format!("invalid grep pattern: {error}"))?;
    let relative = tool_path_arg(arguments);
    let joined = fs_browser::lexical_join(root, &relative)?;
    let path = ensure_within_trusted_root(root, &joined)?;
    let root_canon = root
        .canonicalize()
        .map_err(|error| format!("trusted project root is not accessible: {error}"))?;
    let mut matches = Vec::new();
    let mut truncated = false;
    let mut files_scanned = 0usize;
    if path.is_file() {
        grep_one_file(&root_canon, &path, &regex, &mut matches, &mut truncated);
    } else if path.is_dir() {
        grep_walk_dir(
            &root_canon,
            &path,
            &regex,
            &mut matches,
            &mut truncated,
            &mut files_scanned,
        );
    } else {
        return Err(format!("not a file or directory: {}", relative.trim()));
    }
    if matches.is_empty() {
        return Ok("no matches".into());
    }
    let mut text = matches.join("\n");
    if truncated {
        text.push_str("\n… truncated");
    }
    if text.chars().count() > MAX_FILE_CHARS {
        text = text.chars().take(MAX_FILE_CHARS).collect();
        text.push_str("\n… truncated");
    }
    Ok(text)
}

/// Resolve a project-relative write destination without following a symlink out
/// of the trusted root. Missing parents are reconstructed from the nearest
/// existing ancestor after that ancestor canonicalizes inside the root.
fn resolve_in_root_dest(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.contains('\0') {
        return Err("invalid path".into());
    }
    let rel = relative.trim();
    if rel.is_empty() || rel == "." {
        return Err("empty relative path".into());
    }
    if is_absolute_input(rel) {
        return Err("absolute path not allowed".into());
    }
    let joined = fs_browser::lexical_join(root, rel)?;
    let root_canon = root
        .canonicalize()
        .map_err(|error| format!("trusted project root is not accessible: {error}"))?;
    let Some(file_name) = joined.file_name() else {
        return Err("empty relative path".into());
    };
    let parent = joined.parent().unwrap_or(root);
    let mut ancestor = parent.to_path_buf();
    let mut missing: Vec<OsString> = Vec::new();
    while !ancestor.exists() {
        match ancestor.file_name() {
            Some(name) => missing.push(name.to_os_string()),
            None => return Err("path escapes trusted project root".into()),
        }
        ancestor = ancestor
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "path escapes trusted project root".to_string())?;
        if missing.len() > 64 {
            return Err("path escapes trusted project root".into());
        }
    }
    let mut parent_canon = ancestor
        .canonicalize()
        .map_err(|error| format!("path is not accessible: {error}"))?;
    if !parent_canon.starts_with(&root_canon) {
        return Err("path escapes trusted project root".into());
    }
    for name in missing.into_iter().rev() {
        parent_canon.push(name);
        if !parent_canon.starts_with(&root_canon) {
            return Err("path escapes trusted project root".into());
        }
    }
    let dest = parent_canon.join(file_name);
    if !dest.starts_with(&root_canon) {
        return Err("path escapes trusted project root".into());
    }
    if dest.exists() {
        let meta = dest
            .symlink_metadata()
            .map_err(|error| format!("path is not accessible: {error}"))?;
        if meta.is_dir() {
            return Err(format!("not a file: {}", rel));
        }
        if meta.file_type().is_symlink() {
            match dest.canonicalize() {
                Ok(target) if target.starts_with(&root_canon) => {}
                _ => return Err("path escapes trusted project root".into()),
            }
        }
    }
    Ok(dest)
}

fn atomic_write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "invalid parent directory".to_string())?;
    let tmp_name = format!(
        ".{}.sunsetz-write-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("file"),
        std::process::id()
    );
    let tmp = parent.join(tmp_name);
    std::fs::write(&tmp, bytes).map_err(|error| format!("write temp: {error}"))?;
    std::fs::rename(&tmp, path).map_err(|error| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename into place: {error}")
    })?;
    Ok(())
}

#[derive(Debug)]
struct PreparedWrite {
    rel: String,
    content: String,
    preview: String,
    path_target: String,
    title: String,
}

fn prepare_write_file(root: &Path, arguments: &Value) -> Result<PreparedWrite, String> {
    let rel = tool_write_path_arg(arguments);
    let content = tool_content_arg(arguments)?;
    if rel.contains('\0') {
        return Err("invalid path".into());
    }
    if content.contains('\0') {
        return Err("invalid content".into());
    }
    if content.as_bytes().len() as u64 > fs_browser::MAX_TEXT_BYTES {
        return Err(format!(
            "file too large to write (max {} bytes)",
            fs_browser::MAX_TEXT_BYTES
        ));
    }
    let dest = resolve_in_root_dest(root, &rel)?;
    if dest.is_dir() {
        return Err(format!("not a file: {}", display_rel(&rel)));
    }
    let shown = display_rel(&rel);
    Ok(PreparedWrite {
        preview: format!("{} ({} bytes)", shown, content.as_bytes().len()),
        path_target: dest.to_string_lossy().replace('\\', "/"),
        title: format!("Write {shown}"),
        rel: shown,
        content,
    })
}

pub fn execute_write_file(root: &Path, relative: &str, content: &str) -> Result<String, String> {
    if content.contains('\0') {
        return Err("invalid content".into());
    }
    let bytes = content.as_bytes();
    if bytes.len() as u64 > fs_browser::MAX_TEXT_BYTES {
        return Err(format!(
            "file too large to write (max {} bytes)",
            fs_browser::MAX_TEXT_BYTES
        ));
    }
    let dest = resolve_in_root_dest(root, relative)?;
    if dest.is_dir() {
        return Err(format!("not a file: {}", display_rel(relative)));
    }
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|error| format!("create parent: {error}"))?;
            let parent_canon = parent
                .canonicalize()
                .map_err(|error| format!("path is not accessible: {error}"))?;
            let root_canon = root
                .canonicalize()
                .map_err(|error| format!("trusted project root is not accessible: {error}"))?;
            if !parent_canon.starts_with(&root_canon) {
                return Err("path escapes trusted project root".into());
            }
        }
    }
    atomic_write_file(&dest, bytes)?;
    Ok(format!(
        "wrote {} ({} bytes)",
        display_rel(relative),
        bytes.len()
    ))
}

fn count_nonoverlapping(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

fn prepare_search_replace(root: &Path, arguments: &Value) -> Result<PreparedWrite, String> {
    let rel = tool_write_path_arg(arguments);
    let old = tool_old_string_arg(arguments)?;
    let new = tool_new_string_arg(arguments)?;
    if rel.contains('\0') || old.contains('\0') || new.contains('\0') {
        return Err("invalid content".into());
    }
    if old.is_empty() {
        return Err("search_replace requires a non-empty `old_string`".into());
    }
    let dest = resolve_in_root_dest(root, &rel)?;
    if !dest.exists() {
        return Err(format!("not a file: {}", display_rel(&rel)));
    }
    let meta = dest
        .symlink_metadata()
        .map_err(|error| format!("path is not accessible: {error}"))?;
    if meta.is_dir() {
        return Err(format!("not a file: {}", display_rel(&rel)));
    }
    if meta.file_type().is_symlink() {
        let target = dest
            .canonicalize()
            .map_err(|_| "path escapes trusted project root".to_string())?;
        let root_canon = root
            .canonicalize()
            .map_err(|error| format!("trusted project root is not accessible: {error}"))?;
        if !target.starts_with(&root_canon) {
            return Err("path escapes trusted project root".into());
        }
    }
    let bytes = std::fs::read(&dest).map_err(|error| format!("search_replace: {error}"))?;
    if bytes.contains(&0) {
        return Err(format!("binary file ({} bytes); not shown", bytes.len()));
    }
    let text = String::from_utf8(bytes).map_err(|_| "file is not UTF-8".to_string())?;
    let count = count_nonoverlapping(&text, &old);
    if count == 0 {
        return Err(format!("`old_string` not found in {}", display_rel(&rel)));
    }
    let replace_all = tool_bool_field(arguments, &["replace_all", "replaceAll"]);
    if count > 1 && !replace_all {
        return Err(format!(
            "`old_string` found {count} times in {}; pass replace_all true or a unique string",
            display_rel(&rel)
        ));
    }
    let content = if replace_all {
        text.replace(&old, &new)
    } else {
        text.replacen(&old, &new, 1)
    };
    if content.as_bytes().len() as u64 > fs_browser::MAX_TEXT_BYTES {
        return Err(format!(
            "file too large to write (max {} bytes)",
            fs_browser::MAX_TEXT_BYTES
        ));
    }
    let shown = display_rel(&rel);
    let replacements = if replace_all { count } else { 1 };
    Ok(PreparedWrite {
        preview: format!("{shown} ({replacements} replacement(s))"),
        path_target: dest.to_string_lossy().replace('\\', "/"),
        title: format!("Replace {shown}"),
        rel: shown,
        content,
    })
}

pub fn execute_search_replace(root: &Path, arguments: &Value) -> Result<String, String> {
    let prepared = prepare_search_replace(root, arguments)?;
    execute_write_file(root, &prepared.rel, &prepared.content)?;
    Ok(format!("updated {}", prepared.rel))
}

#[derive(Debug)]
struct PreparedCommand {
    command: String,
    #[allow(dead_code)]
    cwd_rel: String,
    cwd_canon: PathBuf,
    preview: String,
    title: String,
}

fn prepare_run_command(root: &Path, arguments: &Value) -> Result<PreparedCommand, String> {
    let command = tool_command_arg(arguments);
    if command.is_empty() {
        return Err("empty command".into());
    }
    if command.contains('\0') {
        return Err("invalid command".into());
    }
    let cwd_rel = tool_cwd_arg(arguments);
    if cwd_rel.contains('\0') {
        return Err("invalid cwd".into());
    }
    if is_absolute_input(&cwd_rel) {
        return Err("absolute path not allowed".into());
    }
    let cwd_joined = fs_browser::lexical_join(root, &cwd_rel)?;
    let cwd = ensure_within_trusted_root(root, &cwd_joined)?;
    if !cwd.is_dir() {
        return Err(format!("not a directory: {}", display_rel(&cwd_rel)));
    }
    let cwd_canon = cwd
        .canonicalize()
        .map_err(|error| format!("path is not accessible: {error}"))?;
    let root_canon = root
        .canonicalize()
        .map_err(|error| format!("trusted project root is not accessible: {error}"))?;
    if !cwd_canon.starts_with(&root_canon) {
        return Err("path escapes trusted project root".into());
    }
    let cwd_show = display_rel(&cwd_rel);
    let title_cmd = if command.chars().count() > 96 {
        format!("{}…", command.chars().take(96).collect::<String>())
    } else {
        command.clone()
    };
    Ok(PreparedCommand {
        preview: format!("{command} (cwd: {cwd_show})"),
        title: format!("Run {title_cmd}"),
        command,
        cwd_rel: cwd_show,
        cwd_canon,
    })
}

fn bound_command_output(mut text: String) -> String {
    if text.chars().count() > MAX_FILE_CHARS {
        text = text.chars().take(MAX_FILE_CHARS).collect();
        text.push_str("\n… truncated");
    }
    text
}

fn kill_run_command_child(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let _ = pid;
    }
    let _ = child.start_kill();
}

#[derive(Debug)]
pub(crate) enum RunCommandOutcome {
    Output(String),
    Cancelled,
}

/// Fired once per completed line (from either stdout or stderr, in arrival
/// order) while a command is still running — the mechanism `monitor` uses to
/// wake on new/matching output without waiting for the process to exit.
/// Async and side-effect-only so it can take the same command-jobs registry
/// lock the terminal-status path already uses.
pub type CommandLineFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Drains an async byte stream, decoding complete lines lossily and handing
/// each to `on_line` as it arrives, while still returning the raw bytes read
/// so the caller's final combined-output text is unaffected by the per-line
/// lossy decode. Reading only at EOF (the prior behavior) would block the
/// stream shut for the duration of a long-running command, which is exactly
/// what `monitor` needs to observe.
async fn stream_command_output<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    on_line: Option<CommandLineFn>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut line_start = 0usize;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(sink) = &on_line {
                    while let Some(pos) = buf[line_start..].iter().position(|&b| b == b'\n') {
                        let end = line_start + pos;
                        let line = String::from_utf8_lossy(&buf[line_start..end]).into_owned();
                        line_start = end + 1;
                        sink(line).await;
                    }
                }
            }
        }
    }
    if let Some(sink) = &on_line {
        if line_start < buf.len() {
            let line = String::from_utf8_lossy(&buf[line_start..]).into_owned();
            sink(line).await;
        }
    }
    buf
}

async fn execute_run_command(
    command: &str,
    cwd: &Path,
    stop: Arc<AtomicBool>,
    profile: runtime_compat::SandboxProfileV1,
    project_root: &Path,
) -> Result<RunCommandOutcome, String> {
    execute_run_command_timed(
        command,
        cwd,
        stop,
        Duration::from_secs(COMMAND_TIMEOUT_SECS),
        profile,
        project_root,
        None,
    )
    .await
}

pub(crate) async fn execute_run_command_timed(
    command: &str,
    cwd: &Path,
    stop: Arc<AtomicBool>,
    timeout: Duration,
    profile: runtime_compat::SandboxProfileV1,
    project_root: &Path,
    on_line: Option<CommandLineFn>,
) -> Result<RunCommandOutcome, String> {
    let plan = crate::command_sandbox::plan_run_command(command, cwd, project_root, profile)?;
    #[cfg(windows)]
    if plan.application.applied != "off" {
        let isolated = crate::command_sandbox::run_windows_isolated(
            plan,
            Arc::clone(&stop),
            timeout,
        )
        .await?;
        let (out, err, code, cancelled) = isolated;
        if cancelled {
            return Ok(RunCommandOutcome::Cancelled);
        }
        let success = code == 0;
        let mut text = String::from_utf8_lossy(&out).into_owned();
        if !err.is_empty() {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&String::from_utf8_lossy(&err));
        }
        if text.is_empty() {
            text = if success {
                String::new()
            } else {
                format!("exit {code}")
            };
        } else if !success {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("exit {code}"));
        }
        return Ok(RunCommandOutcome::Output(bound_command_output(text)));
    }
    let mut cmd = tokio::process::Command::new(&plan.executable);
    cmd.args(&plan.args);
    if cfg!(windows) {
        process_util::apply_no_window_tokio(&mut cmd);
    }
    cmd.current_dir(&plan.current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|error| format!("run_command: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "run_command: missing stdout".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "run_command: missing stderr".to_string())?;
    let read_out = stream_command_output(&mut stdout, on_line.clone());
    let read_err = stream_command_output(&mut stderr, on_line.clone());
    tokio::select! {
        _ = wait_until_stopped(Arc::clone(&stop)) => {
            kill_run_command_child(&mut child);
            let _ = child.wait().await;
            Ok(RunCommandOutcome::Cancelled)
        }
        _ = tokio::time::sleep(timeout) => {
            kill_run_command_child(&mut child);
            let _ = child.wait().await;
            Err(format!(
                "command timed out after {}s",
                timeout.as_secs().max(1)
            ))
        }
        joined = async {
            // Drain both pipes concurrently with waiting for exit (not after,
            // as a strict read-then-wait would), both so a chatty command
            // can't deadlock on a full pipe buffer and so `on_line` actually
            // observes lines while the process is still running.
            let (status, out, err) = tokio::join!(child.wait(), read_out, read_err);
            (status, out, err)
        } => {
            let (status, out, err) = joined;
            let status = status.map_err(|error| format!("run_command: {error}"))?;
            let mut text = String::from_utf8_lossy(&out).into_owned();
            if !err.is_empty() {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&String::from_utf8_lossy(&err));
            }
            if text.is_empty() {
                text = if status.success() {
                    String::new()
                } else {
                    format!("exit {}", status.code().unwrap_or(-1))
                };
            } else if !status.success() {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&format!("exit {}", status.code().unwrap_or(-1)));
            }
            Ok(RunCommandOutcome::Output(bound_command_output(text)))
        }
    }
}

pub fn execute_tool(root: Option<&Path>, trusted: bool, name: &str, arguments: &Value) -> String {
    if !is_host_tool(name) {
        return format!("unknown tool `{name}`");
    }
    if !trusted {
        return "no trusted project root".into();
    }
    let Some(root) = root else {
        return "no trusted project root".into();
    };
    match name {
        "read_file" => {
            execute_read_file(root, &tool_path_arg(arguments)).unwrap_or_else(|error| error)
        }
        "list_directory" => {
            execute_list_directory(root, &tool_path_arg(arguments)).unwrap_or_else(|error| error)
        }
        "grep" => execute_grep(root, arguments).unwrap_or_else(|error| error),
        "spawn_agent" | "agent_output" | "kill_agent" | "ask_user_question" => {
            format!("`{name}` requires the async Host turn")
        }
        "write_file" => match tool_content_arg(arguments) {
            Ok(content) => execute_write_file(root, &tool_write_path_arg(arguments), &content)
                .unwrap_or_else(|error| error),
            Err(error) => error,
        },
        "search_replace" => execute_search_replace(root, arguments).unwrap_or_else(|error| error),
        "run_command" => "run_command requires the async Host turn".into(),
        _ => format!("unknown tool `{name}`"),
    }
}

fn tool_fingerprint(name: &str, arguments: &Value) -> String {
    format!("{name}\n{arguments}")
}

fn identical_tool_loop_message(name: &str, streak: u32) -> String {
    format!(
        "identical tool loop interrupted: `{name}` repeated {streak} times with the same arguments. Change the arguments or stop."
    )
}

fn remaining_model_rounds(round: u32, max: u32) -> u32 {
    max.saturating_sub(round.saturating_add(1))
}

fn last_tool_round_warning() -> &'static str {
    "Sunsetz Runtime: 1 tool round remains. Finish or summarize remaining work; do not start a long new investigation."
}

fn tool_budget_exhausted_message(max: u32) -> String {
    format!(
        "Sunsetz Runtime paused this turn after {max} tool rounds. Send again to continue."
    )
}

struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn apply_tool_delta(calls: &mut BTreeMap<u32, PendingToolCall>, delta: &Value) {
    let Some(items) = delta.get("tool_calls").and_then(Value::as_array) else {
        return;
    };
    for item in items {
        let index = item.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
        let entry = calls.entry(index).or_insert_with(|| PendingToolCall {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                entry.id = id.to_string();
            }
        }
        if let Some(name) = item
            .pointer("/function/name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            entry.name = name.to_string();
        }
        if let Some(args) = item.pointer("/function/arguments").and_then(Value::as_str) {
            entry.arguments.push_str(args);
        }
    }
}

fn parse_tool_arguments(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return json!({});
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| json!({ "path": trimmed }))
}

async fn wait_until_stopped(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn needs_permission(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "search_replace" | "run_command" | "skill_save"
    ) || crate::permission::is_connector_tool(name)
}

fn connector_permission_preview(arguments: &Value) -> String {
    let compact = arguments.to_string();
    let mut chars = compact.chars();
    let head: String = chars.by_ref().take(240).collect();
    if chars.next().is_none() {
        compact
    } else {
        format!("{head}…")
    }
}

fn is_configured_connector_tool(cfg: &AgentTurnConfig, name: &str) -> bool {
    crate::permission::is_connector_tool(name)
        || cfg
            .connectors
            .tools
            .iter()
            .any(|tool| tool.pointer("/function/name").and_then(Value::as_str) == Some(name))
}

enum HostToolDispatch {
    Output(String),
    Cancelled,
}

async fn request_tool_permission(
    cfg: &AgentTurnConfig,
    req: HostToolPermission,
) -> HostToolPermissionDecision {
    let Some(gate) = cfg.permission_gate.clone() else {
        return HostToolPermissionDecision::Deny;
    };
    tokio::select! {
        _ = wait_until_stopped(Arc::clone(&cfg.stop)) => HostToolPermissionDecision::Cancelled,
        decision = gate(req) => decision,
    }
}

fn format_ask_user_answers(answers: &Value) -> String {
    let pretty = serde_json::to_string_pretty(answers).unwrap_or_else(|_| answers.to_string());
    format!("The user answered:\n{pretty}")
}

async fn request_ask_user(cfg: &AgentTurnConfig, req: HostAskUserRequest) -> HostAskUserDecision {
    let Some(gate) = cfg.ask_user_gate.clone() else {
        return HostAskUserDecision::Cancelled;
    };
    tokio::select! {
        _ = wait_until_stopped(Arc::clone(&cfg.stop)) => HostAskUserDecision::Cancelled,
        decision = gate(req) => decision,
    }
}

async fn dispatch_host_tool(
    cfg: &AgentTurnConfig,
    name: &str,
    arguments: &Value,
    tool_call_id: &str,
    prepared_write: Option<Result<PreparedWrite, String>>,
    prepared_command: Option<Result<PreparedCommand, String>>,
) -> HostToolDispatch {
    if name == "schedule_task" {
        if cfg.kind != AgentKind::Parent || cfg.spawn_depth != 0 {
            return HostToolDispatch::Output(
                "schedule_task is only available on the parent agent".into(),
            );
        }
        return HostToolDispatch::Output(crate::automation_scheduler::apply_schedule_tool(
            arguments,
            cfg.allow_schedule_task,
        ));
    }
    if name == "skill_save" {
        if cfg.kind != AgentKind::Parent || cfg.spawn_depth != 0 {
            return HostToolDispatch::Output("skill_save is only available on the parent agent".into());
        }
        if !cfg.allow_skill_save {
            return HostToolDispatch::Output("skill_save is disabled during scheduled runs".into());
        }
        let project = if cfg.trusted {
            cfg.project_root.as_deref()
        } else {
            None
        };
        let preview = match crate::skill_draft::permission_preview_from_tool(arguments, project) {
            Ok(preview) => preview,
            Err(error) => return HostToolDispatch::Output(error),
        };
        let decision = request_tool_permission(
            cfg,
            HostToolPermission {
                tool_name: name.into(),
                title: preview.title.clone(),
                preview: preview.preview.clone(),
                path_target: preview.path_target.clone(),
                command: String::new(),
                tool_call_id: tool_call_id.into(),
            },
        )
        .await;
        match decision {
            HostToolPermissionDecision::Allow => {}
            HostToolPermissionDecision::Deny => {
                return HostToolDispatch::Output("permission denied by user".into());
            }
            HostToolPermissionDecision::Cancelled => {
                return HostToolDispatch::Cancelled;
            }
        }
        return HostToolDispatch::Output(
            crate::skill_draft::save_from_agent_tool(arguments, project)
                .unwrap_or_else(|error| error),
        );
    }
    if name == "memory" {
        if cfg.kind != AgentKind::Parent || cfg.spawn_depth != 0 {
            return HostToolDispatch::Output(
                "memory writes are only available on the parent agent".into(),
            );
        }
        if !crate::agent_memory::is_enabled() {
            return HostToolDispatch::Output("agent memory is disabled".into());
        }
        return HostToolDispatch::Output(crate::agent_memory::apply_tool(arguments));
    }
    if name == "list_skills" || name == "view_skill" {
        let project = if cfg.trusted {
            cfg.project_root
                .as_ref()
                .and_then(|path| path.to_str())
                .map(str::to_string)
        } else {
            None
        };
        if name == "list_skills" {
            return match crate::skill_inventory::list_enabled_skills_v1(project.as_deref()) {
                Ok(items) => HostToolDispatch::Output(
                    serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into()),
                ),
                Err(error) => HostToolDispatch::Output(error),
            };
        }
        let skill_id = tool_string_field(arguments, &["skill_id", "skillId", "id"])
            .unwrap_or_default()
            .trim()
            .to_string();
        let tree_hash = tool_string_field(arguments, &["tree_hash", "treeHash"])
            .unwrap_or_default()
            .trim()
            .to_string();
        if skill_id.is_empty() || tree_hash.is_empty() {
            return HostToolDispatch::Output("view_skill requires skill_id and tree_hash".into());
        }
        let relative = tool_string_field(arguments, &["path", "relative_path", "relativePath"]);
        return match crate::skill_inventory::view_skill_file_v1(
            &skill_id,
            &tree_hash,
            relative.as_deref(),
            project.as_deref(),
        ) {
            Ok(body) => {
                let extra = body.chars().count();
                let used = cfg.skill_prompt_chars.load(Ordering::SeqCst);
                if used.saturating_add(extra)
                    > crate::skill_inventory::HOST_SKILL_PROMPT_BUDGET_CHARS
                {
                    HostToolDispatch::Output(format!(
                        "SKILL_USE_LIMIT: attached Skill text exceeds {} characters",
                        crate::skill_inventory::HOST_SKILL_PROMPT_BUDGET_CHARS
                    ))
                } else {
                    cfg.skill_prompt_chars.fetch_add(extra, Ordering::SeqCst);
                    HostToolDispatch::Output(body)
                }
            }
            Err(error) => HostToolDispatch::Output(error),
        };
    }
    if name == "ask_user_question" {
        if cfg.kind != AgentKind::Parent || cfg.spawn_depth != 0 {
            return HostToolDispatch::Output(
                "ask_user_question is only available on the parent agent".into(),
            );
        }
        let parsed = parse_ask_user_question_params(arguments);
        if parsed.questions.is_empty() {
            return HostToolDispatch::Output("ask_user_question requires a question".into());
        }
        return match request_ask_user(
            cfg,
            HostAskUserRequest {
                tool_call_id: tool_call_id.into(),
                arguments: arguments.clone(),
            },
        )
        .await
        {
            HostAskUserDecision::Accepted { answers } => {
                HostToolDispatch::Output(format_ask_user_answers(&answers))
            }
            HostAskUserDecision::Cancelled => HostToolDispatch::Output(
                "The user skipped the question form. Continue in chat only if you still need an answer.".into(),
            ),
        };
    }
    if is_configured_connector_tool(cfg, name) {
        if needs_permission(name) {
            let preview = connector_permission_preview(arguments);
            let decision = request_tool_permission(
                cfg,
                HostToolPermission {
                    tool_name: name.into(),
                    title: format!("Connector {name}"),
                    preview,
                    path_target: String::new(),
                    command: String::new(),
                    tool_call_id: tool_call_id.into(),
                },
            )
            .await;
            match decision {
                HostToolPermissionDecision::Allow => {}
                HostToolPermissionDecision::Deny => {
                    return HostToolDispatch::Output("permission denied by user".into());
                }
                HostToolPermissionDecision::Cancelled => {
                    return HostToolDispatch::Cancelled;
                }
            }
        }
        let output = if let Some(invoke) = cfg.connectors.invoke.clone() {
            invoke(name.to_string(), arguments.clone()).await
        } else {
            crate::connectors::invoke_tool(name, arguments).await
        };
        return HostToolDispatch::Output(output);
    }
    if is_spawn_tool(name) {
        if cfg.spawn_depth >= 1 || !cfg.kind.allows_spawn() {
            return HostToolDispatch::Output(
                "subagents cannot spawn further agents (depth limit 1)".into(),
            );
        }
        match name {
            "spawn_agent" => {
                let mut request = match parse_spawn_request(arguments) {
                    Ok(request) => request,
                    Err(error) => return HostToolDispatch::Output(error),
                };
                request.tool_call_id = tool_call_id.to_string();
                let Some(spawn) = cfg.subagents.spawn.clone() else {
                    return HostToolDispatch::Output("subagents are not available".into());
                };
                return HostToolDispatch::Output(spawn(request).await);
            }
            "agent_output" => {
                let id = tool_agent_id_arg(arguments);
                if id.is_empty() {
                    return HostToolDispatch::Output("agent_output requires `id`".into());
                }
                let Some(output) = cfg.subagents.output.clone() else {
                    return HostToolDispatch::Output("subagents are not available".into());
                };
                return HostToolDispatch::Output(output(id).await);
            }
            "kill_agent" => {
                let id = tool_agent_id_arg(arguments);
                if id.is_empty() {
                    return HostToolDispatch::Output("kill_agent requires `id`".into());
                }
                let Some(kill) = cfg.subagents.kill.clone() else {
                    return HostToolDispatch::Output("subagents are not available".into());
                };
                return HostToolDispatch::Output(kill(id).await);
            }
            _ => {}
        }
    }
    if is_command_job_tool(name) {
        if cfg.kind != AgentKind::Parent || cfg.spawn_depth != 0 {
            return HostToolDispatch::Output(
                "background command tools are only available on the parent agent".into(),
            );
        }
        match name {
            "command_output" => {
                let id = tool_agent_id_arg(arguments);
                if id.is_empty() {
                    return HostToolDispatch::Output("command_output requires `id`".into());
                }
                let Some(output) = cfg.command_jobs.output.clone() else {
                    return HostToolDispatch::Output("background commands are not available".into());
                };
                if let Some(ms) = tool_timeout_ms(arguments) {
                    let Some(wait) = cfg.command_jobs.wait.clone() else {
                        return HostToolDispatch::Output(output(id).await);
                    };
                    return HostToolDispatch::Output(wait(vec![id], true, ms).await);
                }
                return HostToolDispatch::Output(output(id).await);
            }
            "wait_commands" => {
                let ids = tool_id_list(arguments);
                if ids.is_empty() {
                    return HostToolDispatch::Output("wait_commands requires `ids`".into());
                }
                let Some(wait) = cfg.command_jobs.wait.clone() else {
                    return HostToolDispatch::Output("background commands are not available".into());
                };
                let wait_any = tool_string_field(arguments, &["mode"])
                    .unwrap_or_default()
                    .eq_ignore_ascii_case("wait_any");
                let ms = tool_timeout_ms(arguments).unwrap_or(30_000);
                return HostToolDispatch::Output(wait(ids, wait_any, ms).await);
            }
            "kill_command" => {
                let id = tool_agent_id_arg(arguments);
                if id.is_empty() {
                    return HostToolDispatch::Output("kill_command requires `id`".into());
                }
                let Some(kill) = cfg.command_jobs.kill.clone() else {
                    return HostToolDispatch::Output("background commands are not available".into());
                };
                return HostToolDispatch::Output(kill(id).await);
            }
            "monitor" => {
                let id = tool_agent_id_arg(arguments);
                if id.is_empty() {
                    return HostToolDispatch::Output("monitor requires `id`".into());
                }
                let pattern = tool_string_field(arguments, &["pattern"]);
                let Some(monitor) = cfg.command_jobs.monitor.clone() else {
                    return HostToolDispatch::Output("background commands are not available".into());
                };
                return HostToolDispatch::Output(monitor(id, pattern).await);
            }
            _ => {}
        }
    }
    if !is_host_tool(name) {
        return HostToolDispatch::Output(format!("unknown tool `{name}`"));
    }
    if (name == "write_file" || name == "search_replace") && !cfg.kind.allows_write() {
        return HostToolDispatch::Output(format!(
            "{} agents cannot write files",
            cfg.kind.as_str()
        ));
    }
    if name == "run_command" && !cfg.kind.allows_command() {
        return HostToolDispatch::Output(format!(
            "{} agents cannot run commands",
            cfg.kind.as_str()
        ));
    }
    if !cfg.trusted || cfg.project_root.is_none() {
        return HostToolDispatch::Output("no trusted project root".into());
    }
    let root = cfg.project_root.as_deref().unwrap();

    if name == "write_file" || name == "search_replace" {
        let prepared = match prepared_write.unwrap_or_else(|| {
            if name == "search_replace" {
                prepare_search_replace(root, arguments)
            } else {
                prepare_write_file(root, arguments)
            }
        }) {
            Ok(prepared) => prepared,
            Err(error) => return HostToolDispatch::Output(error),
        };
        if needs_permission(name) {
            let decision = request_tool_permission(
                cfg,
                HostToolPermission {
                    tool_name: name.into(),
                    title: prepared.title.clone(),
                    preview: prepared.preview.clone(),
                    path_target: prepared.path_target.clone(),
                    command: String::new(),
                    tool_call_id: tool_call_id.into(),
                },
            )
            .await;
            match decision {
                HostToolPermissionDecision::Allow => {}
                HostToolPermissionDecision::Deny => {
                    return HostToolDispatch::Output("permission denied by user".into());
                }
                HostToolPermissionDecision::Cancelled => {
                    return HostToolDispatch::Cancelled;
                }
            }
        }
        let written = execute_write_file(root, &prepared.rel, &prepared.content)
            .unwrap_or_else(|error| error);
        if name == "search_replace" && written.starts_with("wrote ") {
            return HostToolDispatch::Output(format!("updated {}", prepared.rel));
        }
        return HostToolDispatch::Output(written);
    }

    if name == "run_command" {
        let prepared =
            match prepared_command.unwrap_or_else(|| prepare_run_command(root, arguments)) {
                Ok(prepared) => prepared,
                Err(error) => return HostToolDispatch::Output(error),
            };
        if needs_permission(name) {
            let decision = request_tool_permission(
                cfg,
                HostToolPermission {
                    tool_name: name.into(),
                    title: prepared.title.clone(),
                    preview: prepared.preview.clone(),
                    path_target: String::new(),
                    command: prepared.command.clone(),
                    tool_call_id: tool_call_id.into(),
                },
            )
            .await;
            match decision {
                HostToolPermissionDecision::Allow => {}
                HostToolPermissionDecision::Deny => {
                    return HostToolDispatch::Output("permission denied by user".into());
                }
                HostToolPermissionDecision::Cancelled => {
                    return HostToolDispatch::Cancelled;
                }
            }
        }
        let background = tool_bool_field(arguments, &["background", "run_in_background"]);
        if background {
            if cfg.kind != AgentKind::Parent || cfg.spawn_depth != 0 {
                return HostToolDispatch::Output(
                    "background run_command is only available on the parent agent".into(),
                );
            }
            let Some(start) = cfg.command_jobs.start.clone() else {
                return HostToolDispatch::Output("background commands are not available".into());
            };
            return HostToolDispatch::Output(
                start(StartCommandJobRequest {
                    command: prepared.command.clone(),
                    cwd: prepared.cwd_canon.clone(),
                    project_root: root.to_path_buf(),
                    sandbox: cfg.sandbox_profile,
                    tool_call_id: tool_call_id.to_string(),
                    title: prepared.title.clone(),
                })
                .await,
            );
        }
        return match execute_run_command(
            &prepared.command,
            &prepared.cwd_canon,
            Arc::clone(&cfg.stop),
            cfg.sandbox_profile,
            root,
        )
        .await
        {
            Ok(RunCommandOutcome::Output(text)) => HostToolDispatch::Output(text),
            Ok(RunCommandOutcome::Cancelled) => HostToolDispatch::Cancelled,
            Err(error) => HostToolDispatch::Output(error),
        };
    }

    HostToolDispatch::Output(execute_tool(
        cfg.project_root.as_deref(),
        cfg.trusted,
        name,
        arguments,
    ))
}

pub async fn run_turn<F>(cfg: AgentTurnConfig, mut emit: F)
where
    F: FnMut(AcpEvent) + Send,
{
    let host_tools_enabled = cfg.trusted && cfg.project_root.is_some();
    let instruction = load_project_instruction(cfg.project_root.as_deref(), cfg.trusted);
    let mut system = system_prompt(
        cfg.project_root.as_deref(),
        cfg.trusted,
        instruction.as_ref(),
        cfg.kind,
        cfg.sandbox_profile,
    );
    if cfg.kind.allows_connectors() && !cfg.connectors.tools.is_empty() {
        system.push_str(
            " Connected connectors are available as extra tools and do not require a trusted project.",
        );
        if !cfg.connectors.explicit.is_empty() {
            system.push(' ');
            system.push_str(&crate::connectors::explicit_connector_fragment(
                &cfg.connectors.explicit,
            ));
        }
    }
    if let Some(index) = Some(crate::skill_inventory::skill_index_prompt_v1(
        if cfg.trusted {
            cfg.project_root.as_ref().and_then(|path| path.to_str())
        } else {
            None
        },
    ))
    .filter(|text| !text.is_empty())
    {
        system.push_str("\n\n");
        system.push_str(&index);
    }
    let mut tool_defs = Vec::new();
    if crate::agent_memory::is_enabled() {
        let snapshot = crate::agent_memory::snapshot_prompt();
        if !snapshot.is_empty() {
            system.push_str(&snapshot);
        }
        if cfg.kind == AgentKind::Parent && cfg.spawn_depth == 0 {
            if let Value::Array(memory_tools) = memory_tool_definitions() {
                tool_defs.extend(memory_tools);
            }
        }
    }
    if cfg.kind == AgentKind::Parent && cfg.spawn_depth == 0 && cfg.allow_schedule_task {
        tool_defs.push(json!({
            "type": "function",
            "function": {
                "name": "schedule_task",
                "description": "Create, list, update, pause, or delete app-resident scheduled tasks. Actions: create, list, update, set_enabled, delete. Frequencies: daily, weekly, weekdays, once, hourly, interval (interval_minutes >= 15). Optional skill_ids: [{id, tree_hash}]. Disabled during scheduled runs.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string" },
                        "id": { "type": "string" },
                        "title": { "type": "string" },
                        "prompt": { "type": "string" },
                        "frequency": { "type": "string" },
                        "time": { "type": "string" },
                        "weekdays": { "type": "array", "items": { "type": "integer" } },
                        "interval_minutes": { "type": "integer" },
                        "enabled": { "type": "boolean" },
                        "model_id": { "type": "string" },
                        "effort": { "type": "string" },
                        "project_id": { "type": "string" },
                        "skill_ids": { "type": "array" }
                    },
                    "required": ["action"]
                }
            }
        }));
    }
    if cfg.kind == AgentKind::Parent && cfg.spawn_depth == 0 && cfg.allow_skill_save {
        tool_defs.push(json!({
            "type": "function",
            "function": {
                "name": "skill_save",
                "description": "Create or update a Host-owned user or project Skill. Actions: create, update. Update requires skill_id and expected_tree_hash. Cannot overwrite plugin or external Skills. Requires permission; disabled during scheduled runs.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string" },
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "skill_md": { "type": "string" },
                        "references": { "type": "array" },
                        "scope": { "type": "string" },
                        "skill_id": { "type": "string" },
                        "expected_tree_hash": { "type": "string" }
                    },
                    "required": ["action"]
                }
            }
        }));
    }
    if let Value::Array(skills) = skill_tool_definitions() {
        tool_defs.extend(skills);
    }
    if host_tools_enabled {
        if let Value::Array(host) = tool_definitions_for(cfg.kind, cfg.spawn_depth) {
            tool_defs.extend(host);
        }
    }
    if cfg.kind.allows_connectors() {
        tool_defs.extend(cfg.connectors.tools.clone());
    }
    let tools_payload = if tool_defs.is_empty() {
        None
    } else {
        Some(Value::Array(tool_defs))
    };
    let mut messages = vec![json!({
        "role": "system",
        "content": system,
    })];
    messages.extend(cfg.history.clone());
    messages.push(json!({
        "role": "user",
        "content": cfg.user_prompt,
    }));

    let mut last_tool_fingerprint = String::new();
    let mut identical_tool_streak: u32 = 0;
    let mut last_usage: Option<CompletionUsage> = None;
    let mut model_calls: u32 = 0;

    for round in 0..cfg.max_tool_rounds.max(1) {
        if cfg.stop.load(Ordering::SeqCst) {
            return;
        }
        let outcome = tokio::select! {
            _ = wait_until_stopped(Arc::clone(&cfg.stop)) => {
                return;
            }
            outcome = stream_chat_completion(
                &cfg.client,
                &cfg.endpoint,
                &messages,
                tools_payload.as_ref(),
                cfg.reasoning_effort.as_deref(),
                &cfg.stop,
                &mut emit,
                true,
                cfg.stream_idle,
            ) => outcome,
        };
        match outcome {
            Ok((ChatOutcome::Text, usage)) => {
                model_calls = model_calls.saturating_add(1);
                if let Some(usage) = usage {
                    last_usage = Some(usage);
                }
                if !cfg.stop.load(Ordering::SeqCst) {
                    emit_last_usage(
                        &mut emit,
                        &cfg.endpoint.model,
                        model_calls,
                        last_usage.as_ref(),
                    );
                    emit(AcpEvent::PromptComplete {
                        stop_reason: "end_turn".into(),
                    });
                }
                return;
            }
            Ok((ChatOutcome::ToolCalls(calls), usage)) => {
                model_calls = model_calls.saturating_add(1);
                if let Some(usage) = usage {
                    last_usage = Some(usage);
                }
                if calls.is_empty() {
                    emit(AcpEvent::PromptComplete {
                        stop_reason: "end_turn".into(),
                    });
                    return;
                }
                let last_call_index = calls.len().saturating_sub(1);
                let remaining_after = remaining_model_rounds(round, cfg.max_tool_rounds.max(1));
                let mut assistant_tool_calls = Vec::new();
                for (index, call) in calls.into_iter().enumerate() {
                    if cfg.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let id = if call.id.is_empty() {
                        format!("tool-{round}-{index}")
                    } else {
                        call.id.clone()
                    };
                    let args_value = parse_tool_arguments(&call.arguments);
                    let fingerprint = tool_fingerprint(&call.name, &args_value);
                    if fingerprint == last_tool_fingerprint {
                        identical_tool_streak = identical_tool_streak.saturating_add(1);
                    } else {
                        last_tool_fingerprint = fingerprint;
                        identical_tool_streak = 1;
                    }
                    let loop_blocked = identical_tool_streak >= IDENTICAL_TOOL_STREAK_LIMIT;
                    let prepared_write = if cfg.trusted && cfg.project_root.is_some() {
                        match call.name.as_str() {
                            "write_file" => cfg
                                .project_root
                                .as_deref()
                                .map(|root| prepare_write_file(root, &args_value)),
                            "search_replace" => cfg
                                .project_root
                                .as_deref()
                                .map(|root| prepare_search_replace(root, &args_value)),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let prepared_command = if call.name == "run_command"
                        && cfg.trusted
                        && cfg.project_root.is_some()
                    {
                        cfg.project_root
                            .as_deref()
                            .map(|root| prepare_run_command(root, &args_value))
                    } else {
                        None
                    };
                    let title = match call.name.as_str() {
                        "read_file" => format!("Read {}", tool_path_arg(&args_value)),
                        "list_directory" => {
                            format!("List {}", tool_path_arg(&args_value))
                        }
                        "grep" => {
                            let pattern = tool_pattern_arg(&args_value);
                            let shown: String = pattern.chars().take(48).collect();
                            if pattern.chars().count() > 48 {
                                format!("Grep {shown}…")
                            } else if shown.is_empty() {
                                "Grep".into()
                            } else {
                                format!("Grep {shown}")
                            }
                        }
                        "spawn_agent" => {
                            let desc = tool_string_field(&args_value, &["description", "label"])
                                .unwrap_or_else(|| "subagent".into());
                            format!("Subagent {desc}")
                        }
                        "agent_output" => "Agent output".into(),
                        "kill_agent" => "Kill agent".into(),
                        "ask_user_question" => parse_ask_user_question_params(&args_value)
                            .questions
                            .first()
                            .map(|question| question.question.clone())
                            .filter(|text| !text.trim().is_empty())
                            .unwrap_or_else(|| "Ask user".into()),
                        "write_file" | "search_replace" => prepared_write
                            .as_ref()
                            .and_then(|prepared| prepared.as_ref().ok())
                            .map(|prepared| prepared.title.clone())
                            .unwrap_or_else(|| {
                                let rel = display_rel(&tool_write_path_arg(&args_value));
                                if call.name == "search_replace" {
                                    format!("Replace {rel}")
                                } else {
                                    format!("Write {rel}")
                                }
                            }),
                        "run_command" => prepared_command
                            .as_ref()
                            .and_then(|prepared| prepared.as_ref().ok())
                            .map(|prepared| prepared.title.clone())
                            .unwrap_or_else(|| {
                                let command = tool_command_arg(&args_value);
                                if command.is_empty() {
                                    "Run".into()
                                } else {
                                    format!("Run {command}")
                                }
                            }),
                        "list_skills" => "List Skills".into(),
                        "view_skill" => "View Skill".into(),
                        "memory" => "Memory".into(),
                        "schedule_task" => "Schedule".into(),
                        "skill_save" => "Save Skill".into(),
                        "command_output" => "Command output".into(),
                        "wait_commands" => "Wait commands".into(),
                        "kill_command" => "Kill command".into(),
                        other if crate::permission::is_connector_tool(other) => {
                            format!("Connector {other}")
                        }
                        other => other.to_string(),
                    };
                    let kind = match call.name.as_str() {
                        "read_file" => "read",
                        "list_directory" => "list",
                        "grep" => "search",
                        "spawn_agent" | "agent_output" | "kill_agent" => "agent",
                        "ask_user_question" => "ask",
                        "write_file" | "search_replace" => "edit",
                        "run_command" | "command_output" | "wait_commands" | "kill_command" => {
                            "execute"
                        }
                        "list_skills" | "view_skill" | "memory" => "read",
                        "skill_save" => "edit",
                        other if crate::permission::is_connector_write_tool(other) => "execute",
                        other if crate::permission::is_connector_tool(other) => "fetch",
                        other => other,
                    };
                    let raw_input = match call.name.as_str() {
                        "write_file" => {
                            let rel = display_rel(&tool_write_path_arg(&args_value));
                            let bytes = tool_content_arg(&args_value)
                                .ok()
                                .map(|content| content.len())
                                .unwrap_or(0);
                            json!({ "path": rel, "bytes": bytes })
                        }
                        "search_replace" => {
                            let rel = display_rel(&tool_write_path_arg(&args_value));
                            json!({
                                "path": rel,
                                "replaceAll": tool_bool_field(
                                    &args_value,
                                    &["replace_all", "replaceAll"]
                                ),
                            })
                        }
                        "spawn_agent" => json!({
                            "description": tool_string_field(&args_value, &["description", "label"]).unwrap_or_default(),
                            "agentType": tool_string_field(&args_value, &["agent_type", "agentType"]).unwrap_or_else(|| "general".into()),
                            "background": tool_bool_field(&args_value, &["background", "run_in_background"]),
                        }),
                        "agent_output" | "kill_agent" => json!({
                            "id": tool_agent_id_arg(&args_value),
                        }),
                        "run_command" => json!({
                            "command": tool_command_arg(&args_value),
                            "cwd": display_rel(&tool_cwd_arg(&args_value)),
                            "background": tool_bool_field(
                                &args_value,
                                &["background", "run_in_background"]
                            ),
                        }),
                        "command_output" | "kill_command" => json!({
                            "id": tool_agent_id_arg(&args_value),
                        }),
                        "wait_commands" => json!({
                            "ids": tool_id_list(&args_value),
                        }),
                        _ => args_value.clone(),
                    };
                    let raw = json!({ "rawInput": raw_input });
                    emit(AcpEvent::ToolCall {
                        tool_call_id: id.clone(),
                        title: title.clone(),
                        kind: kind.into(),
                        status: "in_progress".into(),
                        raw: raw.clone(),
                    });
                    let mut output = if loop_blocked {
                        identical_tool_loop_message(&call.name, identical_tool_streak)
                    } else {
                        match dispatch_host_tool(
                            &cfg,
                            &call.name,
                            &args_value,
                            &id,
                            prepared_write,
                            prepared_command,
                        )
                        .await
                        {
                            HostToolDispatch::Cancelled => return,
                            HostToolDispatch::Output(output) => output,
                        }
                    };
                    if remaining_after == 1 && index == last_call_index {
                        if !output.is_empty() && !output.ends_with('\n') {
                            output.push('\n');
                        }
                        output.push('\n');
                        output.push_str(last_tool_round_warning());
                    }
                    if cfg.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let mut raw = raw;
                    let mut tool_status = "completed";
                    if call.name == "spawn_agent" || call.name == "run_command" {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&output) {
                            if let Some(agent_id) = parsed.get("id").and_then(Value::as_str) {
                                raw["rawInput"]["id"] = json!(agent_id);
                            }
                            if parsed.get("status").and_then(Value::as_str) == Some("running") {
                                tool_status = "in_progress";
                            }
                            if parsed.get("status").and_then(Value::as_str) == Some("failed") {
                                tool_status = "failed";
                            }
                            if parsed.get("status").and_then(Value::as_str) == Some("cancelled") {
                                tool_status = "failed";
                            }
                        }
                    }
                    emit(AcpEvent::ToolCall {
                        tool_call_id: id.clone(),
                        title,
                        kind: kind.into(),
                        status: tool_status.into(),
                        raw,
                    });
                    assistant_tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments,
                        }
                    }));
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": output,
                    }));
                }
                messages.insert(
                    messages.len() - assistant_tool_calls.len(),
                    json!({
                        "role": "assistant",
                        "content": null,
                        "tool_calls": assistant_tool_calls,
                    }),
                );
            }
            Err(error) => {
                if !cfg.stop.load(Ordering::SeqCst) {
                    emit(AcpEvent::Error { error });
                }
                return;
            }
        }
    }
    if cfg.stop.load(Ordering::SeqCst) {
        return;
    }
    emit_last_usage(
        &mut emit,
        &cfg.endpoint.model,
        model_calls,
        last_usage.as_ref(),
    );
    let budget_text = tool_budget_exhausted_message(cfg.max_tool_rounds.max(1));
    emit(AcpEvent::Stream {
        kind: StreamKind::Assistant,
        text: budget_text,
        message_id: None,
        done: false,
    });
    emit(AcpEvent::Stream {
        kind: StreamKind::Assistant,
        text: String::new(),
        message_id: None,
        done: true,
    });
    emit(AcpEvent::PromptComplete {
        stop_reason: TOOL_BUDGET_STOP_REASON.into(),
    });
}

enum ChatOutcome {
    Text,
    ToolCalls(Vec<PendingToolCall>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub reasoning_tokens: u64,
}

fn emit_last_usage<F>(
    emit: &mut F,
    model_id: &str,
    model_calls: u32,
    usage: Option<&CompletionUsage>,
) where
    F: FnMut(AcpEvent) + Send,
{
    let Some(usage) = usage else {
        return;
    };
    emit(AcpEvent::Usage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cached_read_tokens: usage.cached_read_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        model_calls,
        model_id: Some(model_id.to_string()),
    });
}

fn json_u64_field(value: &Value, camel: &str, snake: &str) -> u64 {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn parse_completion_usage(parsed: &Value) -> Option<CompletionUsage> {
    let usage = parsed.get("usage")?;
    let input_tokens = json_u64_field(usage, "promptTokens", "prompt_tokens");
    let output_tokens = json_u64_field(usage, "completionTokens", "completion_tokens");
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }
    Some(CompletionUsage {
        input_tokens,
        output_tokens,
        cached_read_tokens: json_u64_field(usage, "cachedPromptTokens", "cached_prompt_tokens"),
        reasoning_tokens: json_u64_field(usage, "reasoningTokens", "reasoning_tokens"),
    })
}

fn chat_completion_body(
    model: &str,
    messages: &[Value],
    tools: Option<&Value>,
    reasoning_effort: Option<&str>,
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if let Some(tools) = tools {
        body["tools"] = tools.clone();
        body["tool_choice"] = json!("auto");
        body["parallel_tool_calls"] = json!(false);
    }
    let effort = reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| crate::models_catalog::model_supports_reasoning_effort(model));
    if let Some(effort) = effort {
        body["reasoning_effort"] = json!(effort);
    }
    body
}

pub async fn complete_text(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    messages: &[Value],
    stop: &AtomicBool,
) -> Result<String, AgentError> {
    let mut collected = String::new();
    let mut emit = |event: AcpEvent| {
        if let AcpEvent::Stream {
            text, done: false, ..
        } = event
        {
            collected.push_str(&text);
        }
    };
    let _ = stream_chat_completion(
        client,
        endpoint,
        messages,
        None,
        None,
        stop,
        &mut emit,
        true,
        stall_duration(DEFAULT_STREAM_STALL_SECONDS),
    )
    .await?;
    if stop.load(Ordering::SeqCst) {
        return Err(AgentError::new(
            AgentErrorCode::AgentCrashed,
            "compact cancelled",
        ));
    }
    Ok(collected)
}

async fn stream_chat_completion<F>(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    messages: &[Value],
    tools: Option<&Value>,
    reasoning_effort: Option<&str>,
    stop: &AtomicBool,
    emit: &mut F,
    emit_text: bool,
    stream_idle: Duration,
) -> Result<(ChatOutcome, Option<CompletionUsage>), AgentError>
where
    F: FnMut(AcpEvent) + Send,
{
    let url = chat_completions_url(&endpoint.base_url);
    let body = chat_completion_body(&endpoint.model, messages, tools, reasoning_effort);
    let send = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", endpoint.api_key))
        .header("Accept", "text/event-stream")
        .json(&body)
        .send();
    let response = await_with_idle(send, stream_idle, "chat request stalled")
        .await?
        .map_err(|error| {
            AgentError::new(
                AgentErrorCode::NetworkProvider,
                format!("chat request failed: {error}"),
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(240)
            .collect::<String>();
        let code = if status.as_u16() == 401 || status.as_u16() == 403 {
            AgentErrorCode::AuthFailed
        } else if status.as_u16() == 429 {
            AgentErrorCode::QuotaExceeded
        } else {
            AgentErrorCode::NetworkProvider
        };
        return Err(AgentError::new(
            code,
            format!("chat HTTP {}: {detail}", status.as_u16()),
        ));
    }

    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    let mut tool_calls: BTreeMap<u32, PendingToolCall> = BTreeMap::new();
    let mut saw_text = false;
    let mut usage = None;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok((ChatOutcome::Text, usage));
        }
        let next = await_with_idle(stream.next(), stream_idle, "chat stream stalled").await?;
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(|error| {
            AgentError::new(
                AgentErrorCode::NetworkProvider,
                format!("chat stream: {error}"),
            )
        })?;
        pending.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(idx) = pending.find("\n\n") {
            let event = pending[..idx].replace("\r", "");
            pending = pending[idx + 2..].to_string();
            for line in event.lines() {
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let parsed: Value = serde_json::from_str(data).map_err(|_| {
                    AgentError::new(AgentErrorCode::NetworkProvider, "chat stream is not JSON")
                })?;
                if let Some(next_usage) = parse_completion_usage(&parsed) {
                    usage = Some(next_usage);
                }
                let choice = parsed.pointer("/choices/0").cloned().unwrap_or(Value::Null);
                let delta = choice.get("delta").cloned().unwrap_or(Value::Null);
                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        saw_text = true;
                        if emit_text {
                            emit(AcpEvent::Stream {
                                kind: StreamKind::Assistant,
                                text: text.to_string(),
                                message_id: None,
                                done: false,
                            });
                        }
                    }
                }
                apply_tool_delta(&mut tool_calls, &delta);
            }
        }
    }
    if !tool_calls.is_empty() {
        return Ok((
            ChatOutcome::ToolCalls(tool_calls.into_values().collect()),
            usage,
        ));
    }
    if saw_text && emit_text {
        emit(AcpEvent::Stream {
            kind: StreamKind::Assistant,
            text: String::new(),
            message_id: None,
            done: true,
        });
    }
    Ok((ChatOutcome::Text, usage))
}

async fn await_with_idle<F, T>(
    future: F,
    idle: Duration,
    stalled: &str,
) -> Result<T, AgentError>
where
    F: Future<Output = T>,
{
    if idle.is_zero() {
        return Ok(future.await);
    }
    tokio::time::timeout(idle, future).await.map_err(|_| {
        AgentError::new(
            AgentErrorCode::NetworkProvider,
            format!("{stalled} after {}s", idle.as_secs().max(1)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    fn temp_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sunsetz-agent-loop-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_file(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn loads_first_available_project_instruction_and_skips_untrusted() {
        let root = temp_root("instruction");
        write_file(&root, "CLAUDE.md", "from claude");
        write_file(&root, "AGENTS.md", "from agents");
        let loaded = load_project_instruction(Some(&root), true).unwrap();
        assert_eq!(loaded.relative_path, "AGENTS.md");
        assert_eq!(loaded.body, "from agents");
        assert!(!loaded.truncated);
        assert!(load_project_instruction(Some(&root), false).is_none());
        let prompt = system_prompt(
            Some(&root),
            true,
            Some(&loaded),
            AgentKind::Parent,
            runtime_compat::SandboxProfileV1::Off,
        );
        assert!(prompt.contains("from agents"));
        assert!(prompt.contains("cannot override permission policy"));
        assert!(prompt.contains("grep, list_directory, or read_file"));
        assert!(prompt.contains("unsandboxed"));
        let sandboxed = system_prompt(
            Some(&root),
            true,
            Some(&loaded),
            AgentKind::Parent,
            runtime_compat::SandboxProfileV1::ReadOnly,
        );
        assert!(sandboxed.contains("read-only sandbox"));
        assert!(!sandboxed.contains("unsandboxed"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn truncates_project_instructions_and_skips_empty_files() {
        let root = temp_root("instruction-trunc");
        write_file(&root, "AGENTS.md", "   \n");
        write_file(
            &root,
            "Sunsetz.md",
            &"project-rule ".repeat(MAX_PROJECT_INSTRUCTION_CHARS / 8 + 8),
        );
        let loaded = load_project_instruction(Some(&root), true).unwrap();
        assert_eq!(loaded.relative_path, "Sunsetz.md");
        assert!(loaded.truncated);
        assert_eq!(loaded.body.chars().count(), MAX_PROJECT_INSTRUCTION_CHARS);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_project_instruction_files() {
        let root = temp_root("instruction-link");
        let target = root.join("secret.md");
        std::fs::write(&target, "should not inject").unwrap();
        std::os::unix::fs::symlink(&target, root.join("AGENTS.md")).unwrap();
        write_file(&root, "Sunsetz.md", "safe instructions");
        let loaded = load_project_instruction(Some(&root), true).unwrap();
        assert_eq!(loaded.relative_path, "Sunsetz.md");
        assert_eq!(loaded.body, "safe instructions");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sse_text(parts: &[&str]) -> String {
        let mut out = String::new();
        for (index, part) in parts.iter().enumerate() {
            let finish = if index + 1 == parts.len() {
                json!("stop")
            } else {
                Value::Null
            };
            let payload = json!({
                "id": "chatcmpl-test",
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
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

    fn sse_text_with_usage(parts: &[&str], prompt_tokens: u64, completion_tokens: u64) -> String {
        let mut out = sse_text(parts);
        let usage = json!({
            "id": "chatcmpl-test",
            "object": "chat.completion.chunk",
            "choices": [],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "cached_prompt_tokens": 4,
                "reasoning_tokens": 1
            }
        });
        // Insert the usage chunk before [DONE].
        out = out.replace(
            "data: [DONE]\n\n",
            &format!("data: {usage}\n\ndata: [DONE]\n\n"),
        );
        out
    }

    fn sse_tool_then(name: &str, arguments: &str, answer_parts: &[&str]) -> (String, String) {
        let start = json!({
            "id": "chatcmpl-test",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": name, "arguments": "" }
                    }]
                },
                "finish_reason": null
            }]
        });
        let args = json!({
            "id": "chatcmpl-test",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": arguments }
                    }]
                },
                "finish_reason": null
            }]
        });
        let done = json!({
            "id": "chatcmpl-test",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        });
        let mut first = String::new();
        for payload in [start, args, done] {
            first.push_str("data: ");
            first.push_str(&payload.to_string());
            first.push_str("\n\n");
        }
        first.push_str("data: [DONE]\n\n");
        (first, sse_text(answer_parts))
    }

    struct MockLlm {
        responses: Arc<Mutex<VecDeque<String>>>,
    }

    async fn spawn_mock_llm(responses: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = MockLlm {
            responses: Arc::new(Mutex::new(VecDeque::from(responses))),
        };
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                let mut headers_end = None;
                let mut content_length = 0usize;
                loop {
                    let n = match socket.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if headers_end.is_none() {
                        if let Some(idx) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            headers_end = Some(idx);
                            let headers = String::from_utf8_lossy(&buf[..idx]).to_ascii_lowercase();
                            content_length = headers
                                .lines()
                                .find_map(|line| {
                                    line.strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse().ok())
                                })
                                .unwrap_or(0);
                        }
                    }
                    if let Some(idx) = headers_end {
                        if buf.len() >= idx + 4 + content_length {
                            break;
                        }
                    }
                }
                let body = state
                    .responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| sse_text(&["missing mock response"]));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}/v1"), handle)
    }

    async fn spawn_stalling_llm(delay: Duration) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
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
                if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nTransfer-Encoding: chunked\r\n\r\n";
            let _ = socket.write_all(headers.as_bytes()).await;
            let _ = socket.flush().await;
            tokio::time::sleep(delay).await;
        });
        (format!("http://{addr}/v1"), handle)
    }

    async fn collect_events(cfg: AgentTurnConfig) -> Vec<AcpEvent> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        run_turn(cfg, move |event| {
            let _ = tx.send(event);
        })
        .await;
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            out.push(event);
        }
        out
    }

    fn base_cfg(
        base_url: String,
        root: Option<PathBuf>,
        trusted: bool,
        prompt: &str,
    ) -> AgentTurnConfig {
        AgentTurnConfig {
            endpoint: LlmEndpoint {
                base_url,
                api_key: "test-key".into(),
                model: "grok-4.5".into(),
            },
            project_root: root,
            trusted,
            history: Vec::new(),
            user_prompt: prompt.into(),
            stop: Arc::new(AtomicBool::new(false)),
            client: http_client().unwrap(),
            max_tool_rounds: MAX_TOOL_ROUNDS,
            stream_idle: stall_duration(DEFAULT_STREAM_STALL_SECONDS),
            permission_gate: None,
            ask_user_gate: None,
            connectors: ConnectorTurn::default(),
            reasoning_effort: None,
            kind: AgentKind::Parent,
            spawn_depth: 0,
            subagents: SubagentHooks::default(),
            command_jobs: CommandJobHooks::default(),
            sandbox_profile: runtime_compat::SandboxProfileV1::Off,
            skill_prompt_chars: Arc::new(AtomicUsize::new(0)),
            allow_schedule_task: true,
            allow_skill_save: true,
        }
    }

    fn allow_gate() -> HostToolPermissionGate {
        Arc::new(|_req| Box::pin(async { HostToolPermissionDecision::Allow }))
    }

    fn deny_gate() -> HostToolPermissionGate {
        Arc::new(|_req| Box::pin(async { HostToolPermissionDecision::Deny }))
    }

    fn counting_gate(
        hits: Arc<std::sync::atomic::AtomicU32>,
        decision: HostToolPermissionDecision,
    ) -> HostToolPermissionGate {
        Arc::new(move |_req| {
            hits.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { decision })
        })
    }

    fn recording_gate(
        sink: Arc<Mutex<Vec<HostToolPermission>>>,
        decision: HostToolPermissionDecision,
    ) -> HostToolPermissionGate {
        Arc::new(move |req| {
            sink.lock().unwrap().push(req);
            Box::pin(async move { decision })
        })
    }

    #[test]
    fn default_backend_is_sunsetz_without_legacy_flag() {
        assert_eq!(default_runtime_backend(), BACKEND_SUNSETZ);
        assert_eq!(resolve_backend("", None, false), BACKEND_SUNSETZ);
        assert_eq!(resolve_backend("sunsetz", None, false), BACKEND_SUNSETZ);
        assert_eq!(resolve_backend("grok_acp", None, false), BACKEND_GROK_ACP);
        assert_eq!(
            resolve_backend("sunsetz", Some("grok_acp"), false),
            BACKEND_GROK_ACP
        );
        assert_eq!(
            resolve_backend("grok_acp", Some("sunsetz"), false),
            BACKEND_SUNSETZ
        );
        assert_eq!(resolve_backend("sunsetz", None, true), BACKEND_MOCK);
        assert!(!use_legacy_grok_acp() || current_backend() == BACKEND_GROK_ACP);
        assert_eq!(
            resolve_backend_report("sunsetz", None, true),
            BackendResolution {
                stored: BACKEND_SUNSETZ.into(),
                effective: BACKEND_MOCK.into(),
                override_source: OVERRIDE_ENV_MOCK.into(),
            }
        );
        assert_eq!(
            resolve_backend_report("sunsetz", Some("grok_acp"), false),
            BackendResolution {
                stored: BACKEND_SUNSETZ.into(),
                effective: BACKEND_GROK_ACP.into(),
                override_source: OVERRIDE_ENV_BACKEND.into(),
            }
        );
        assert_eq!(
            resolve_backend_report("grok_acp", Some("sunsetz"), false),
            BackendResolution {
                stored: SETTING_LEGACY_GROK_ACP.into(),
                effective: BACKEND_SUNSETZ.into(),
                override_source: OVERRIDE_ENV_BACKEND.into(),
            }
        );
        assert_eq!(
            resolve_backend_report("sunsetz", Some("sunsetz"), false).override_source,
            OVERRIDE_ENV_BACKEND
        );
        assert_eq!(
            resolve_backend_report("grok_acp", None, false),
            BackendResolution {
                stored: SETTING_LEGACY_GROK_ACP.into(),
                effective: BACKEND_GROK_ACP.into(),
                override_source: OVERRIDE_NONE.into(),
            }
        );
    }

    #[test]
    fn chat_body_sends_reasoning_effort_only_when_declared() {
        let messages = vec![json!({ "role": "user", "content": "hi" })];
        let supported = chat_completion_body("grok-4.5", &messages, None, Some("high"));
        assert_eq!(supported["reasoning_effort"], json!("high"));
        assert_eq!(
            supported["stream_options"],
            json!({ "include_usage": true })
        );
        let custom = chat_completion_body("claude-opus-4-6", &messages, None, Some("high"));
        assert!(custom.get("reasoning_effort").is_none());
    }

    #[test]
    fn tools_include_write_and_run_command() {
        let listed = tool_definitions().to_string();
        assert!(listed.contains("read_file"));
        assert!(listed.contains("list_directory"));
        assert!(listed.contains("grep"));
        assert!(listed.contains("write_file"));
        assert!(listed.contains("search_replace"));
        assert!(listed.contains("run_command"));
        assert!(listed.contains("spawn_agent"));
        assert!(listed.contains("command_output"));
        assert!(listed.contains("wait_commands"));
        assert!(listed.contains("kill_command"));
        assert!(listed.contains("monitor"));
        assert!(listed.contains("ask_user_question"));
        let skills = skill_tool_definitions().to_string();
        assert!(skills.contains("list_skills"));
        assert!(skills.contains("view_skill"));
        let explore = tool_definitions_for(AgentKind::Explore, 0).to_string();
        assert!(explore.contains("grep"));
        assert!(!explore.contains("write_file"));
        assert!(!explore.contains("run_command"));
        assert!(!explore.contains("spawn_agent"));
        assert!(!explore.contains("ask_user_question"));
        let child_general = tool_definitions_for(AgentKind::General, 1).to_string();
        assert!(child_general.contains("write_file"));
        assert!(!child_general.contains("spawn_agent"));
        assert!(!child_general.contains("command_output"));
        assert!(!child_general.contains("monitor"));
        assert!(!child_general.contains("ask_user_question"));
    }

    #[tokio::test]
    async fn background_run_command_returns_id_without_waiting() {
        let root = temp_root("bg-run");
        let (tool, answer) = sse_tool_then(
            "run_command",
            r#"{"command":"sleep 30","background":true}"#,
            &["started in background"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "run");
        cfg.permission_gate = Some(allow_gate());
        cfg.command_jobs.start = Some(Arc::new(|request| {
            Box::pin(async move {
                json!({
                    "id": "cmd-1",
                    "status": "running",
                    "command": request.command,
                })
                .to_string()
            })
        }));
        let started = std::time::Instant::now();
        let events = collect_events(cfg).await;
        server.abort();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, .. }
                if kind == "execute" && status == "in_progress"
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn ask_user_question_returns_answers_from_gate() {
        let root = temp_root("ask-user");
        let gate: HostAskUserGate = Arc::new(|_req| {
            Box::pin(async {
                HostAskUserDecision::Accepted {
                    answers: json!({ "哪一块": "报名" }),
                }
            })
        });
        let mut cfg = base_cfg("http://127.0.0.1/v1".into(), Some(root), true, "hi");
        cfg.ask_user_gate = Some(gate);
        let output = dispatch_host_tool(
            &cfg,
            "ask_user_question",
            &json!({ "question": "哪一块？", "options": ["报名", "统计"] }),
            "ask-1",
            None,
            None,
        )
        .await;
        match output {
            HostToolDispatch::Output(text) => {
                assert!(text.contains("报名"), "{text}");
            }
            HostToolDispatch::Cancelled => panic!("expected answers"),
        }
    }

    #[tokio::test]
    async fn skill_save_is_disabled_during_scheduled_runs() {
        let mut cfg = base_cfg("http://127.0.0.1/v1".into(), None, false, "hi");
        cfg.allow_skill_save = false;
        let output = dispatch_host_tool(
            &cfg,
            "skill_save",
            &json!({ "action": "create", "name": "X" }),
            "skill-1",
            None,
            None,
        )
        .await;
        match output {
            HostToolDispatch::Output(text) => {
                assert!(text.contains("disabled during scheduled runs"), "{text}");
            }
            HostToolDispatch::Cancelled => panic!("expected disabled message"),
        }
    }

    #[tokio::test]
    async fn monitor_dispatches_id_and_pattern_to_the_hook() {
        let root = temp_root("monitor-dispatch");
        let mut cfg = base_cfg("http://127.0.0.1/v1".into(), Some(root.clone()), true, "hi");
        cfg.command_jobs.monitor = Some(Arc::new(|id, pattern| {
            Box::pin(async move {
                json!({ "id": id, "pattern": pattern, "status": "watching" }).to_string()
            })
        }));
        let output = dispatch_host_tool(
            &cfg,
            "monitor",
            &json!({ "id": "cmd-1", "pattern": "error" }),
            "monitor-1",
            None,
            None,
        )
        .await;
        match output {
            HostToolDispatch::Output(text) => {
                let parsed: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["id"], "cmd-1");
                assert_eq!(parsed["pattern"], "error");
            }
            HostToolDispatch::Cancelled => panic!("expected monitor output"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn monitor_requires_id_and_is_parent_only() {
        let root = temp_root("monitor-guard");
        let mut cfg = base_cfg("http://127.0.0.1/v1".into(), Some(root.clone()), true, "hi");
        cfg.command_jobs.monitor = Some(Arc::new(|id, pattern| {
            Box::pin(async move { json!({ "id": id, "pattern": pattern }).to_string() })
        }));
        let missing_id = dispatch_host_tool(&cfg, "monitor", &json!({}), "monitor-1", None, None).await;
        match missing_id {
            HostToolDispatch::Output(text) => assert!(text.contains("requires `id`"), "{text}"),
            HostToolDispatch::Cancelled => panic!("expected missing-id message"),
        }
        cfg.spawn_depth = 1;
        let from_child = dispatch_host_tool(
            &cfg,
            "monitor",
            &json!({ "id": "cmd-1" }),
            "monitor-2",
            None,
            None,
        )
        .await;
        match from_child {
            HostToolDispatch::Output(text) => {
                assert!(text.contains("only available on the parent agent"), "{text}");
            }
            HostToolDispatch::Cancelled => panic!("expected parent-only message"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_and_list_stay_inside_trusted_root() {
        let root = temp_root("tools");
        std::fs::write(root.join("README.md"), "hello from root").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();

        let listed = execute_list_directory(&root, ".").unwrap();
        assert!(listed.contains("README.md"));
        assert!(listed.contains("src"));

        let read = execute_read_file(&root, "README.md").unwrap();
        assert_eq!(read, "hello from root");

        let escape = execute_read_file(&root, "../secret.txt").unwrap_err();
        assert!(escape.contains("escapes"), "{escape}");
        let abs = execute_read_file(&root, "/etc/passwd").unwrap_err();
        assert!(
            abs.contains("absolute") || abs.contains("escapes") || abs.contains("not a file"),
            "{abs}"
        );
        assert!(!abs.contains("root:"));
        assert!(execute_tool(
            Some(&root),
            false,
            "read_file",
            &json!({"path":"README.md"})
        )
        .contains("no trusted project root"));
        assert!(execute_tool(
            Some(&root),
            true,
            "write_file",
            &json!({"path":"x.txt","content":"hi"})
        )
        .starts_with("wrote x.txt"));
        assert!(root.join("x.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_file_create_overwrite_and_parent_dir() {
        let root = temp_root("write-ok");
        let created = execute_write_file(&root, "notes.txt", "alpha").unwrap();
        assert!(created.contains("wrote notes.txt"));
        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "alpha"
        );

        execute_write_file(&root, "notes.txt", "beta").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "beta"
        );

        execute_write_file(&root, "nested/dir/file.txt", "gamma").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("nested").join("dir").join("file.txt")).unwrap(),
            "gamma"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_file_refuses_escape_absolute_dir_and_oversize() {
        let root = temp_root("write-refuse");
        let outside = root.parent().unwrap().join(format!(
            "sunsetz-agent-loop-secret-{}",
            uuid::Uuid::new_v4()
        ));
        let escape = execute_write_file(&root, "../secret.txt", "nope").unwrap_err();
        assert!(
            escape.contains("escapes") || escape.contains("absolute"),
            "{escape}"
        );
        assert!(!outside.exists());
        assert!(!root.parent().unwrap().join("secret.txt").exists());

        let abs = execute_write_file(&root, "/etc/passwd", "nope").unwrap_err();
        assert!(abs.contains("absolute") || abs.contains("escapes"), "{abs}");

        std::fs::create_dir(root.join("dir")).unwrap();
        let dir_err = execute_write_file(&root, "dir", "nope").unwrap_err();
        assert!(
            dir_err.contains("not a file") || dir_err.contains("empty"),
            "{dir_err}"
        );

        let huge = "x".repeat((fs_browser::MAX_TEXT_BYTES as usize) + 1);
        let oversize = execute_write_file(&root, "big.txt", &huge).unwrap_err();
        assert!(oversize.contains("too large"), "{oversize}");
        assert!(!root.join("big.txt").exists());

        let empty = execute_write_file(&root, "", "x").unwrap_err();
        assert!(empty.contains("empty"), "{empty}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_file_prepare_preview_has_no_body() {
        let root = temp_root("write-preview");
        let prepared = prepare_write_file(
            &root,
            &json!({"path":"src/a.rs","content":"fn secret() {}"}),
        )
        .unwrap();
        assert_eq!(prepared.preview, "src/a.rs (14 bytes)");
        assert!(!prepared.preview.contains("secret"));
        assert!(!prepared.path_target.contains("fn secret"));
        assert_eq!(prepared.title, "Write src/a.rs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn grep_finds_hits_inside_root_and_skips_build_dirs() {
        let root = temp_root("grep-ok");
        write_file(&root, "src/main.rs", "fn alpha() {}\nfn beta() {}\n");
        write_file(&root, "node_modules/secret.rs", "fn alpha() {}\n");
        write_file(&root, "target/debug/out.rs", "fn alpha() {}\n");
        let hits = execute_grep(&root, &json!({"pattern": "alpha"})).unwrap();
        assert!(hits.contains("src/main.rs:1:fn alpha() {}"), "{hits}");
        assert!(!hits.contains("node_modules"), "{hits}");
        assert!(!hits.contains("target/"), "{hits}");
        let none = execute_grep(&root, &json!({"pattern": "zzz-missing"})).unwrap();
        assert_eq!(none, "no matches");
        let case = execute_grep(
            &root,
            &json!({"pattern": "ALPHA", "case_insensitive": true}),
        )
        .unwrap();
        assert!(case.contains("src/main.rs:1:"), "{case}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn grep_refuses_escape_and_invalid_pattern() {
        let root = temp_root("grep-refuse");
        write_file(&root, "a.rs", "hello");
        let escape = execute_grep(&root, &json!({"pattern": "hello", "path": "../"})).unwrap_err();
        assert!(
            escape.contains("escapes") || escape.contains("absolute") || escape.contains("project"),
            "{escape}"
        );
        let bad = execute_grep(&root, &json!({"pattern": "("})).unwrap_err();
        assert!(bad.contains("invalid grep pattern"), "{bad}");
        assert!(
            execute_tool(Some(&root), false, "grep", &json!({"pattern": "hello"}))
                .contains("no trusted project root")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_replace_updates_unique_match_and_replace_all() {
        let root = temp_root("replace-ok");
        write_file(&root, "src/a.rs", "alpha\nalpha\nbeta\n");
        let unique_err = execute_search_replace(
            &root,
            &json!({"path":"src/a.rs","old_string":"alpha","new_string":"gamma"}),
        )
        .unwrap_err();
        assert!(unique_err.contains("2 times"), "{unique_err}");
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "alpha\nalpha\nbeta\n"
        );

        let updated = execute_search_replace(
            &root,
            &json!({
                "path":"src/a.rs",
                "old_string":"alpha",
                "new_string":"gamma",
                "replace_all": true
            }),
        )
        .unwrap();
        assert!(updated.contains("updated src/a.rs"), "{updated}");
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "gamma\ngamma\nbeta\n"
        );

        let once = execute_search_replace(
            &root,
            &json!({"path":"src/a.rs","old_string":"beta","new_string":"delta"}),
        )
        .unwrap();
        assert!(once.contains("updated"), "{once}");
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "gamma\ngamma\ndelta\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_replace_refuses_escape_missing_and_preview_has_no_body() {
        let root = temp_root("replace-refuse");
        write_file(&root, "src/a.rs", "secret_token = 1\n");
        let missing = execute_search_replace(
            &root,
            &json!({"path":"src/a.rs","old_string":"nope","new_string":"x"}),
        )
        .unwrap_err();
        assert!(missing.contains("not found"), "{missing}");

        let escape = execute_search_replace(
            &root,
            &json!({"path":"../secret.txt","old_string":"a","new_string":"b"}),
        )
        .unwrap_err();
        assert!(
            escape.contains("escapes") || escape.contains("absolute"),
            "{escape}"
        );

        let prepared = prepare_search_replace(
            &root,
            &json!({"path":"src/a.rs","old_string":"secret_token = 1","new_string":"ok = 1"}),
        )
        .unwrap();
        assert_eq!(prepared.preview, "src/a.rs (1 replacement(s))");
        assert!(!prepared.preview.contains("secret_token"));
        assert!(!prepared.path_target.contains("secret_token"));
        assert_eq!(prepared.title, "Replace src/a.rs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn run_command_succeeds_in_root_and_refuses_escape_cwd() {
        let root = temp_root("run-ok");
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("marker.txt"), "here").unwrap();
        let echo = prepare_run_command(&root, &json!({"command":"echo sunsetz-host"})).unwrap();
        match execute_run_command(
            &echo.command,
            &echo.cwd_canon,
            Arc::new(AtomicBool::new(false)),
            runtime_compat::SandboxProfileV1::Off,
            &root,
        )
        .await
        .unwrap()
        {
            RunCommandOutcome::Output(text) => assert!(text.contains("sunsetz-host"), "{text}"),
            RunCommandOutcome::Cancelled => panic!("echo cancelled"),
        }

        let escaped = prepare_run_command(&root, &json!({"command":"pwd","cwd":".."})).unwrap_err();
        assert!(
            escaped.contains("escapes") || escaped.contains("absolute"),
            "{escaped}"
        );
        let abs = prepare_run_command(&root, &json!({"command":"pwd","cwd":"/tmp"})).unwrap_err();
        assert!(abs.contains("absolute") || abs.contains("escapes"), "{abs}");
        let empty = prepare_run_command(&root, &json!({"command":"  "})).unwrap_err();
        assert!(empty.contains("empty"), "{empty}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "macos")]
    fn sandbox_project_root(label: &str) -> PathBuf {
        let home = std::env::var("HOME").expect("HOME");
        let path = PathBuf::from(home)
            .join("Library/Caches/sunsetz-command-sandbox-tests")
            .join(format!("agent-loop-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn run_command_read_only_sandbox_blocks_project_write() {
        let root = sandbox_project_root("run-ro");
        let prepared =
            prepare_run_command(&root, &json!({"command":"printf blocked > inside.txt"})).unwrap();
        match execute_run_command(
            &prepared.command,
            &prepared.cwd_canon,
            Arc::new(AtomicBool::new(false)),
            runtime_compat::SandboxProfileV1::ReadOnly,
            &root,
        )
        .await
        .unwrap()
        {
            RunCommandOutcome::Output(text) => {
                assert!(
                    !root.join("inside.txt").exists(),
                    "read_only wrote the project file: {text}"
                );
            }
            RunCommandOutcome::Cancelled => panic!("read_only command cancelled"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn run_command_timeout_kills_child() {
        let root = temp_root("run-timeout");
        let hang = if cfg!(windows) {
            "ping -n 30 127.0.0.1"
        } else {
            "sleep 30"
        };
        let prepared = prepare_run_command(&root, &json!({"command": hang})).unwrap();
        let started = std::time::Instant::now();
        let err = execute_run_command_timed(
            &prepared.command,
            &prepared.cwd_canon,
            Arc::new(AtomicBool::new(false)),
            Duration::from_millis(400),
            runtime_compat::SandboxProfileV1::Off,
            &root,
            None,
        )
        .await
        .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(8));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn execute_run_command_streams_lines_to_on_line() {
        let root = temp_root("run-stream-lines");
        let multi = if cfg!(windows) {
            "echo one&echo two&echo three"
        } else {
            "printf 'one\\ntwo\\nthree\\n'"
        };
        let prepared = prepare_run_command(&root, &json!({"command": multi})).unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let on_line: CommandLineFn = Arc::new(move |line: String| {
            let sink = Arc::clone(&sink);
            Box::pin(async move {
                sink.lock().unwrap().push(line);
            })
        });
        let outcome = execute_run_command_timed(
            &prepared.command,
            &prepared.cwd_canon,
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(10),
            runtime_compat::SandboxProfileV1::Off,
            &root,
            Some(on_line),
        )
        .await
        .unwrap();
        match outcome {
            RunCommandOutcome::Output(text) => {
                assert!(text.contains("one") && text.contains("two") && text.contains("three"));
            }
            RunCommandOutcome::Cancelled => panic!("stream command cancelled"),
        }
        let lines = seen.lock().unwrap().clone();
        assert_eq!(lines, vec!["one", "two", "three"], "{lines:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn command_timeout_default_is_fifteen_minutes() {
        assert_eq!(COMMAND_TIMEOUT_SECS, 15 * 60);
    }

    #[test]
    fn journal_history_drops_current_user_and_markers() {
        let now = chrono::Utc::now();
        let history = chat_history_from_journal(&[
            ChatMessageStored {
                id: "u1".into(),
                role: "user".into(),
                content: "first".into(),
                thought: None,
                created_at: now,
                is_error: false,
                attachments: None,
                marker: None,
            },
            ChatMessageStored {
                id: "a1".into(),
                role: "assistant".into(),
                content: "ok".into(),
                thought: None,
                created_at: now,
                is_error: false,
                attachments: None,
                marker: None,
            },
            ChatMessageStored {
                id: "t1".into(),
                role: "tool".into(),
                content: "ignored".into(),
                thought: None,
                created_at: now,
                is_error: false,
                attachments: None,
                marker: Some("tool_step".into()),
            },
            ChatMessageStored {
                id: "u2".into(),
                role: "user".into(),
                content: "current".into(),
                thought: None,
                created_at: now,
                is_error: false,
                attachments: None,
                marker: None,
            },
        ]);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["content"], "first");
        assert_eq!(history[1]["content"], "ok");
    }

    #[tokio::test]
    async fn streams_assistant_text_from_mock_openai() {
        let (base, server) = spawn_mock_llm(vec![sse_text(&["Hello", " world"])]).await;
        let events = collect_events(base_cfg(base, None, false, "hi")).await;
        server.abort();
        let texts: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::Stream { text, .. } if !text.is_empty() => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts.concat(), "Hello world");
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::PromptComplete { stop_reason } if stop_reason == "end_turn"
        )));
    }

    #[tokio::test]
    async fn emits_last_inference_usage_from_stream() {
        let (base, server) =
            spawn_mock_llm(vec![sse_text_with_usage(&["Hello"], 34128, 641)]).await;
        let events = collect_events(base_cfg(base, None, false, "hi")).await;
        server.abort();
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::Usage {
                input_tokens: 34128,
                output_tokens: 641,
                cached_read_tokens: 4,
                reasoning_tokens: 1,
                model_calls: 1,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn complete_text_collects_summary_without_tools() {
        let (base, server) = spawn_mock_llm(vec![sse_text(&["Auth is in ", "session.ts."])]).await;
        let text = complete_text(
            &http_client().unwrap(),
            &LlmEndpoint {
                base_url: base,
                api_key: "test-key".into(),
                model: "grok-4.5".into(),
            },
            &[json!({ "role": "user", "content": "summarize" })],
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        server.abort();
        assert_eq!(text, "Auth is in session.ts.");
    }

    #[tokio::test]
    async fn connector_tools_run_without_trusted_project() {
        let (tool, answer) = sse_tool_then(
            "github_list_pull_requests",
            r#"{"owner":"acme","repo":"app"}"#,
            &["found the PR"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let called = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let called_invoke = Arc::clone(&called);
        let mut cfg = base_cfg(base, None, false, "list prs");
        cfg.permission_gate = Some(allow_gate());
        cfg.connectors.tools = vec![json!({
            "type": "function",
            "function": {
                "name": "github_list_pull_requests",
                "parameters": { "type": "object" }
            }
        })];
        cfg.connectors.explicit = vec!["github".into()];
        cfg.connectors.invoke = Some(Arc::new(move |name, _arguments| {
            let called_invoke = Arc::clone(&called_invoke);
            Box::pin(async move {
                called_invoke.lock().unwrap().push(name);
                r#"[{"number":7,"title":"Ready"}]"#.into()
            })
        }));
        let events = collect_events(cfg).await;
        server.abort();
        assert_eq!(
            called.lock().unwrap().as_slice(),
            ["github_list_pull_requests"]
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, title, .. }
                if kind == "fetch" && status == "completed" && title.contains("github_list_pull_requests")
        )));
    }

    #[tokio::test]
    async fn connector_write_denied_does_not_invoke() {
        let (tool, answer) = sse_tool_then(
            "github_create_issue",
            r#"{"owner":"acme","repo":"app","title":"Bug"}"#,
            &["denied"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let called = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let hits = Arc::clone(&called);
        let mut cfg = base_cfg(base, None, false, "open issue");
        cfg.permission_gate = Some(deny_gate());
        cfg.connectors.tools = vec![json!({
            "type": "function",
            "function": { "name": "github_create_issue", "parameters": { "type": "object" } }
        })];
        cfg.connectors
            .write_tools
            .insert("github_create_issue".into());
        cfg.connectors.invoke = Some(Arc::new(move |_, _| {
            hits.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { "should not run".into() })
        }));
        let _events = collect_events(cfg).await;
        server.abort();
        assert_eq!(called.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn executes_read_file_then_answers() {
        let root = temp_root("read-turn");
        std::fs::write(root.join("notes.txt"), "alpha").unwrap();
        let (tool, answer) =
            sse_tool_then("read_file", r#"{"path":"notes.txt"}"#, &["file says alpha"]);
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let events = collect_events(base_cfg(base, Some(root.clone()), true, "read notes")).await;
        server.abort();
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { title, status, .. }
                if title.contains("notes.txt") && status == "completed"
        )));
        let texts: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::Stream { text, .. } if !text.is_empty() => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.concat().contains("alpha"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn list_directory_tool_round_uses_trusted_root() {
        let root = temp_root("list-turn");
        std::fs::write(root.join("a.rs"), "a").unwrap();
        let (tool, answer) = sse_tool_then("list_directory", r#"{"path":"."}"#, &["listed a.rs"]);
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let events = collect_events(base_cfg(base, Some(root.clone()), true, "list")).await;
        server.abort();
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, .. } if kind == "list" && status == "completed"
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn tool_without_trusted_root_does_not_read_disk() {
        let root = temp_root("untrusted");
        std::fs::write(root.join("secret.txt"), "nope").unwrap();
        let (tool, answer) = sse_tool_then("read_file", r#"{"path":"secret.txt"}"#, &["refused"]);
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let events = collect_events(base_cfg(base, Some(root.clone()), false, "read")).await;
        server.abort();
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { status, .. } if status == "completed"
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn stop_flag_ends_turn_without_prompt_complete() {
        let (base, server) = spawn_mock_llm(vec![sse_text(&["zzzzzzzzzz"])]).await;
        let cfg = base_cfg(base, None, false, "hi");
        cfg.stop.store(true, Ordering::SeqCst);
        let events = collect_events(cfg).await;
        server.abort();
        assert!(!events
            .iter()
            .any(|event| matches!(event, AcpEvent::PromptComplete { .. })));
    }

    #[tokio::test]
    async fn write_file_turn_allows_then_answers() {
        let root = temp_root("write-turn");
        let (tool, answer) = sse_tool_then(
            "write_file",
            r#"{"path":"out.txt","content":"PERM_OK"}"#,
            &["wrote the file"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "write out");
        cfg.permission_gate = Some(allow_gate());
        let events = collect_events(cfg).await;
        server.abort();
        assert_eq!(
            std::fs::read_to_string(root.join("out.txt")).unwrap(),
            "PERM_OK"
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, title, .. }
                if kind == "edit" && status == "completed" && title.contains("out.txt")
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_file_deny_does_not_touch_disk() {
        let root = temp_root("write-deny");
        let (tool, answer) = sse_tool_then(
            "write_file",
            r#"{"path":"out.txt","content":"NOPE"}"#,
            &["denied"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "write out");
        cfg.permission_gate = Some(deny_gate());
        let _events = collect_events(cfg).await;
        server.abort();
        assert!(!root.join("out.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_file_escape_does_not_open_dock_or_create_file() {
        let root = temp_root("write-escape-turn");
        let hits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (tool, answer) = sse_tool_then(
            "write_file",
            r#"{"path":"../secret.txt","content":"NOPE"}"#,
            &["refused"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "write escape");
        cfg.permission_gate = Some(counting_gate(
            Arc::clone(&hits),
            HostToolPermissionDecision::Allow,
        ));
        let _events = collect_events(cfg).await;
        server.abort();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!root.parent().unwrap().join("secret.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_file_untrusted_does_not_open_dock() {
        let root = temp_root("write-untrusted");
        let hits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (tool, answer) = sse_tool_then(
            "write_file",
            r#"{"path":"out.txt","content":"NOPE"}"#,
            &["refused"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), false, "write");
        cfg.permission_gate = Some(counting_gate(
            Arc::clone(&hits),
            HostToolPermissionDecision::Allow,
        ));
        let _events = collect_events(cfg).await;
        server.abort();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!root.join("out.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_file_missing_gate_is_fail_closed() {
        let root = temp_root("write-no-gate");
        let (tool, answer) = sse_tool_then(
            "write_file",
            r#"{"path":"out.txt","content":"NOPE"}"#,
            &["denied"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let events = collect_events(base_cfg(base, Some(root.clone()), true, "write")).await;
        server.abort();
        assert!(!root.join("out.txt").exists());
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { status, .. } if status == "completed"
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_file_preview_never_includes_body() {
        let root = temp_root("write-preview-turn");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (tool, answer) = sse_tool_then(
            "write_file",
            r#"{"path":"a.rs","content":"fn secret_token() {}"}"#,
            &["ok"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "write");
        cfg.permission_gate = Some(recording_gate(
            Arc::clone(&seen),
            HostToolPermissionDecision::Deny,
        ));
        let events = collect_events(cfg).await;
        server.abort();
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].preview.contains("bytes"));
        assert!(!reqs[0].preview.contains("secret_token"));
        assert!(!reqs[0].path_target.contains("secret_token"));
        for event in &events {
            if let AcpEvent::ToolCall { raw, .. } = event {
                let dumped = raw.to_string();
                assert!(!dumped.contains("secret_token"), "{dumped}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn stream_idle_timeout_fails_without_total_request_cap() {
        let (base, server) = spawn_stalling_llm(Duration::from_secs(3)).await;
        let mut cfg = base_cfg(base, None, false, "hi");
        cfg.stream_idle = Duration::from_millis(250);
        let started = std::time::Instant::now();
        let events = collect_events(cfg).await;
        server.abort();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::Error { error }
                if error.code == AgentErrorCode::NetworkProvider
                    && error.message.contains("stalled")
        )));
        assert!(!events
            .iter()
            .any(|event| matches!(event, AcpEvent::PromptComplete { .. })));
    }

    #[tokio::test]
    async fn tool_budget_completes_instead_of_crashing() {
        let root = temp_root("tool-budget");
        write_file(&root, "src/lib.rs", "pub fn marker() {}\n");
        let (tool, _answer) = sse_tool_then(
            "grep",
            r#"{"pattern":"marker"}"#,
            &["should not be needed"],
        );
        let (base, server) = spawn_mock_llm(vec![tool.clone(), tool]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "search");
        cfg.max_tool_rounds = 2;
        let events = collect_events(cfg).await;
        server.abort();
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::PromptComplete { stop_reason } if stop_reason == TOOL_BUDGET_STOP_REASON
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            AcpEvent::Error { error } if error.code == AgentErrorCode::AgentCrashed
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::Stream { text, done: false, .. }
                if text.contains("2 tool rounds")
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sse_repeated_tool(
        name: &str,
        arguments: &str,
        repeats: usize,
        answer_parts: &[&str],
    ) -> Vec<String> {
        let (tool, answer) = sse_tool_then(name, arguments, answer_parts);
        let mut out = Vec::with_capacity(repeats + 1);
        for _ in 0..repeats {
            out.push(tool.clone());
        }
        out.push(answer);
        out
    }

    #[tokio::test]
    async fn spawn_agent_parent_calls_host_and_child_depth_fails() {
        let spawned = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let hook_count = Arc::clone(&spawned);
        let (tool, answer) = sse_tool_then(
            "spawn_agent",
            r#"{"prompt":"look","description":"search repo","agent_type":"explore"}"#,
            &["spawned"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, None, false, "spawn");
        cfg.subagents.spawn = Some(Arc::new(move |req| {
            hook_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                format!(
                    "{{\"id\":\"child-1\",\"status\":\"completed\",\"agentType\":\"{}\",\"description\":\"{}\",\"summary\":\"ok\"}}",
                    req.agent_type, req.description
                )
            })
        }));
        let events = collect_events(cfg).await;
        server.abort();
        assert_eq!(spawned.load(Ordering::SeqCst), 1);
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, .. } if kind == "agent"
        )));

        let (tool2, answer2) = sse_tool_then(
            "spawn_agent",
            r#"{"prompt":"look","description":"nope"}"#,
            &["refused"],
        );
        let (base2, server2) = spawn_mock_llm(vec![tool2, answer2]).await;
        let mut child = base_cfg(base2, None, false, "nested");
        child.kind = AgentKind::Explore;
        child.spawn_depth = 1;
        child.subagents.spawn = Some(Arc::new(move |_req| {
            Box::pin(async move { "should not run".into() })
        }));
        let events2 = collect_events(child).await;
        server2.abort();
        // Child configs do not advertise spawn_agent, so the invented call is
        // rejected by depth/kind before the host hook runs.
        let _ = events2;
    }

    #[tokio::test]
    async fn grep_turn_searches_trusted_root() {
        let root = temp_root("grep-turn");
        write_file(&root, "src/lib.rs", "pub fn host_grep_marker() {}\n");
        let (tool, answer) = sse_tool_then(
            "grep",
            r#"{"pattern":"host_grep_marker"}"#,
            &["found the marker"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let events = collect_events(base_cfg(base, Some(root.clone()), true, "search")).await;
        server.abort();
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, title, .. }
                if kind == "search" && status == "completed" && title.contains("host_grep_marker")
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn search_replace_turn_allows_then_answers() {
        let root = temp_root("replace-turn");
        write_file(&root, "out.txt", "OLD");
        let (tool, answer) = sse_tool_then(
            "search_replace",
            r#"{"path":"out.txt","old_string":"OLD","new_string":"NEW"}"#,
            &["replaced it"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "replace");
        cfg.permission_gate = Some(allow_gate());
        let events = collect_events(cfg).await;
        server.abort();
        assert_eq!(
            std::fs::read_to_string(root.join("out.txt")).unwrap(),
            "NEW"
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, title, .. }
                if kind == "edit" && status == "completed" && title.contains("out.txt")
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn search_replace_deny_does_not_touch_disk() {
        let root = temp_root("replace-deny");
        write_file(&root, "out.txt", "OLD");
        let (tool, answer) = sse_tool_then(
            "search_replace",
            r#"{"path":"out.txt","old_string":"OLD","new_string":"NEW"}"#,
            &["denied"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "replace");
        cfg.permission_gate = Some(deny_gate());
        let _events = collect_events(cfg).await;
        server.abort();
        assert_eq!(
            std::fs::read_to_string(root.join("out.txt")).unwrap(),
            "OLD"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn search_replace_escape_does_not_open_dock() {
        let root = temp_root("replace-escape-turn");
        let hits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (tool, answer) = sse_tool_then(
            "search_replace",
            r#"{"path":"../secret.txt","old_string":"a","new_string":"b"}"#,
            &["refused"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "replace escape");
        cfg.permission_gate = Some(counting_gate(
            Arc::clone(&hits),
            HostToolPermissionDecision::Allow,
        ));
        let _events = collect_events(cfg).await;
        server.abort();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!root.parent().unwrap().join("secret.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn search_replace_preview_never_includes_body() {
        let root = temp_root("replace-preview-turn");
        write_file(&root, "a.rs", "fn secret_token() {}");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (tool, answer) = sse_tool_then(
            "search_replace",
            r#"{"path":"a.rs","old_string":"fn secret_token() {}","new_string":"fn ok() {}"}"#,
            &["ok"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "replace");
        cfg.permission_gate = Some(recording_gate(
            Arc::clone(&seen),
            HostToolPermissionDecision::Deny,
        ));
        let events = collect_events(cfg).await;
        server.abort();
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].preview.contains("replacement"));
        assert!(!reqs[0].preview.contains("secret_token"));
        for event in &events {
            if let AcpEvent::ToolCall { raw, .. } = event {
                let dumped = raw.to_string();
                assert!(!dumped.contains("secret_token"), "{dumped}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn identical_tool_loop_stops_before_third_repeat() {
        let root = temp_root("loop-guard");
        write_file(&root, "notes.txt", "alpha");
        let hits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let responses = sse_repeated_tool(
            "write_file",
            r#"{"path":"out.txt","content":"same"}"#,
            3,
            &["stopped looping"],
        );
        let (base, server) = spawn_mock_llm(responses).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "write same");
        cfg.permission_gate = Some(counting_gate(
            Arc::clone(&hits),
            HostToolPermissionDecision::Allow,
        ));
        let events = collect_events(cfg).await;
        server.abort();
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert_eq!(
            std::fs::read_to_string(root.join("out.txt")).unwrap(),
            "same"
        );
        let texts: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::Stream { text, .. } if !text.is_empty() => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.concat().contains("stopped looping"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn run_command_turn_allows_then_answers() {
        let root = temp_root("run-turn");
        let (tool, answer) =
            sse_tool_then("run_command", r#"{"command":"echo host-ok"}"#, &["ran it"]);
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "run");
        cfg.permission_gate = Some(allow_gate());
        let events = collect_events(cfg).await;
        server.abort();
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, .. }
                if kind == "execute" && status == "completed"
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn list_skills_turn_does_not_require_permission() {
        let root = temp_root("list-skills");
        let (tool, answer) = sse_tool_then("list_skills", r#"{}"#, &["listed"]);
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "skills");
        let hits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        cfg.permission_gate = Some(counting_gate(
            Arc::clone(&hits),
            HostToolPermissionDecision::Allow,
        ));
        let events = collect_events(cfg).await;
        server.abort();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::ToolCall { kind, status, title, .. }
                if kind == "read" && status == "completed" && title == "List Skills"
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn stop_during_permission_wait_does_not_write() {
        let root = temp_root("write-stop");
        let started = Arc::new(tokio::sync::Notify::new());
        let stop_flag = Arc::new(AtomicBool::new(false));
        let gate: HostToolPermissionGate = {
            let started = Arc::clone(&started);
            let stop_flag = Arc::clone(&stop_flag);
            Arc::new(move |_req| {
                let started = Arc::clone(&started);
                let stop_flag = Arc::clone(&stop_flag);
                Box::pin(async move {
                    started.notify_one();
                    wait_until_stopped(stop_flag).await;
                    HostToolPermissionDecision::Cancelled
                })
            })
        };
        let (tool, answer) = sse_tool_then(
            "write_file",
            r#"{"path":"out.txt","content":"NOPE"}"#,
            &["should not run"],
        );
        let (base, server) = spawn_mock_llm(vec![tool, answer]).await;
        let mut cfg = base_cfg(base, Some(root.clone()), true, "write");
        cfg.stop = Arc::clone(&stop_flag);
        cfg.permission_gate = Some(gate);
        let turn = tokio::spawn(collect_events(cfg));
        started.notified().await;
        stop_flag.store(true, Ordering::SeqCst);
        let events = turn.await.unwrap();
        server.abort();
        assert!(!root.join("out.txt").exists());
        assert!(!events
            .iter()
            .any(|event| matches!(event, AcpEvent::PromptComplete { .. })));
        let _ = std::fs::remove_dir_all(&root);
    }
}
