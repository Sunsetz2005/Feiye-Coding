//! Custom OpenAI-compatible providers → agent-readable config.toml under GROK_HOME.
//! Intentionally original implementation (not ported from other desktops).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::paths::{agent_config_toml, agent_home_dir, ensure_app_dirs};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProvider {
    pub id: String,
    pub model: String,
    pub base_url: String,
    pub name: String,
    pub has_api_key: bool,
    pub api_backend: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertProviderInput {
    pub id: String,
    pub model: String,
    pub base_url: String,
    pub name: Option<String>,
    /// Empty / omitted = keep existing key on edit.
    pub api_key: Option<String>,
    pub api_backend: Option<String>,
    pub set_as_default: Option<bool>,
    pub create_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersListResult {
    pub providers: Vec<CustomProvider>,
    pub default_model: Option<String>,
    /// `official` = built-in Grok OAuth / xAI path; `custom` = a config.toml model with base_url.
    pub active_source: String,
    /// When `active_source == "custom"`, the selected provider id.
    pub active_provider_id: Option<String>,
    pub config_path: String,
    pub agent_home: String,
}

/// Built-in model id used when routing back to the official upstream Runtime.
pub const OFFICIAL_DEFAULT_MODEL: &str = "grok";

/// Catalog model preferred for composer / official spawn when none is set.
pub const OFFICIAL_CATALOG_MODEL: &str = "grok-4.5";

/// Which inference channel the agent should use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveRoute {
    /// Built-in upstream route (OIDC via auth.json).
    Official,
    /// OpenAI-compatible relay section id in config.toml (`[model.<id>]`).
    Custom { id: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPingResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub endpoint: String,
    pub status: Option<u16>,
    pub error: Option<String>,
    /// Raw, size-bounded response detail for diagnostics only.
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelsResult {
    pub endpoint: String,
    pub models: Vec<RemoteModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModel {
    pub id: String,
    pub owned_by: Option<String>,
}

struct Section {
    id: String,
    start: usize,
    end: usize,
    fields: std::collections::HashMap<String, String>,
}

fn unquote(v: &str) -> String {
    let t = v.trim();
    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        t[1..t.len().saturating_sub(1)].to_string()
    } else {
        t.to_string()
    }
}

fn quote(v: &str) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| format!("\"{v}\""))
}

fn sanitize_id(raw: &str) -> Result<String, String> {
    let id = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if id.is_empty() || !id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
        return Err("provider id must start with a letter or digit".into());
    }
    Ok(id)
}

fn normalize_backend(v: Option<&str>) -> String {
    match v.unwrap_or("").trim() {
        "responses" => "responses".into(),
        "messages" => "messages".into(),
        _ => "chat_completions".into(),
    }
}

fn validate_backend(v: Option<&str>) -> Result<String, String> {
    match v.unwrap_or("").trim() {
        "" | "chat_completions" => Ok("chat_completions".into()),
        "responses" => Ok("responses".into()),
        "messages" => Ok("messages".into()),
        other => Err(format!(
            "unsupported api_backend `{other}` (use chat_completions|responses|messages)"
        )),
    }
}

/// Grok Build joins `{base_url}/chat/completions` (or `/responses`).
/// OpenAI-compatible relays almost always expect `…/v1` as the base.
/// Without it, requests hit `https://host/chat/completions` (404/HTML) and the
/// agent may retry for minutes with no user-visible progress.
pub fn normalize_openai_base_url(raw: &str, api_backend: &str) -> String {
    let mut base = raw.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return base;
    }
    // Anthropic-style messages often use bare host or /v1 already; still prefer /v1.
    let lower = base.to_ascii_lowercase();
    let needs_v1 = matches!(
        api_backend,
        "chat_completions" | "responses" | "messages" | ""
    );
    if needs_v1
        && !lower.ends_with("/v1")
        && !lower.contains("/v1/")
        && !lower.ends_with("/chat/completions")
        && !lower.ends_with("/responses")
        && !lower.ends_with("/messages")
    {
        base.push_str("/v1");
    }
    base
}

