//! Sunsetz-owned agent kernel.
//!
//! The product runtime is this in-process loop, not a spawned `grok agent stdio`
//! process. It sends the user prompt to an OpenAI-compatible chat endpoint using
//! existing providers/secrets, executes Host-owned `read_file`, `list_directory`,
//! `write_file`, and `run_command` inside a trusted project root, and emits the
//! same `AcpEvent` surface the session UI already consumes.
//!
//! Writes and commands wait on the Host permission dock (or an explicit
//! auto-allow policy) before they run. Reads stay immediate inside a trusted
//! root. This slice is unsandboxed: no bubblewrap / seatbelt / job object.
//!
//! Out of scope this slice: Hermes skills, memory injection, cron, plugins,
//! sandbox. The Grok ACP adapter stays behind an explicit legacy flag.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::acp_client::{AcpEvent, StreamKind};
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

const MAX_TOOL_ROUNDS: u32 = 8;
const MAX_FILE_CHARS: usize = 32_768;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_HISTORY_CHARS: usize = 32_768;
const HTTP_TIMEOUT_SECS: u64 = 120;
const COMMAND_TIMEOUT_SECS: u64 = 60;

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
    pub permission_gate: Option<HostToolPermissionGate>,
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

/// Resolve the product kernel vs legacy ACP adapter vs in-process mock.
pub fn resolve_backend(settings_backend: &str, env_backend: Option<&str>, mock: bool) -> String {
    if mock {
        return BACKEND_MOCK.into();
    }
    if let Some(env) = env_backend.map(str::trim).filter(|s| !s.is_empty()) {
        let parsed = parse_runtime_backend(env);
        if parsed == SETTING_LEGACY_GROK_ACP {
            return BACKEND_GROK_ACP.into();
        }
        if parsed == BACKEND_MOCK {
            return BACKEND_MOCK.into();
        }
        return BACKEND_SUNSETZ.into();
    }
    let parsed = parse_runtime_backend(settings_backend);
    if parsed == SETTING_LEGACY_GROK_ACP {
        BACKEND_GROK_ACP.into()
    } else if parsed == BACKEND_MOCK {
        BACKEND_MOCK.into()
    } else {
        BACKEND_SUNSETZ.into()
    }
}

pub fn current_backend() -> String {
    resolve_backend(
        &store::load_settings().runtime_backend,
        std::env::var(runtime_compat::PRODUCT_RUNTIME_BACKEND_ENV)
            .ok()
            .as_deref(),
        crate::acp_client::AcpClient::use_mock(),
    )
}

pub fn use_sunsetz_kernel() -> bool {
    is_sunsetz_backend(&current_backend())
}

pub fn use_legacy_grok_acp() -> bool {
    is_legacy_grok_backend(&current_backend())
}

pub fn http_client() -> Result<reqwest::Client, AgentError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
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
    let mut kept: Vec<&ChatMessageStored> = messages
        .iter()
        .filter(|message| {
            if message.is_error || message.content.trim().is_empty() {
                return false;
            }
            if let Some(marker) = message.marker.as_deref() {
                if marker == "tool_step"
                    || marker == "turn_cancelled"
                    || marker.starts_with("memory")
                {
                    return false;
                }
            }
            message.role == "user" || message.role == "assistant"
        })
        .collect();
    if kept.last().is_some_and(|message| message.role == "user") {
        kept.pop();
    }
    if kept.len() > MAX_HISTORY_MESSAGES {
        kept = kept
            .into_iter()
            .rev()
            .take(MAX_HISTORY_MESSAGES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }
    let mut out = Vec::new();
    let mut chars = 0usize;
    for message in kept {
        let content: String = message.content.chars().take(4_000).collect();
        chars = chars.saturating_add(content.len());
        if chars > MAX_HISTORY_CHARS && !out.is_empty() {
            break;
        }
        out.push(json!({
            "role": message.role,
            "content": content,
        }));
    }
    out
}

pub fn system_prompt(project_root: Option<&Path>, trusted: bool) -> String {
    let mut prompt = String::from(
        "You are Sunsetz Runtime, the built-in agent kernel of the Sunsetz desktop workbench. \
         Answer the user directly. You may call read_file, list_directory, write_file, and \
         run_command only inside the trusted project root. Writes and commands require user \
         permission. You cannot access paths outside that root. run_command is unsandboxed \
         except for the trusted-root cwd pin, permission gate, and a 60s timeout. \
         The user may attach reviewed Skill text for this turn; do not invent or auto-load Skills.",
    );
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
    prompt
}

