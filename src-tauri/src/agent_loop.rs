//! Sunsetz-owned agent kernel (first slice).
//!
//! The product runtime is this in-process loop, not a spawned `grok agent stdio`
//! process. It sends the user prompt to an OpenAI-compatible chat endpoint using
//! existing providers/secrets, executes Host-owned `read_file` and
//! `list_directory` inside a trusted project root, and emits the same
//! `AcpEvent` surface the session UI already consumes.
//!
//! Out of scope this slice: write_file, run_command, Hermes skills, memory
//! injection, cron, plugins, sandbox.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::acp_client::{AcpEvent, StreamKind};
use crate::error::{AgentError, AgentErrorCode};
use crate::fs_browser;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmEndpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct AgentTurnConfig {
    pub endpoint: LlmEndpoint,
    pub project_root: Option<PathBuf>,
    pub trusted: bool,
    pub history: Vec<Value>,
    pub user_prompt: String,
    pub stop: Arc<AtomicBool>,
    pub client: reqwest::Client,
    pub max_tool_rounds: u32,
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
         Answer the user directly. You may call read_file and list_directory only inside the \
         trusted project root. You cannot write files, run commands, or access paths outside \
         that root.",
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

pub fn execute_tool(root: Option<&Path>, trusted: bool, name: &str, arguments: &Value) -> String {
    if name == "write_file" || name == "run_command" {
        return format!("unsupported tool `{name}` in this Sunsetz Runtime slice");
    }
    if !matches!(name, "read_file" | "list_directory") {
        return format!("unknown tool `{name}`");
    }
    if !trusted {
        return "no trusted project root".into();
    }
    let Some(root) = root else {
        return "no trusted project root".into();
    };
    let rel = tool_path_arg(arguments);
    match name {
        "read_file" => execute_read_file(root, &rel).unwrap_or_else(|error| error),
        "list_directory" => execute_list_directory(root, &rel).unwrap_or_else(|error| error),
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

pub async fn run_turn<F>(cfg: AgentTurnConfig, mut emit: F)
where
    F: FnMut(AcpEvent) + Send,
{
    let tools_enabled = cfg.trusted && cfg.project_root.is_some();
    let mut messages = vec![json!({
        "role": "system",
        "content": system_prompt(cfg.project_root.as_deref(), cfg.trusted),
    })];
    messages.extend(cfg.history);
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
                    let title = match call.name.as_str() {
                        "read_file" => format!("Read {}", tool_path_arg(&args_value)),
                        "list_directory" => {
                            format!("List {}", tool_path_arg(&args_value))
                        }
                        other => other.to_string(),
                    };
                    let kind = match call.name.as_str() {
                        "read_file" => "read",
                        "list_directory" => "list",
                        other => other,
                    };
                    let raw = json!({ "rawInput": args_value });
                    emit(AcpEvent::ToolCall {
                        tool_call_id: id.clone(),
                        title: title.clone(),
                        kind: kind.into(),
                        status: "in_progress".into(),
                        raw: raw.clone(),
                    });
                    let output = execute_tool(
                        cfg.project_root.as_deref(),
                        cfg.trusted,
                        &call.name,
                        &args_value,
                    );
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
        }
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
    fn tools_do_not_include_write_or_shell() {
        let listed = tool_definitions().to_string();
        assert!(listed.contains("read_file"));
        assert!(listed.contains("list_directory"));
        assert!(!listed.contains("write_file"));
        assert!(!listed.contains("run_command"));
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
        assert!(
            execute_tool(Some(&root), true, "write_file", &json!({"path":"x"}))
                .contains("unsupported")
        );
        assert!(
            execute_tool(Some(&root), true, "run_command", &json!({"path":"x"}))
                .contains("unsupported")
        );
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
}