fn validate_and_normalize_base_url(raw: &str, api_backend: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("base_url is required".into());
    }
    let parsed = reqwest::Url::parse(raw).map_err(|_| "base_url is not a valid URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("base_url must start with http:// or https://".into());
    }
    if parsed.host_str().is_none() {
        return Err("base_url must include a host".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("base_url must not include credentials".into());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("base_url must not include a query or fragment".into());
    }
    let path = parsed.path().trim_end_matches('/').to_ascii_lowercase();
    if path.ends_with("/models")
        || path.ends_with("/responses")
        || path.ends_with("/chat/completions")
        || path.ends_with("/messages")
    {
        return Err("base_url must be the API root, not an operation endpoint".into());
    }
    Ok(normalize_openai_base_url(raw, api_backend))
}

/// One-shot repair: rewrite stored custom base_url values that omit /v1.
pub fn repair_custom_base_urls() -> Result<bool, String> {
    let path = agent_config_toml();
    if !path.is_file() {
        return Ok(false);
    }
    let text = read_text(&path);
    let sections = parse_model_sections(&text);
    let mut changed = false;
    let mut out = text.clone();
    for s in sections {
        if !is_custom(&s.fields) {
            continue;
        }
        let backend = normalize_backend(s.fields.get("api_backend").map(|x| x.as_str()));
        let Some(old) = s.fields.get("base_url").cloned() else {
            continue;
        };
        let new = normalize_openai_base_url(&old, &backend);
        if new != old.trim().trim_end_matches('/') && new != old {
            // Re-write whole section via remove + append to keep format stable.
            let model = s
                .fields
                .get("model")
                .cloned()
                .unwrap_or_else(|| s.id.clone());
            let name = s
                .fields
                .get("name")
                .cloned()
                .unwrap_or_else(|| s.id.clone());
            let key = s.fields.get("api_key").cloned().unwrap_or_default();
            out = remove_section(&out, &s.id);
            out = append_section(
                &out,
                &s.id,
                &[
                    ("model".into(), model),
                    ("base_url".into(), new),
                    ("name".into(), name),
                    ("api_key".into(), key),
                    ("api_backend".into(), backend),
                ],
            );
            changed = true;
            tracing::info!(
                target: "providers",
                id = %s.id,
                "repaired base_url to include /v1"
            );
        }
    }
    if changed {
        // Preserve [models].default
        let def = get_models_default(&text);
        if let Some(d) = def {
            out = set_models_default(&out, &d);
        }
        write_text(&path, &out)?;
    }
    Ok(changed)
}

fn model_header(id: &str) -> String {
    if id
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    {
        format!("[model.{}]", quote(id))
    } else {
        format!("[model.{id}]")
    }
}

fn parse_model_header_id(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("[model.")?.strip_suffix(']')?;
    Some(unquote(rest).trim().to_string()).filter(|s| !s.is_empty())
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, text).map_err(|e| e.to_string())
}

fn parse_model_sections(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut sections = Vec::new();
    let mut cur: Option<Section> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(hid) = parse_model_header_id(trimmed) {
            if let Some(mut c) = cur.take() {
                c.end = i;
                sections.push(c);
            }
            cur = Some(Section {
                id: hid,
                start: i,
                end: lines.len(),
                fields: std::collections::HashMap::new(),
            });
            continue;
        }
        if trimmed.starts_with('[') {
            if let Some(mut c) = cur.take() {
                c.end = i;
                sections.push(c);
            }
            continue;
        }
        if let Some(ref mut c) = cur {
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(eq) = trimmed.find('=') {
                let key = trimmed[..eq].trim().to_string();
                let val = unquote(trimmed[eq + 1..].trim());
                c.fields.insert(key, val);
            }
        }
    }
    if let Some(c) = cur {
        sections.push(c);
    }
    sections
}