pub fn tool_definitions() -> Value {
    json!([
        {
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
        },
        {
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
        },
        {
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
        },
        {
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Run a shell command with cwd pinned to a directory inside the trusted project root. Unsandboxed except for the cwd pin, permission gate, and 60s timeout. Requires permission; AcceptEdits does not auto-allow this tool.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Exact command string to run." },
                        "cwd": { "type": "string", "description": "Optional relative directory under the trusted project root. Defaults to the root." }
                    },
                    "required": ["command"]
                }
            }
        }
    ])
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
enum RunCommandOutcome {
    Output(String),
    Cancelled,
}

async fn execute_run_command(
    command: &str,
    cwd: &Path,
    stop: Arc<AtomicBool>,
) -> Result<RunCommandOutcome, String> {
    execute_run_command_timed(
        command,
        cwd,
        stop,
        Duration::from_secs(COMMAND_TIMEOUT_SECS),
    )
    .await
}

async fn execute_run_command_timed(
    command: &str,
    cwd: &Path,
    stop: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<RunCommandOutcome, String> {
    if command.trim().is_empty() {
        return Err("empty command".into());
    }
    if command.contains('\0') {
        return Err("invalid command".into());
    }
    let mut cmd = if cfg!(windows) {
        let mut spawned = tokio::process::Command::new("cmd.exe");
        spawned.arg("/C").arg(command);
        process_util::apply_no_window_tokio(&mut spawned);
        spawned
    } else {
        let mut spawned = tokio::process::Command::new("/bin/sh");
        spawned.arg("-lc").arg(command);
        spawned
    };
    cmd.current_dir(cwd)
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
    let read_out = async {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf).await;
        buf
    };
    let read_err = async {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        buf
    };
    tokio::select! {
        _ = wait_until_stopped(Arc::clone(&stop)) => {
            kill_run_command_child(&mut child);
            let _ = child.wait().await;
            Ok(RunCommandOutcome::Cancelled)
        }
        _ = tokio::time::sleep(timeout) => {
            kill_run_command_child(&mut child);
            let _ = child.wait().await;
            Err(format!("command timed out after {COMMAND_TIMEOUT_SECS}s"))
        }
        joined = async {
            let status = child.wait().await;
            let (out, err) = tokio::join!(read_out, read_err);
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
    if !matches!(
        name,
        "read_file" | "list_directory" | "write_file" | "run_command"
    ) {
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
        "write_file" => match tool_content_arg(arguments) {
            Ok(content) => execute_write_file(root, &tool_write_path_arg(arguments), &content)
                .unwrap_or_else(|error| error),
            Err(error) => error,
        },
        "run_command" => "run_command requires the async Host turn".into(),
        _ => format!("unknown tool `{name}`"),
    }
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
    matches!(name, "write_file" | "run_command")
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

async fn dispatch_host_tool(
    cfg: &AgentTurnConfig,
    name: &str,
    arguments: &Value,
    tool_call_id: &str,
    prepared_write: Option<Result<PreparedWrite, String>>,
    prepared_command: Option<Result<PreparedCommand, String>>,
) -> HostToolDispatch {
    if !matches!(
        name,
        "read_file" | "list_directory" | "write_file" | "run_command"
    ) {
        return HostToolDispatch::Output(format!("unknown tool `{name}`"));
    }
    if !cfg.trusted || cfg.project_root.is_none() {
        return HostToolDispatch::Output("no trusted project root".into());
    }
    let root = cfg.project_root.as_deref().unwrap();

    if name == "write_file" {
        let prepared = match prepared_write.unwrap_or_else(|| prepare_write_file(root, arguments)) {
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
        return HostToolDispatch::Output(
            execute_write_file(root, &prepared.rel, &prepared.content)
                .unwrap_or_else(|error| error),
        );
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
        return match execute_run_command(
            &prepared.command,
            &prepared.cwd_canon,
            Arc::clone(&cfg.stop),
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
    let tools_enabled = cfg.trusted && cfg.project_root.is_some();
    let mut messages = vec![json!({
        "role": "system",
        "content": system_prompt(cfg.project_root.as_deref(), cfg.trusted),
    })];
    messages.extend(cfg.history.clone());
    messages.push(json!({
        "role": "user",
        "content": cfg.user_prompt,
    }));

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
                tools_enabled,
                &cfg.stop,
                &mut emit,
            ) => outcome,
        };
        match outcome {
            Ok(ChatOutcome::Text) => {
                if !cfg.stop.load(Ordering::SeqCst) {
                    emit(AcpEvent::PromptComplete {
                        stop_reason: "end_turn".into(),
                    });
                }
                return;
            }
            Ok(ChatOutcome::ToolCalls(calls)) => {
                if calls.is_empty() {
                    emit(AcpEvent::PromptComplete {
                        stop_reason: "end_turn".into(),
                    });
                    return;
                }
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
                    let prepared_write =
                        if call.name == "write_file" && cfg.trusted && cfg.project_root.is_some() {
                            cfg.project_root
                                .as_deref()
                                .map(|root| prepare_write_file(root, &args_value))
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
                        "write_file" => prepared_write
                            .as_ref()
                            .and_then(|prepared| prepared.as_ref().ok())
                            .map(|prepared| prepared.title.clone())
                            .unwrap_or_else(|| {
                                format!("Write {}", display_rel(&tool_write_path_arg(&args_value)))
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
                        other => other.to_string(),
                    };
                    let kind = match call.name.as_str() {
                        "read_file" => "read",
                        "list_directory" => "list",
                        "write_file" => "edit",
                        "run_command" => "execute",
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
                        "run_command" => json!({
                            "command": tool_command_arg(&args_value),
                            "cwd": display_rel(&tool_cwd_arg(&args_value)),
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
                    let output = match dispatch_host_tool(
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
                    };
                    if cfg.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    emit(AcpEvent::ToolCall {
                        tool_call_id: id.clone(),
                        title,
                        kind: kind.into(),
                        status: "completed".into(),
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
    emit(AcpEvent::Error {
        error: AgentError::new(
            AgentErrorCode::AgentCrashed,
            "Sunsetz Runtime stopped after too many tool rounds",
        ),
    });
}

enum ChatOutcome {
    Text,
    ToolCalls(Vec<PendingToolCall>),
}

async fn stream_chat_completion<F>(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    messages: &[Value],
    tools_enabled: bool,
    stop: &AtomicBool,
    emit: &mut F,
) -> Result<ChatOutcome, AgentError>
where
    F: FnMut(AcpEvent) + Send,
{
    let url = chat_completions_url(&endpoint.base_url);
    let mut body = json!({
        "model": endpoint.model,
        "messages": messages,
        "stream": true,
    });
    if tools_enabled {
        body["tools"] = tool_definitions();
        body["tool_choice"] = json!("auto");
        body["parallel_tool_calls"] = json!(false);
    }
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", endpoint.api_key))
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
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
    while let Some(chunk) = stream.next().await {
        if stop.load(Ordering::SeqCst) {
            return Ok(ChatOutcome::Text);
        }
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
                let choice = parsed.pointer("/choices/0").cloned().unwrap_or(Value::Null);
                let delta = choice.get("delta").cloned().unwrap_or(Value::Null);
                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        saw_text = true;
                        emit(AcpEvent::Stream {
                            kind: StreamKind::Assistant,
                            text: text.to_string(),
                            message_id: None,
                            done: false,
                        });
                    }
                }
                apply_tool_delta(&mut tool_calls, &delta);
                if let Some(reason) = choice
                    .get("finish_reason")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty() && *value != "null")
                {
                    if reason == "tool_calls" {
                        return Ok(ChatOutcome::ToolCalls(tool_calls.into_values().collect()));
                    }
                }
            }
        }
    }
    if !tool_calls.is_empty() {
        return Ok(ChatOutcome::ToolCalls(tool_calls.into_values().collect()));
    }
    if saw_text {
        emit(AcpEvent::Stream {
            kind: StreamKind::Assistant,
            text: String::new(),
            message_id: None,
            done: true,
        });
    }
    Ok(ChatOutcome::Text)
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
            permission_gate: None,
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
    }

    #[test]
    fn tools_include_write_and_run_command() {
        let listed = tool_definitions().to_string();
        assert!(listed.contains("read_file"));
        assert!(listed.contains("list_directory"));
        assert!(listed.contains("write_file"));
        assert!(listed.contains("run_command"));
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
        )
        .await
        .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(8));
        let _ = std::fs::remove_dir_all(&root);
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
