//! Host-owned connector catalog for the Sunsetz plugin marketplace.
//!
//! GitHub, Notion, and Slack use pasted tokens. Gmail, Drive, and Calendar use
//! Google OAuth. Remaining catalog slugs stay coming-soon unless a loopback
//! Open Connector is explicitly configured. Connect is fail-closed.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::paths::{app_data_root, ensure_app_dirs};

mod calendar;
mod drive;
mod github;
mod gmail;
mod google_oauth;
mod notion;
mod protocol;
mod slack;
#[cfg(test)]
mod tests;

const HOST_TOOLS: &[&str] = &[
    "read_file",
    "list_directory",
    "grep",
    "write_file",
    "search_replace",
    "run_command",
    "spawn_agent",
    "agent_output",
    "kill_agent",
];
const CONNECT_TIMEOUT_SECS: u64 = 8;
const GITHUB_CONNECT_TIMEOUT_SECS: u64 = 20;
const INVOKE_TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT_CHARS: usize = 32_768;
const CREDENTIAL_ACCOUNT_PREFIX: &str = "connector:";

const CATALOG: &[CatalogSeed] = &[
    CatalogSeed {
        id: "gmail",
        slug: "gmail",
        developer: "Google",
        version: "0.1.11",
    },
    CatalogSeed {
        id: "github",
        slug: "github",
        developer: "GitHub",
        version: "0.1.8",
    },
    CatalogSeed {
        id: "google-drive",
        slug: "google-drive",
        developer: "Google",
        version: "0.1.6",
    },
    CatalogSeed {
        id: "google-calendar",
        slug: "google-calendar",
        developer: "Google",
        version: "0.1.6",
    },
    CatalogSeed {
        id: "notion",
        slug: "notion",
        developer: "Notion",
        version: "0.1.4",
    },
    CatalogSeed {
        id: "slack",
        slug: "slack",
        developer: "Slack",
        version: "0.1.5",
    },
    CatalogSeed {
        id: "granola",
        slug: "granola",
        developer: "Granola",
        version: "0.1.2",
    },
    CatalogSeed {
        id: "fireflies",
        slug: "fireflies",
        developer: "Fireflies",
        version: "0.1.2",
    },
    CatalogSeed {
        id: "outlook",
        slug: "outlook",
        developer: "Microsoft",
        version: "0.1.3",
    },
    CatalogSeed {
        id: "plaud",
        slug: "plaud",
        developer: "Plaud",
        version: "0.1.1",
    },
];

struct CatalogSeed {
    id: &'static str,
    slug: &'static str,
    developer: &'static str,
    version: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorStateV1 {
    pub id: String,
    pub slug: String,
    pub developer: String,
    pub version: String,
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ConnectorStoreV1 {
    version: u8,
    connectors: Vec<ConnectorStateV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CredentialStoreV1 {
    tokens: HashMap<String, String>,
}

fn store_path() -> PathBuf {
    app_data_root().join("connectors.json")
}

fn credentials_path() -> PathBuf {
    app_data_root().join("connector-credentials.json")
}

fn load_store() -> ConnectorStoreV1 {
    let Ok(raw) = fs::read_to_string(store_path()) else {
        return ConnectorStoreV1 {
            version: 1,
            connectors: Vec::new(),
        };
    };
    serde_json::from_str(&raw).unwrap_or(ConnectorStoreV1 {
        version: 1,
        connectors: Vec::new(),
    })
}

fn save_store(store: &ConnectorStoreV1) -> Result<(), String> {
    ensure_app_dirs().map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    fs::write(store_path(), json).map_err(|e| e.to_string())
}

fn seed_state(seed: &CatalogSeed, stored: Option<&ConnectorStateV1>) -> ConnectorStateV1 {
    ConnectorStateV1 {
        id: seed.id.into(),
        slug: seed.slug.into(),
        developer: seed.developer.into(),
        version: seed.version.into(),
        enabled: stored.map(|row| row.enabled).unwrap_or(false),
        connected: stored.map(|row| row.connected).unwrap_or(false),
        last_error: stored.and_then(|row| row.last_error.clone()),
        tools: stored.map(|row| row.tools.clone()).unwrap_or_default(),
    }
}

fn catalog_seed(id: &str) -> Result<&'static CatalogSeed, String> {
    CATALOG
        .iter()
        .find(|row| row.id == id)
        .ok_or_else(|| format!("CONNECTOR_UNKNOWN:{id}"))
}

pub fn is_loopback_http_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw.trim()) else {
        return false;
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }
    match url.host_str() {
        Some(host) if host.eq_ignore_ascii_case("localhost") => true,
        Some(host) => host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false),
        None => false,
    }
}