fn get_models_default(text: &str) -> Option<String> {
    let mut in_models = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_models = trimmed == "[models]";
            continue;
        }
        if !in_models || trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("default") {
            let rest = rest.trim().strip_prefix('=')?.trim();
            return Some(unquote(rest));
        }
    }
    None
}

fn set_models_default(text: &str, model_id: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let mut in_models = false;
    let mut models_start: Option<usize> = None;
    for i in 0..lines.len() {
        let trimmed = lines[i].trim().to_string();
        if trimmed.starts_with('[') {
            if trimmed == "[models]" {
                in_models = true;
                models_start = Some(i);
            } else if in_models {
                lines.insert(i, format!("default = {}", quote(model_id)));
                return lines.join("\n");
            } else {
                in_models = false;
            }
            continue;
        }
        if in_models && trimmed.starts_with("default") && trimmed.contains('=') {
            lines[i] = format!("default = {}", quote(model_id));
            return lines.join("\n");
        }
    }
    if let Some(start) = models_start {
        lines.insert(start + 1, format!("default = {}", quote(model_id)));
        return lines.join("\n");
    }
    let block = format!("\n[models]\ndefault = {}\n", quote(model_id));
    let base = text.trim_end();
    if base.is_empty() {
        block.trim_start().to_string()
    } else {
        format!("{base}{block}")
    }
}

fn remove_section(text: &str, id: &str) -> String {
    let sections = parse_model_sections(text);
    let Some(hit) = sections.iter().find(|s| s.id == id) else {
        return text.to_string();
    };
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    lines.drain(hit.start..hit.end);
    let joined = lines.join("\n");
    // collapse excess blank lines
    let mut out = String::new();
    let mut blanks = 0;
    for line in joined.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= 2 {
                out.push('\n');
            }
        } else {
            blanks = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn append_section(text: &str, id: &str, fields: &[(String, String)]) -> String {
    let body: String = fields
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k} = {}", quote(v)))
        .collect::<Vec<_>>()
        .join("\n");
    let block = format!("\n{}\n{body}\n", model_header(id));
    let base = text.trim_end();
    if base.is_empty() {
        block.trim_start().to_string()
    } else {
        format!("{base}\n{block}")
    }
}

fn is_custom(fields: &std::collections::HashMap<String, String>) -> bool {
    fields
        .get("base_url")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn ensure_agent_home() -> Result<PathBuf, String> {
    ensure_app_dirs().map_err(|e| e.to_string())?;
    let home = agent_home_dir();
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    Ok(home)
}

/// Migrate legacy single-slot secrets.relay_* into config.toml once.
pub fn maybe_migrate_legacy_relay(
    relay_base: Option<&str>,
    relay_key: Option<&str>,
    default_model: Option<&str>,
) -> Result<(), String> {
    let base = relay_base.map(str::trim).filter(|s| !s.is_empty());
    let key = relay_key.map(str::trim).filter(|s| !s.is_empty());
    let (Some(base), Some(key)) = (base, key) else {
        return Ok(());
    };
    let list = list_custom_providers()?;
    if !list.providers.is_empty() {
        return Ok(());
    }
    let model = default_model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("grok-4.5");
    let _ = upsert_custom_provider(UpsertProviderInput {
        id: "relay".into(),
        model: model.into(),
        base_url: base.into(),
        name: Some("Imported relay".into()),
        api_key: Some(key.into()),
        api_backend: Some("responses".into()),
        set_as_default: Some(true),
        create_only: Some(true),
    })?;
    Ok(())
}

/// Cap CLI transport retries (Codex-like). Host also circuit-breaks at 5 via retry_state.
pub const PROVIDER_MAX_RETRIES: u32 = 5;

/// Ensure `[models] max_retries = 5` so the agent does not spin 15× on 503.
pub fn ensure_models_retry_cap() -> Result<(), String> {
    let _ = ensure_agent_home()?;
    let path = agent_config_toml();
    let text = read_text(&path);
    let next = set_models_u32_field(&text, "max_retries", PROVIDER_MAX_RETRIES);
    if next != text {
        write_text(&path, &next)?;
        tracing::info!(
            target: "providers",
            "set [models].max_retries = {PROVIDER_MAX_RETRIES}"
        );
    }
    Ok(())
}

fn set_models_u32_field(text: &str, key: &str, value: u32) -> String {
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let mut in_models = false;
    let mut models_start: Option<usize> = None;
    for i in 0..lines.len() {
        let trimmed = lines[i].trim().to_string();
        if trimmed.starts_with('[') {
            if trimmed == "[models]" {
                in_models = true;
                models_start = Some(i);
            } else if in_models {
                lines.insert(i, format!("{key} = {value}"));
                return lines.join("\n");
            } else {
                in_models = false;
            }
            continue;
        }
        if in_models && trimmed.starts_with(key) && trimmed.contains('=') {
            lines[i] = format!("{key} = {value}");
            return lines.join("\n");
        }
    }
    if let Some(start) = models_start {
        lines.insert(start + 1, format!("{key} = {value}"));
        return lines.join("\n");
    }
    let block = format!("\n[models]\n{key} = {value}\n");
    let base = text.trim_end();
    if base.is_empty() {
        block.trim_start().to_string()
    } else {
        format!("{base}{block}")
    }
}

fn route_from_default(def: Option<&str>, providers: &[CustomProvider]) -> (String, Option<String>) {
    if let Some(d) = def {
        if providers.iter().any(|p| p.id == d) {
            return ("custom".into(), Some(d.to_string()));
        }
    }
    ("official".into(), None)
}

fn build_list_result(home: PathBuf, path: PathBuf, text: &str) -> ProvidersListResult {
    let def = get_models_default(text);
    let mut providers = Vec::new();
    for s in parse_model_sections(text) {
        if !is_custom(&s.fields) {
            continue;
        }
        let model = s
            .fields
            .get("model")
            .cloned()
            .unwrap_or_else(|| s.id.clone());
        let base_url = s.fields.get("base_url").cloned().unwrap_or_default();
        let name = s
            .fields
            .get("name")
            .cloned()
            .unwrap_or_else(|| s.id.clone());
        let has_api_key = s
            .fields
            .get("api_key")
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false);
        let api_backend = normalize_backend(s.fields.get("api_backend").map(|s| s.as_str()));
        let is_default = def.as_deref() == Some(s.id.as_str());
        providers.push(CustomProvider {
            id: s.id,
            model,
            base_url,
            name,
            has_api_key,
            api_backend,
            is_default,
        });
    }
    let (active_source, active_provider_id) = route_from_default(def.as_deref(), &providers);
    ProvidersListResult {
        providers,
        default_model: def,
        active_source,
        active_provider_id,
        config_path: path.display().to_string(),
        agent_home: home.display().to_string(),
    }
}

pub fn list_custom_providers() -> Result<ProvidersListResult, String> {
    let home = ensure_agent_home()?;
    let path = agent_config_toml();
    let text = read_text(&path);
    Ok(build_list_result(home, path, &text))
}

/// Current channel from `[models].default` vs custom provider sections.
pub fn active_route() -> ActiveRoute {
    match list_custom_providers() {
        Ok(list) if list.active_source == "custom" => {
            if let Some(id) = list.active_provider_id.filter(|s| !s.trim().is_empty()) {
                return ActiveRoute::Custom { id };
            }
            ActiveRoute::Official
        }
        _ => ActiveRoute::Official,
    }
}

/// Whether `id` is a configured custom provider route (not an official catalog model).
pub fn is_custom_provider_id(id: &str) -> bool {
    let id = id.trim();
    if id.is_empty() {
        return false;
    }
    list_custom_providers()
        .map(|list| list.providers.iter().any(|p| p.id == id))
        .unwrap_or(false)
}