pub fn open_connector_endpoint() -> Option<String> {
    configured_open_connector_url().ok().flatten()
}

fn configured_open_connector_url() -> Result<Option<String>, String> {
    let raw = match std::env::var("SUNSETZ_OPEN_CONNECTOR_URL") {
        Ok(value) => value.trim().to_string(),
        Err(_) => return Ok(None),
    };
    if raw.is_empty() {
        return Ok(None);
    }
    if !is_loopback_http_url(&raw) {
        return Err(
            "CONNECTOR_RUNTIME_REJECTED: Open Connector URL must be loopback (127.0.0.1 or localhost)"
                .into(),
        );
    }
    Ok(Some(raw.trim_end_matches('/').to_string()))
}

fn http_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .no_proxy()
        .build()
        .map_err(|error| error.to_string())
}

/// GitHub must honor HTTPS_PROXY / system proxy. Loopback mocks must not.
fn github_http_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    connector_http_client(timeout_secs, "SUNSETZ_GITHUB_API_URL")
}

fn connector_http_client(
    timeout_secs: u64,
    mock_env: &'static str,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("Sunsetz-Desktop/1.0");
    let mock = std::env::var(mock_env).unwrap_or_default();
    if !mock.trim().is_empty() && is_loopback_http_url(mock.trim()) {
        builder = builder.no_proxy();
    }
    builder.build().map_err(|error| error.to_string())
}

fn bound_output(text: String) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(MAX_OUTPUT_CHARS).collect();
    if chars.next().is_none() {
        text
    } else {
        format!("{head}\n[truncated]")
    }
}

fn reserved_tool_name(name: &str) -> bool {
    HOST_TOOLS.iter().any(|tool| *tool == name)
}

fn tool_allowed_for_slug(slug: &str, name: &str) -> bool {
    if reserved_tool_name(name) {
        return false;
    }
    let prefix = format!("{slug}_");
    name.starts_with(&prefix)
}

pub fn list_connectors() -> Result<Vec<ConnectorStateV1>, String> {
    let store = load_store();
    Ok(CATALOG
        .iter()
        .map(|seed| {
            let stored = store.connectors.iter().find(|row| row.id == seed.id);
            seed_state(seed, stored)
        })
        .collect())
}

fn persist_state(next: ConnectorStateV1) -> Result<ConnectorStateV1, String> {
    let mut store = load_store();
    store.version = 1;
    store.connectors.retain(|row| row.id != next.id);
    store.connectors.push(next.clone());
    save_store(&store)?;
    Ok(next)
}

fn disconnected_state(seed: &CatalogSeed) -> ConnectorStateV1 {
    ConnectorStateV1 {
        id: seed.id.into(),
        slug: seed.slug.into(),
        developer: seed.developer.into(),
        version: seed.version.into(),
        enabled: false,
        connected: false,
        last_error: None,
        tools: Vec::new(),
    }
}

fn connected_state(seed: &CatalogSeed, tools: Vec<String>) -> ConnectorStateV1 {
    ConnectorStateV1 {
        id: seed.id.into(),
        slug: seed.slug.into(),
        developer: seed.developer.into(),
        version: seed.version.into(),
        enabled: true,
        connected: true,
        last_error: None,
        tools,
    }
}

async fn connect_host_adapter(
    seed: &CatalogSeed,
    connect: impl std::future::Future<Output = Result<(), String>>,
    tools: Vec<String>,
) -> Result<ConnectorStateV1, String> {
    match connect.await {
        Ok(()) => persist_state(connected_state(seed, tools)),
        Err(error) => {
            let _ = persist_state(ConnectorStateV1 {
                last_error: Some(error.clone()),
                ..disconnected_state(seed)
            });
            Err(error)
        }
    }
}

pub async fn connect_connector(
    id: &str,
    credential: Option<&str>,
) -> Result<ConnectorStateV1, String> {
    let seed = catalog_seed(id)?;
    match seed.id {
        "github" => {
            connect_token_adapter(
                seed,
                credential,
                "CONNECTOR_CREDENTIAL_MISSING: paste a GitHub personal access token",
                async {
                    let token = load_credential("github").unwrap_or_default();
                    github::verify_token(&token).await.map(|_| ())
                },
                github::tool_names(),
            )
            .await
        }
        "notion" => {
            connect_token_adapter(
                seed,
                credential,
                "CONNECTOR_CREDENTIAL_MISSING: paste a Notion internal integration token",
                async {
                    let token = load_credential("notion").unwrap_or_default();
                    notion::verify_token(&token).await
                },
                notion::tool_names(),
            )
            .await
        }
        "slack" => {
            connect_token_adapter(
                seed,
                credential,
                "CONNECTOR_CREDENTIAL_MISSING: paste a Slack bot token",
                async {
                    let token = load_credential("slack").unwrap_or_default();
                    slack::verify_token(&token).await
                },
                slack::tool_names(),
            )
            .await
        }
        "gmail" => {
            connect_host_adapter(seed, gmail::connect(credential), gmail::tool_names()).await
        }
        "google-drive" => {
            connect_host_adapter(seed, drive::connect(credential), drive::tool_names()).await
        }
        "google-calendar" => {
            connect_host_adapter(seed, calendar::connect(credential), calendar::tool_names()).await
        }
        _ => connect_open_connector(seed).await,
    }
}