/// Model flag for `grok agent --model`.
///
/// Grok Build behavior (verified 0.2.111):
/// - Custom route: must pass the **provider section id** (e.g. `yunyi`) and
///   **must not** have OIDC `auth.json` in GROK_HOME (else Auth:Oidc hits the
///   relay base_url → 401).
/// - Official route: pass a catalog id (`grok-4.5`); needs `auth.json`.
pub fn agent_spawn_model_id(composer_model: &str) -> String {
    match active_route() {
        ActiveRoute::Custom { id } => id,
        ActiveRoute::Official => {
            let m = composer_model.trim();
            if m.is_empty() || is_custom_provider_id(m) || m == OFFICIAL_DEFAULT_MODEL {
                OFFICIAL_CATALOG_MODEL.into()
            } else {
                m.into()
            }
        }
    }
}

/// Prepare agent-home auth material for the active route.
///
/// Custom: strip agent-home `auth.json` so inference uses `api_key` only.
/// Official: mirror `~/.grok/auth.json` into agent-home for OAuth.
pub fn prepare_route_auth_for_agent() {
    match active_route() {
        ActiveRoute::Custom { ref id } => {
            crate::account::clear_agent_home_auth();
            tracing::info!(
                target: "providers",
                "custom route `{id}`: cleared agent-home auth.json (api_key only)"
            );
        }
        ActiveRoute::Official => {
            if let Err(e) = crate::account::sync_cli_auth_to_agent_home() {
                tracing::warn!(
                    target: "providers",
                    "official route: auth sync failed: {e}"
                );
            }
        }
    }
}

/// Switch active route: `official` or `custom` (+ provider_id).
///
/// Completely rebinds agent-home credentials so the next ACP spawn cannot
/// mix OIDC with a custom relay (or leave a relay as default when going official).
pub fn activate_provider(
    source: &str,
    provider_id: Option<&str>,
) -> Result<ProvidersListResult, String> {
    let source = source.trim().to_ascii_lowercase();
    match source.as_str() {
        "official" => {
            let result = set_default_model_id(OFFICIAL_DEFAULT_MODEL)?;
            // Restore official OAuth into agent-home; drop relay display fields.
            if let Err(e) = crate::account::sync_cli_auth_to_agent_home() {
                tracing::warn!(target: "providers", "activate official: auth sync: {e}");
            }
            let mut secrets = crate::store::load_secrets();
            secrets.relay_base_url = None;
            // Prefer catalog id for composer, not the synthetic "grok" default key.
            secrets.default_model = Some(OFFICIAL_CATALOG_MODEL.into());
            let _ = crate::store::save_secrets(&secrets);
            Ok(result)
        }
        "custom" => {
            let id = provider_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "providerId is required for custom source".to_string())?;
            let list = list_custom_providers()?;
            if !list.providers.iter().any(|p| p.id == id) {
                return Err(format!("unknown provider `{id}`"));
            }
            let result = set_default_model_id(id)?;
            // Critical: remove OIDC so Grok Build uses [model.<id>].api_key.
            crate::account::clear_agent_home_auth();
            if let Some(p) = result.providers.iter().find(|p| p.id == id) {
                let mut secrets = crate::store::load_secrets();
                secrets.relay_base_url = Some(p.base_url.clone());
                // Route id selects the channel; upstream model lives in config.toml.
                secrets.default_model = Some(id.to_string());
                let _ = crate::store::save_secrets(&secrets);
            }
            Ok(result)
        }
        _ => Err(format!("unknown source `{source}` (use official|custom)")),
    }
}

pub fn upsert_custom_provider(input: UpsertProviderInput) -> Result<ProvidersListResult, String> {
    let id = sanitize_id(&input.id)?;
    let model = input.model.trim().to_string();
    if model.is_empty() {
        return Err("model is required for custom providers".into());
    }
    let api_backend = validate_backend(input.api_backend.as_deref())?;
    let base_url = validate_and_normalize_base_url(input.base_url.trim(), &api_backend)?;

    let _ = ensure_agent_home()?;
    let path = agent_config_toml();
    let mut text = read_text(&path);
    let sections = parse_model_sections(&text);
    let existing = sections.iter().find(|s| s.id == id);
    if input.create_only.unwrap_or(false) && existing.is_some() {
        return Err(format!("provider id `{id}` already exists"));
    }
    let prev_key = existing
        .and_then(|s| s.fields.get("api_key"))
        .cloned()
        .unwrap_or_default();
    let next_key = match input.api_key.as_deref() {
        None | Some("") => prev_key,
        Some(k) => k.trim().to_string(),
    };
    if next_key.is_empty() {
        return Err("api_key is required for custom providers".into());
    }

    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(id.as_str())
        .to_string();

    text = remove_section(&text, &id);
    text = append_section(
        &text,
        &id,
        &[
            ("model".into(), model),
            ("base_url".into(), base_url),
            ("name".into(), name),
            ("api_key".into(), next_key),
            ("api_backend".into(), api_backend),
        ],
    );

    if input.set_as_default.unwrap_or(false) {
        text = set_models_default(&text, &id);
    }

    write_text(&path, &text)?;
    let result = list_custom_providers()?;
    if input.set_as_default.unwrap_or(false) {
        // Newly defaulted custom channel must not inherit OIDC.
        crate::account::clear_agent_home_auth();
    }
    Ok(result)
}

pub fn remove_custom_provider(id: &str) -> Result<ProvidersListResult, String> {
    let id = sanitize_id(id)?;
    let path = agent_config_toml();
    let mut text = read_text(&path);
    let def = get_models_default(&text);
    text = remove_section(&text, &id);
    let fell_back_official = def.as_deref() == Some(id.as_str());
    if fell_back_official {
        text = set_models_default(&text, OFFICIAL_DEFAULT_MODEL);
    }
    write_text(&path, &text)?;
    let result = list_custom_providers()?;
    if fell_back_official {
        prepare_route_auth_for_agent();
    }
    Ok(result)
}

pub fn set_default_model_id(model_id: &str) -> Result<ProvidersListResult, String> {
    let id = model_id.trim();
    if id.is_empty() {
        return Err("modelId is required".into());
    }
    let path = agent_config_toml();
    let mut text = read_text(&path);
    text = set_models_default(&text, id);
    write_text(&path, &text)?;
    list_custom_providers()
}

pub(crate) fn resolve_stored_key(provider_id: Option<&str>) -> String {
    let Some(pid) = provider_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let Ok(sid) = sanitize_id(pid) else {
        return String::new();
    };
    let text = read_text(&agent_config_toml());
    parse_model_sections(&text)
        .into_iter()
        .find(|s| s.id == sid)
        .and_then(|s| s.fields.get("api_key").cloned())
        .unwrap_or_default()
}

fn resolve_stored_base(provider_id: Option<&str>) -> String {
    let Some(pid) = provider_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let Ok(sid) = sanitize_id(pid) else {
        return String::new();
    };
    let text = read_text(&agent_config_toml());
    parse_model_sections(&text)
        .into_iter()
        .find(|s| s.id == sid)
        .and_then(|s| s.fields.get("base_url").cloned())
        .unwrap_or_default()
}

pub fn models_list_endpoint(base_url: &str) -> Result<String, String> {
    let normalized = validate_and_normalize_base_url(base_url, "")?;
    let base = normalized.trim_end_matches('/');
    Ok(format!("{base}/models"))
}