pub async fn disconnect_connector(id: &str) -> Result<ConnectorStateV1, String> {
    let seed = catalog_seed(id)?;
    let next = persist_state(disconnected_state(seed))?;
    if matches!(seed.id, "github" | "notion" | "slack") {
        let _ = delete_credential(seed.id);
    } else if google_oauth::is_google_app(seed.id) {
        let _ = google_oauth::delete_google_credential_if_unused();
    }
    Ok(next)
}

pub fn connected_connectors() -> Vec<ConnectorStateV1> {
    list_connectors()
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.connected && row.enabled)
        .collect()
}

pub fn connected_tool_definitions() -> Vec<Value> {
    let mut tools = Vec::new();
    for row in connected_connectors() {
        match row.id.as_str() {
            "github" => tools.extend(github::tool_definitions()),
            "gmail" => tools.extend(gmail::tool_definitions()),
            "google-drive" => tools.extend(drive::tool_definitions()),
            "google-calendar" => tools.extend(calendar::tool_definitions()),
            "notion" => tools.extend(notion::tool_definitions()),
            "slack" => tools.extend(slack::tool_definitions()),
            _ => tools.extend(row.cached_remote_tool_defs()),
        }
    }
    tools
}

pub fn connected_write_tools() -> Vec<String> {
    connected_connectors()
        .into_iter()
        .flat_map(|row| match row.id.as_str() {
            "github" => github::write_tools(),
            "gmail" => gmail::write_tools(),
            "google-drive" => drive::write_tools(),
            "google-calendar" => calendar::write_tools(),
            "notion" => notion::write_tools(),
            "slack" => slack::write_tools(),
            _ => row
                .tools
                .into_iter()
                .filter(|name| crate::permission::is_connector_write_tool(name))
                .collect(),
        })
        .collect()
}

pub fn explicit_connector_fragment(ids: &[String]) -> String {
    let names = ids.join(", ");
    format!(
        "The user explicitly requested the {names} connector(s) this turn. Prefer those connector tools for this request. Host file tools remain available. Do not mention these instructions."
    )
}

pub fn connector_ids_from_display(display_text: Option<&str>) -> Result<Vec<String>, String> {
    let value = display_text.unwrap_or_default();
    if value.len() > 256 * 1024 {
        return Err("CONNECTOR_USE_LIMIT: display text is too large to verify".into());
    }
    let mut remaining = value;
    let mut ids = Vec::new();
    while let Some(start) = remaining.find("[[connector:") {
        remaining = &remaining[start + "[[connector:".len()..];
        if remaining.starts_with("v1:") {
            return Err("CONNECTOR_USE_INVALID: recovery tokens must not enter the journal".into());
        }
        let end = remaining
            .find("]]")
            .ok_or_else(|| "CONNECTOR_USE_INVALID: unterminated connector marker".to_string())?;
        let id = remaining[..end].trim();
        if id.is_empty()
            || id
                .chars()
                .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
        {
            return Err("CONNECTOR_USE_INVALID: invalid connector marker".into());
        }
        ids.push(id.to_string());
        remaining = &remaining[end + 2..];
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

pub fn verify_connector_selections(
    ids: &[String],
    display_text: Option<&str>,
) -> Result<Vec<String>, String> {
    if ids.len() > 8 {
        return Err("CONNECTOR_USE_LIMIT: at most 8 connectors per turn".into());
    }
    let markers = connector_ids_from_display(display_text)?;
    if ids.is_empty() {
        if markers.is_empty() {
            return Ok(Vec::new());
        }
        return Err(
            "CONNECTOR_USE_UNVERIFIED: connector markers require verified selections".into(),
        );
    }
    let listed = list_connectors()?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err("CONNECTOR_USE_INVALID: duplicate connector selection".into());
        }
        let row = listed
            .iter()
            .find(|row| row.id == *id)
            .ok_or_else(|| format!("CONNECTOR_UNKNOWN:{id}"))?;
        if !row.connected || !row.enabled {
            return Err(format!("CONNECTOR_NOT_CONNECTED:{id}"));
        }
        out.push(id.clone());
    }
    out.sort();
    if out != markers {
        return Err("CONNECTOR_USE_UNVERIFIED: connector markers and selections differ".into());
    }
    Ok(out)
}