pub async fn ping_provider(
    base_url: Option<String>,
    api_key: Option<String>,
    provider_id: Option<String>,
) -> Result<ProviderPingResult, String> {
    let mut base = base_url.unwrap_or_default().trim().to_string();
    if base.is_empty() {
        base = resolve_stored_base(provider_id.as_deref());
    }
    base = validate_and_normalize_base_url(&base, "")?;
    let mut key = api_key.unwrap_or_default().trim().to_string();
    if key.is_empty() {
        key = resolve_stored_key(provider_id.as_deref());
    }
    let endpoint = models_list_endpoint(&base)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let mut req = client.get(&endpoint).header("Accept", "application/json");
    if !key.is_empty() {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    match req.send().await {
        Ok(res) => {
            let status_code = res.status();
            let status = status_code.as_u16();
            let body = res
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(2048)
                .collect::<String>();
            let ok = status_code.is_success();
            Ok(ProviderPingResult {
                ok,
                latency_ms: t0.elapsed().as_millis() as u64,
                endpoint,
                status: Some(status),
                error: (!ok).then(|| format!("Provider returned HTTP {status}")),
                diagnostic: (!body.is_empty()).then_some(body),
            })
        }
        Err(e) => Ok(ProviderPingResult {
            ok: false,
            latency_ms: t0.elapsed().as_millis() as u64,
            endpoint,
            status: None,
            error: Some(e.to_string()),
            diagnostic: None,
        }),
    }
}

pub async fn list_remote_models(
    base_url: String,
    api_key: Option<String>,
    provider_id: Option<String>,
) -> Result<RemoteModelsResult, String> {
    let base = validate_and_normalize_base_url(base_url.trim(), "")?;
    let mut key = api_key.unwrap_or_default().trim().to_string();
    if key.is_empty() {
        key = resolve_stored_key(provider_id.as_deref());
    }
    if key.is_empty() {
        return Err("api_key is required to list models".into());
    }
    let endpoint = models_list_endpoint(&base)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(&endpoint)
        .header("Authorization", format!("Bearer {key}"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "models HTTP {}: {}",
            status.as_u16(),
            text.chars().take(240).collect::<String>()
        ));
    }
    let data: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "models response is not JSON".to_string())?;
    let arr = if let Some(a) = data.as_array() {
        a.clone()
    } else if let Some(a) = data.get("data").and_then(|d| d.as_array()) {
        a.clone()
    } else {
        Vec::new()
    };
    let mut models = Vec::new();
    for item in arr {
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(id) = id else { continue };
        models.push(RemoteModel {
            id: id.to_string(),
            owned_by: item
                .get("owned_by")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        });
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(RemoteModelsResult { endpoint, models })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_and_endpoint() {
        assert_eq!(sanitize_id("My Relay").unwrap(), "my-relay");
        assert!(models_list_endpoint("https://x.example/v1")
            .unwrap()
            .ends_with("/v1/models"));
    }

    #[test]
    fn normalizes_missing_v1() {
        assert_eq!(
            normalize_openai_base_url("https://api.yunyi.ai", "chat_completions"),
            "https://api.yunyi.ai/v1"
        );
        assert_eq!(
            normalize_openai_base_url("https://api.yunyi.ai/v1", "chat_completions"),
            "https://api.yunyi.ai/v1"
        );
        assert_eq!(
            normalize_openai_base_url("https://api.yunyi.ai/v1/", "chat_completions"),
            "https://api.yunyi.ai/v1"
        );
    }

    #[test]
    fn rejects_invalid_provider_contract_values() {
        assert!(validate_backend(Some("response")).is_err());
        assert!(validate_backend(Some("responses")).is_ok());
        assert!(validate_and_normalize_base_url(
            "https://api.example.com/v1/responses",
            "responses"
        )
        .is_err());
        assert!(validate_and_normalize_base_url(
            "https://api.example.com/v1?token=secret",
            "responses"
        )
        .is_err());
        assert_eq!(
            validate_and_normalize_base_url("https://api.example.com", "responses").unwrap(),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn roundtrip_section_text() {
        let text = "";
        let text = append_section(
            text,
            "demo",
            &[
                ("model".into(), "m1".into()),
                ("base_url".into(), "https://ex/v1".into()),
                ("name".into(), "Demo".into()),
                ("api_key".into(), "sk-test".into()),
                ("api_backend".into(), "chat_completions".into()),
            ],
        );
        let text = set_models_default(&text, "demo");
        let sections = parse_model_sections(&text);
        assert_eq!(sections.len(), 1);
        assert_eq!(get_models_default(&text).as_deref(), Some("demo"));
        assert!(is_custom(&sections[0].fields));
    }
}