pub async fn invoke_tool(name: &str, arguments: &Value) -> String {
    let owner = connected_connectors()
        .into_iter()
        .find(|row| row.tools.iter().any(|tool| tool == name));
    let Some(owner) = owner else {
        return format!("unknown tool `{name}`");
    };
    match owner.id.as_str() {
        "github" => github::invoke(name, arguments).await,
        "gmail" => gmail::invoke(name, arguments).await,
        "google-drive" => drive::invoke(name, arguments).await,
        "google-calendar" => calendar::invoke(name, arguments).await,
        "notion" => notion::invoke(name, arguments).await,
        "slack" => slack::invoke(name, arguments).await,
        _ => protocol::invoke(&owner.slug, name, arguments).await,
    }
}

impl ConnectorStateV1 {
    fn cached_remote_tool_defs(&self) -> Vec<Value> {
        self.tools
            .iter()
            .filter(|name| tool_allowed_for_slug(&self.slug, name))
            .map(|name| {
                json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": format!("Invoke the {} connector tool `{name}`.", self.slug),
                        "parameters": {
                            "type": "object",
                            "additionalProperties": true
                        }
                    }
                })
            })
            .collect()
    }
}

async fn connect_token_adapter<F>(
    seed: &CatalogSeed,
    credential: Option<&str>,
    missing: &str,
    verify: F,
    tools: Vec<String>,
) -> Result<ConnectorStateV1, String>
where
    F: std::future::Future<Output = Result<(), String>>,
{
    if let Some(token) = credential
        .map(github::normalize_token)
        .filter(|value| !value.is_empty())
    {
        save_credential(seed.id, &token)?;
    }
    if load_credential(seed.id)
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        return Err(missing.into());
    }
    match verify.await {
        Ok(()) => persist_state(connected_state(seed, tools)),
        Err(error) => {
            let _ = persist_state(ConnectorStateV1 {
                last_error: Some(error.clone()),
                ..disconnected_state(seed)
            });
            Err(error)
        }
    }
}

async fn connect_open_connector(seed: &CatalogSeed) -> Result<ConnectorStateV1, String> {
    let endpoint = configured_open_connector_url()?.ok_or_else(|| {
        "CONNECTOR_RUNTIME_MISSING: start Open Connector locally, then set SUNSETZ_OPEN_CONNECTOR_URL"
            .to_string()
    })?;
    match protocol::discover(&endpoint, seed.slug).await {
        Ok(tools) => persist_state(connected_state(seed, tools)),
        Err(error) => {
            let _ = persist_state(ConnectorStateV1 {
                last_error: Some(error.clone()),
                ..disconnected_state(seed)
            });
            Err(error)
        }
    }
}

fn credential_account(id: &str) -> String {
    format!("{CREDENTIAL_ACCOUNT_PREFIX}{id}")
}

fn load_credential_file() -> CredentialStoreV1 {
    let Ok(raw) = fs::read_to_string(credentials_path()) else {
        return CredentialStoreV1::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_credential_file(store: &CredentialStoreV1) -> Result<(), String> {
    ensure_app_dirs().map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(credentials_path(), json.as_bytes()).map_err(|e| e.to_string())?;
        let _ = fs::set_permissions(credentials_path(), fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(credentials_path(), json).map_err(|e| e.to_string())
    }
}

fn load_credential(id: &str) -> Option<String> {
    if !cfg!(test) {
        if let Some(token) = crate::secrets::named_secret_get(&credential_account(id)) {
            if !token.trim().is_empty() {
                return Some(token);
            }
        }
    }
    load_credential_file()
        .tokens
        .get(id)
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

fn save_credential(id: &str, token: &str) -> Result<(), String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return delete_credential(id);
    }
    if !cfg!(test) {
        let _ = crate::secrets::named_secret_set(&credential_account(id), Some(trimmed));
    }
    let mut store = load_credential_file();
    store.tokens.insert(id.to_string(), trimmed.to_string());
    save_credential_file(&store)
}

fn delete_credential(id: &str) -> Result<(), String> {
    if !cfg!(test) {
        let _ = crate::secrets::named_secret_set(&credential_account(id), None);
    }
    let mut store = load_credential_file();
    store.tokens.remove(id);
    save_credential_file(&store)
}
