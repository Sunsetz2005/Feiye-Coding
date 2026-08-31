//! Versioned independent store for projects, sessions, settings, and secrets.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::paths::{
    automations_file, ensure_app_dirs, projects_file, session_dir, sessions_index_file,
    settings_file,
};

/// Where composer model / effort / mode / permission choices are remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerPrefsScope {
    Global,
    Project,
    Session,
}

impl ComposerPrefsScope {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "project" => Self::Project,
            "session" => Self::Session,
            _ => Self::Global,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Session => "session",
        }
    }
}

/// Effective composer prefs resolved for the current context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerPrefs {
    pub model_id: String,
    pub effort: String,
    pub mode: String,
    pub permission_policy: String,
    /// Scope that was used when resolving (after reading settings).
    pub scope: String,
    /// Which layer actually supplied the values (global | project | session).
    pub source: String,
}

impl Default for ComposerPrefs {
    fn default() -> Self {
        Self {
            model_id: crate::providers::OFFICIAL_DEFAULT_MODEL.into(),
            // Balanced default: faster than high, deeper than low.
            effort: "medium".into(),
            mode: "agent".into(),
            permission_policy: "ask".into(),
            scope: "global".into(),
            source: "global".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub trusted: bool,
    pub last_opened_at: DateTime<Utc>,
    pub path_ok: bool,
    /// Pinned projects float to the top of the sidebar.
    #[serde(default)]
    pub pinned: bool,
    /// Per-project composer prefs (used when scope = project).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub agent_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model_id: Option<String>,
    /// Archived chats stay on disk but hide from the default tree.
    #[serde(default)]
    pub archived: bool,
    /// Per-session composer prefs (used when scope = session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<String>,
    /// Created by shell scheduled automation (`runAutomation`).
    #[serde(default)]
    pub scheduled: bool,
    /// Last exact model context measurement reported by the Runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<SessionTokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsage {
    /// Current occupied context from the final inference: input + output.
    pub used_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub reasoning_tokens: u64,
    /// Aggregate provider usage for the whole agent turn.
    pub turn_input_tokens: u64,
    pub turn_output_tokens: u64,
    pub model_calls: u32,
    pub model_id: String,
    pub context_window_tokens: Option<u64>,
    pub updated_at: DateTime<Utc>,
    /// Stable provenance label; currently always `runtime`.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: String,
    pub locale: String,
    pub session_data_mode: String,
    pub manual_cli_path: Option<String>,
    pub permission_policy: String,
    pub model_id: Option<String>,
    pub effort: Option<String>,
    pub mode: String,
    pub onboarding_done: bool,
    pub setup_skipped: bool,
    /// First-run setup wizard finished (CLI gate + optional auth step).
    #[serde(default)]
    pub setup_wizard_completed: bool,
    /// User skipped account/provider configuration during setup.
    #[serde(default)]
    pub auth_setup_deferred: bool,
    /// Default “open path” target: `finder` / `explorer` / editor id (`code`, `cursor`, …).
    #[serde(default = "default_open_target")]
    pub default_open_target: String,
    /// Remember model / effort / mode / permission at global | project | session.
    #[serde(default = "default_composer_prefs_scope")]
    pub composer_prefs_scope: String,
    /// **API mode.** When set (`host:port`), sessions connect to a remote ACP
    /// server over TCP instead of spawning the local `grok agent stdio` — the
    /// agent can run in WSL, a container, or on another host. Empty/unset uses
    /// the normal local-CLI spawn path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_server_addr: Option<String>,
    /// Max warm/live agent processes (I02). Default 3.
    #[serde(default = "default_max_concurrent_agents")]
    pub max_concurrent_agents: u32,
    /// Recycle idle agent processes after this many minutes (I03). Default 30.
    #[serde(default = "default_agent_idle_minutes")]
    pub agent_idle_minutes: u32,
    /// Pure stream silence before cancel prompt (I06). Default 120 seconds.
    #[serde(default = "default_stream_stall_seconds")]
    pub stream_stall_seconds: u32,
    /// Runtime subprocess sandbox: off | workspace_write | read_only.
    #[serde(default = "default_sandbox_profile")]
    pub sandbox_profile: String,
    /// Store App API keys in the OS keychain (macOS Keychain / Win Cred / Secret Service).
    /// Default **false**: keys stay in `secrets.json` (0600) so cold start does not
    /// trigger system password prompts. Official CLI login still uses `auth.json`.
    #[serde(default)]
    pub store_api_keys_in_keychain: bool,
    /// Product kernel selector. Default `sunsetz` (in-process agent loop).
    /// `grok_acp` keeps the legacy `grok agent stdio` ACP adapter.
    #[serde(default = "default_runtime_backend")]
    pub runtime_backend: String,
    /// Register a login / interval OS job that starts Sunsetz with `--background`.
    #[serde(default)]
    pub run_scheduled_tasks_in_background: bool,
}

fn default_composer_prefs_scope() -> String {
    "global".into()
}

fn default_open_target() -> String {
    "finder".into()
}

fn default_max_concurrent_agents() -> u32 {
    crate::process_limits::DEFAULT_MAX_CONCURRENT_AGENTS
}

fn default_agent_idle_minutes() -> u32 {
    crate::process_limits::DEFAULT_AGENT_IDLE_MINUTES
}

fn default_stream_stall_seconds() -> u32 {
    crate::stream_stall::DEFAULT_STREAM_STALL_SECONDS
}

fn default_sandbox_profile() -> String {
    "off".into()
}

fn default_runtime_backend() -> String {
    "sunsetz".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            locale: "zh".into(),
            session_data_mode: "independent".into(),
            manual_cli_path: None,
            permission_policy: "ask".into(),
            model_id: None,
            effort: Some("medium".into()),
            mode: "agent".into(),
            onboarding_done: false,
            setup_skipped: false,
            setup_wizard_completed: false,
            auth_setup_deferred: false,
            default_open_target: default_open_target(),
            composer_prefs_scope: default_composer_prefs_scope(),
            acp_server_addr: None,
            max_concurrent_agents: default_max_concurrent_agents(),
            agent_idle_minutes: default_agent_idle_minutes(),
            stream_stall_seconds: default_stream_stall_seconds(),
            sandbox_profile: default_sandbox_profile(),
            store_api_keys_in_keychain: false,
            runtime_backend: default_runtime_backend(),
            run_scheduled_tasks_in_background: false,
        }
    }
}

/// App-owned secrets surface (backend-agnostic).
///
/// Sensitive fields (`official_api_key`, `relay_api_key`) prefer the OS keychain
/// (macOS Keychain / Windows Credential Manager / Linux Secret Service) with a
/// `secrets.json` (0600) fallback. See [`crate::secrets`].
///
/// Never log these fields.
///
/// `keychain_has_*` are non-secret booleans written to `secrets.json` so the UI
/// can report "has a key" without unlocking the OS keychain on every launch.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecretsFile {
    pub official_api_key: Option<String>,
    pub relay_base_url: Option<String>,
    pub relay_api_key: Option<String>,
    pub default_model: Option<String>,
    /// Official API key lives in OS keychain (value not on disk).
    #[serde(default)]
    pub keychain_has_official: bool,
    /// Relay API key lives in OS keychain (value not on disk).
    #[serde(default)]
    pub keychain_has_relay: bool,
}

/// File/image card persisted with a chat message (user attach or agent image_gen).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageAttachmentStored {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageStored {
    pub id: String,
    pub role: String,
    pub content: String,
    pub thought: Option<String>,
    pub created_at: DateTime<Utc>,
    /// True when this assistant row records a turn failure (retries exhausted, etc.).
    #[serde(default)]
    pub is_error: bool,
    /// Local file cards (e.g. image_gen output paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<MessageAttachmentStored>>,
    /// UI marker type, e.g. `context_compact` for agent auto/manual compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
}

const SESSION_PREVIEW_TEXT_LIMIT: usize = 280;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPreviewV1 {
    pub version: u8,
    pub session_id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub updated_at: DateTime<Utc>,
    pub model_id: Option<String>,
    pub context_usage: Option<SessionTokenUsage>,
    pub archived: bool,
    pub scheduled: bool,
    pub recent_user_summary: Option<String>,
    pub recent_assistant_summary: Option<String>,
}

fn bounded_visible_summary(input: &str) -> Option<String> {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    let redacted = redact_text(&normalized);
    let mut chars = redacted.trim().chars();
    let mut value = chars
        .by_ref()
        .take(SESSION_PREVIEW_TEXT_LIMIT)
        .collect::<String>();
    if chars.next().is_some() {
        value.push('…');
    }
    (!value.is_empty()).then_some(value)
}

pub fn session_preview(id: &str) -> Result<SessionPreviewV1, String> {
    let meta = load_sessions_index()
        .into_iter()
        .find(|session| session.id == id)
        .ok_or_else(|| "session not found".to_string())?;
    let messages = load_messages(id);
    let recent_user_summary = messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .and_then(|message| bounded_visible_summary(&message.content));
    let recent_assistant_summary = messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .and_then(|message| bounded_visible_summary(&message.content));

    Ok(SessionPreviewV1 {
        version: 1,
        session_id: meta.id,
        project_id: meta.project_id,
        title: meta.title,
        updated_at: meta.updated_at,
        model_id: meta.model_id,
        context_usage: meta.context_usage,
        archived: meta.archived,
        scheduled: meta.scheduled,
        recent_user_summary,
        recent_assistant_summary,
    })
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &PathBuf) -> T {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

/// Read JSON; if the file exists but is corrupt, quarantine it and return default.
fn read_json_recover<T: for<'de> Deserialize<'de> + Default>(path: &PathBuf) -> T {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => T::default(),
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    "corrupt store file {} ({e}); quarantining and starting empty",
                    path.display()
                );
                let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
                let bak = path.with_extension(format!("corrupt-{stamp}.json"));
                let _ = fs::rename(path, &bak);
                T::default()
            }
        },
        Err(_) => T::default(),
    }
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    // Exclusive lock + temp rename so shared-mode / dual-instance writes do not
    // leave a half-written index (E06).
    crate::store_lock::write_bytes_atomic(path, s.as_bytes())
}

pub fn load_settings() -> AppSettings {
    let _ = ensure_app_dirs();
    let path = settings_file();
    let s: AppSettings = read_json(&path);
    // One-time: installs that already stored keys in keychain before the opt-in
    // keep keychain mode so keys remain reachable without a silent loss.
    if !s.store_api_keys_in_keychain {
        let disk = crate::secrets::load_secrets_disk_only();
        if disk.keychain_has_official || disk.keychain_has_relay {
            // Re-check both files while holding the same transaction lock used by
            // explicit keychain toggles. A concurrent disable may have cleared the
            // presence flags while this caller was waiting for the lock.
            if let Ok(recovered) = recover_keychain_preference_at(
                &path,
                &keychain_preference_transaction_target(),
                || {
                    let disk = crate::secrets::load_secrets_disk_only();
                    disk.keychain_has_official || disk.keychain_has_relay
                },
            ) {
                return recovered;
            }
        }
    }
    s
}

/// Read settings without running keychain-compatibility recovery.
/// Call only while the keychain preference transaction lock is already held.
pub(crate) fn load_settings_for_keychain_transaction() -> AppSettings {
    let _ = ensure_app_dirs();
    read_json(&settings_file())
}

pub fn save_settings(s: &AppSettings) -> Result<(), String> {
    let _ = ensure_app_dirs();
    write_json(&settings_file(), s)
}

fn patch_global_composer_prefs(
    model_id: Option<String>,
    effort: Option<String>,
    mode: Option<String>,
    permission_policy: Option<String>,
) -> Result<(), String> {
    let mut patch = serde_json::Map::new();
    if let Some(value) = model_id {
        patch.insert("modelId".into(), serde_json::Value::String(value));
    }
    if let Some(value) = effort {
        patch.insert("effort".into(), serde_json::Value::String(value));
    }
    if let Some(value) = mode {
        patch.insert("mode".into(), serde_json::Value::String(value));
    }
    if let Some(value) = permission_policy {
        patch.insert("permissionPolicy".into(), serde_json::Value::String(value));
    }
    if !patch.is_empty() {
        patch_settings_v1(serde_json::Value::Object(patch))?;
    }
    Ok(())
}

const SETTINGS_PATCH_KEYS_V1: &[&str] = &[
    "theme",
    "locale",
    "sessionDataMode",
    "manualCliPath",
    "permissionPolicy",
    "modelId",
    "effort",
    "mode",
    "onboardingDone",
    "setupSkipped",
    "setupWizardCompleted",
    "authSetupDeferred",
    "defaultOpenTarget",
    "composerPrefsScope",
    "acpServerAddr",
    "maxConcurrentAgents",
    "agentIdleMinutes",
    "streamStallSeconds",
    "sandboxProfile",
    "storeApiKeysInKeychain",
    "runtimeBackend",
    "runScheduledTasksInBackground",
];

fn keychain_preference_transaction_target() -> PathBuf {
    settings_file().with_extension("keychain-preference-transaction.v1")
}

fn with_keychain_preference_transaction_at<R>(
    transaction_target: &Path,
    body: impl FnOnce() -> Result<R, String>,
) -> Result<R, String> {
    crate::store_lock::with_exclusive_lock(transaction_target, body)
}

pub(crate) fn with_keychain_preference_transaction_v1<R>(
    body: impl FnOnce() -> Result<R, String>,
) -> Result<R, String> {
    let _ = ensure_app_dirs();
    with_keychain_preference_transaction_at(&keychain_preference_transaction_target(), body)
}

fn recover_keychain_preference_at(
    settings_path: &Path,
    transaction_target: &Path,
    keychain_still_has_values: impl FnOnce() -> bool,
) -> Result<AppSettings, String> {
    with_keychain_preference_transaction_at(transaction_target, || {
        let current: AppSettings = read_json(&settings_path.to_path_buf());
        if current.store_api_keys_in_keychain || !keychain_still_has_values() {
            return Ok(current);
        }
        patch_settings_at(
            settings_path,
            serde_json::json!({"storeApiKeysInKeychain": true}),
        )
        .map(|(_, next)| next)
    })
}

/// Convert a legacy full-object write into at most one field-level patch.
///
/// A multi-field difference is indistinguishable from a stale snapshot trying
/// to restore fields that another writer already changed, so it is rejected.
pub fn legacy_settings_patch_v1(
    current: &AppSettings,
    requested: &AppSettings,
) -> Result<Option<serde_json::Value>, String> {
    let current = serde_json::to_value(current).map_err(|error| error.to_string())?;
    let requested = serde_json::to_value(requested).map_err(|error| error.to_string())?;
    let current = current
        .as_object()
        .ok_or_else(|| "serialize current settings object".to_string())?;
    let requested = requested
        .as_object()
        .ok_or_else(|| "serialize requested settings object".to_string())?;

    let keys = current
        .keys()
        .chain(requested.keys())
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let mut changed = serde_json::Map::new();
    for key in keys.iter().copied() {
        let before = current.get(key).unwrap_or(&serde_json::Value::Null);
        let after = requested.get(key).unwrap_or(&serde_json::Value::Null);
        if before != after {
            changed.insert(key.to_string(), after.clone());
        }
    }
    match changed.len() {
        0 => Ok(None),
        1 => Ok(Some(serde_json::Value::Object(changed))),
        _ => {
            let fields = changed.keys().cloned().collect::<Vec<_>>().join(",");
            Err(format!(
                "SETTINGS_SET_REJECTED_USE_SETTINGS_PATCH_V1: full-object request changes multiple fields ({fields})"
            ))
        }
    }
}

fn patch_settings_at(
    path: &Path,
    patch: serde_json::Value,
) -> Result<(AppSettings, AppSettings), String> {
    let encoded = serde_json::to_vec(&patch).map_err(|error| error.to_string())?;
    if encoded.len() > 32 * 1024 {
        return Err("SETTINGS_PATCH_TOO_LARGE".into());
    }
    let updates = patch
        .as_object()
        .ok_or_else(|| "SETTINGS_PATCH_OBJECT_REQUIRED".to_string())?;
    for key in updates.keys() {
        if !SETTINGS_PATCH_KEYS_V1.contains(&key.as_str()) {
            return Err(format!("SETTINGS_PATCH_UNKNOWN_FIELD:{key}"));
        }
    }
    let updates = updates.clone();
    crate::store_lock::update_json_locked(path, AppSettings::default, move |current| {
        let previous = current.clone();
        let mut merged = serde_json::to_value(&*current).map_err(|error| error.to_string())?;
        let object = merged
            .as_object_mut()
            .ok_or_else(|| "serialize settings object".to_string())?;
        for (key, value) in updates {
            object.insert(key, value);
        }
        let next: AppSettings = serde_json::from_value(merged)
            .map_err(|error| format!("SETTINGS_PATCH_INVALID:{error}"))?;
        *current = next.clone();
        Ok((previous, next))
    })
}

fn patch_settings_with_keychain_transaction_at(
    settings_path: &Path,
    transaction_target: &Path,
    patch: serde_json::Value,
    migrate: impl FnOnce(bool) -> Result<(), String>,
) -> Result<(AppSettings, AppSettings), String> {
    with_keychain_preference_transaction_at(transaction_target, || {
        let updated = patch_settings_at(settings_path, patch)?;
        finish_keychain_preference_patch_at(settings_path, updated, migrate)
    })
}

fn finish_keychain_preference_patch_at(
    settings_path: &Path,
    (previous, current): (AppSettings, AppSettings),
    migrate: impl FnOnce(bool) -> Result<(), String>,
) -> Result<(AppSettings, AppSettings), String> {
    if previous.store_api_keys_in_keychain == current.store_api_keys_in_keychain {
        return Ok((previous, current));
    }

    if let Err(error) = migrate(current.store_api_keys_in_keychain) {
        return match compare_restore_keychain_preference_at(
            settings_path,
            current.store_api_keys_in_keychain,
            previous.store_api_keys_in_keychain,
        ) {
            Ok(true) => Err(error),
            Ok(false) => Err(format!(
                "{error}; SETTINGS_PATCH_ROLLBACK_SKIPPED_NON_TRANSACTIONAL_WRITE"
            )),
            Err(rollback) => Err(format!(
                "{error}; SETTINGS_PATCH_ROLLBACK_FAILED:{rollback}"
            )),
        };
    }

    Ok((previous, current))
}

fn patch_legacy_settings_with_keychain_transaction_at(
    settings_path: &Path,
    transaction_target: &Path,
    requested: &AppSettings,
    migrate: impl FnOnce(bool) -> Result<(), String>,
) -> Result<(AppSettings, AppSettings), String> {
    with_keychain_preference_transaction_at(transaction_target, || {
        let observed: AppSettings = read_json(&settings_path.to_path_buf());
        let Some(patch) = legacy_settings_patch_v1(&observed, requested)? else {
            return Ok((observed.clone(), observed));
        };
        let updated = patch_settings_at(settings_path, patch)?;
        finish_keychain_preference_patch_at(settings_path, updated, migrate)
    })
}

/// Atomically patch explicit settings fields without replacing concurrent
/// updates to unrelated fields. The returned pair is `(previous, current)` so
/// callers can apply process-level side effects and perform a guarded rollback.
pub fn patch_settings_v1(patch: serde_json::Value) -> Result<(AppSettings, AppSettings), String> {
    let _ = ensure_app_dirs();
    if patch
        .as_object()
        .is_some_and(|object| object.contains_key("storeApiKeysInKeychain"))
    {
        return Err("SETTINGS_PATCH_KEYCHAIN_TRANSACTION_REQUIRED".into());
    }
    patch_settings_at(&settings_file(), patch)
}

/// Patch settings and migrate key material as one serialized transaction.
///
/// The settings file lock is held only for each short JSON update. A distinct
/// process-wide sidecar lock spans the keychain migration and guarded rollback,
/// so unrelated settings patches remain independent.
pub fn patch_settings_with_keychain_transaction_v1(
    patch: serde_json::Value,
    migrate: impl FnOnce(bool) -> Result<(), String>,
) -> Result<(AppSettings, AppSettings), String> {
    let _ = ensure_app_dirs();
    patch_settings_with_keychain_transaction_at(
        &settings_file(),
        &keychain_preference_transaction_target(),
        patch,
        migrate,
    )
}

/// Fail-safe adapter for the legacy full-object command. Comparison, optional
/// one-field patch, keychain migration, and rollback share one transaction.
pub fn patch_legacy_settings_with_keychain_transaction_v1(
    requested: &AppSettings,
    migrate: impl FnOnce(bool) -> Result<(), String>,
) -> Result<(AppSettings, AppSettings), String> {
    let _ = ensure_app_dirs();
    patch_legacy_settings_with_keychain_transaction_at(
        &settings_file(),
        &keychain_preference_transaction_target(),
        requested,
        migrate,
    )
}

/// Roll back the keychain preference only if no newer writer changed that same
/// field. Concurrent updates to unrelated settings remain intact.
fn compare_restore_keychain_preference_at(
    path: &Path,
    expected: bool,
    replacement: bool,
) -> Result<bool, String> {
    crate::store_lock::update_json_locked(path, AppSettings::default, |current| {
        if current.store_api_keys_in_keychain != expected {
            return Ok(false);
        }
        current.store_api_keys_in_keychain = replacement;
        Ok(true)
    })
}

pub fn load_projects() -> Vec<Project> {
    let _ = ensure_app_dirs();
    let mut list: Vec<Project> = read_json_recover(&projects_file());
    for p in &mut list {
        p.path_ok = PathBuf::from(&p.path).is_dir();
    }
    list.sort_by(|a, b| match (b.pinned, a.pinned) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => b.last_opened_at.cmp(&a.last_opened_at),
    });
    list
}

pub fn save_projects(list: &[Project]) -> Result<(), String> {
    write_json(&projects_file(), &list)
}

pub fn add_project(path: String, trust: bool) -> Result<Project, String> {
    let path_buf = PathBuf::from(&path);
    if !path_buf.is_dir() {
        return Err("path is not a directory".into());
    }
    let name = path_buf
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let mut list = load_projects();
    if let Some(existing) = list.iter_mut().find(|p| p.path == path) {
        existing.trusted = trust || existing.trusted;
        existing.last_opened_at = Utc::now();
        existing.path_ok = true;
        let clone = existing.clone();
        save_projects(&list)?;
        return Ok(clone);
    }
    let p = Project {
        id: Uuid::new_v4().to_string(),
        name,
        path,
        trusted: trust,
        last_opened_at: Utc::now(),
        path_ok: true,
        pinned: false,
        model_id: None,
        effort: None,
        mode: None,
        permission_policy: None,
    };
    list.push(p.clone());
    save_projects(&list)?;
    Ok(p)
}

/// Delete every session belonging to a project (including archived journals).
pub fn delete_project_sessions(project_id: &str) -> Result<usize, String> {
    let ids: Vec<String> = load_sessions_index()
        .into_iter()
        .filter(|session| session.project_id.as_deref() == Some(project_id))
        .map(|session| session.id)
        .collect();
    let n = ids.len();
    for id in ids {
        delete_session(&id)?;
    }
    Ok(n)
}

/// Remove the project from the app list and delete its chats.
/// Does **not** delete the disk folder.
pub fn remove_project(id: &str) -> Result<(), String> {
    let _ = delete_project_sessions(id)?;
    let mut list = load_projects();
    list.retain(|p| p.id != id);
    save_projects(&list)
}

pub fn set_project_path(id: &str, path: &str) -> Result<Project, String> {
    let path_buf = PathBuf::from(path);
    if !path_buf.is_dir() {
        return Err("path is not a directory".into());
    }
    let next = path_buf
        .canonicalize()
        .unwrap_or(path_buf)
        .to_string_lossy()
        .to_string();
    let mut list = load_projects();
    if list.iter().any(|p| p.id != id && p.path == next) {
        return Err("another project already uses this folder".into());
    }
    let p = list
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "project not found".to_string())?;
    p.path = next;
    p.path_ok = true;
    p.last_opened_at = Utc::now();
    let clone = p.clone();
    save_projects(&list)?;
    Ok(clone)
}

pub fn rename_project(id: &str, name: &str) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name empty".into());
    }
    let mut list = load_projects();
    let p = list
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "project not found".to_string())?;
    p.name = name.to_string();
    let clone = p.clone();
    save_projects(&list)?;
    Ok(clone)
}

pub fn set_project_pinned(id: &str, pinned: bool) -> Result<Project, String> {
    let mut list = load_projects();
    let p = list
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "project not found".to_string())?;
    p.pinned = pinned;
    let clone = p.clone();
    save_projects(&list)?;
    Ok(clone)
}

pub fn trust_project(id: &str) -> Result<Project, String> {
    let mut list = load_projects();
    let p = list
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "project not found".to_string())?;
    p.trusted = true;
    p.last_opened_at = Utc::now();
    let clone = p.clone();
    save_projects(&list)?;
    Ok(clone)
}

/// Set or clear a project-level permission tier (L10).
///
/// `policy = None` / empty / `"inherit"` clears the override so the app default
/// applies. Untrusted projects cannot store a relaxed tier.
pub fn set_project_permission_policy(id: &str, policy: Option<String>) -> Result<Project, String> {
    use crate::permission::PermissionPolicy;

    let mut list = load_projects();
    let p = list
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "project not found".to_string())?;
    if !p.trusted {
        return Err("trust this project before setting a permission tier".into());
    }

    let next = match policy {
        None => None,
        Some(raw) => {
            let t = raw.trim();
            if t.is_empty()
                || t.eq_ignore_ascii_case("inherit")
                || t.eq_ignore_ascii_case("app_default")
                || t.eq_ignore_ascii_case("default")
            {
                None
            } else {
                Some(PermissionPolicy::parse(t).as_str().to_string())
            }
        }
    };
    p.permission_policy = next;
    let clone = p.clone();
    save_projects(&list)?;
    Ok(clone)
}

pub fn load_sessions_index() -> Vec<SessionMeta> {
    let _ = ensure_app_dirs();
    // Recover from torn/corrupt index (shared CLI+App or crash mid-write).
    let mut list: Vec<SessionMeta> = read_json_recover(&sessions_index_file());
    list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    list
}

pub fn save_sessions_index(list: &[SessionMeta]) -> Result<(), String> {
    write_json(&sessions_index_file(), &list)
}

fn update_sessions_index<R>(
    update: impl FnOnce(&mut Vec<SessionMeta>) -> Result<R, String>,
) -> Result<R, String> {
    let _ = ensure_app_dirs();
    crate::store_lock::update_json_locked(&sessions_index_file(), Vec::<SessionMeta>::new, update)
}

pub fn create_session(
    project_id: Option<String>,
    title: Option<String>,
    scheduled: bool,
) -> Result<SessionMeta, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let meta = SessionMeta {
        id: id.clone(),
        project_id,
        title: title.unwrap_or_else(|| "New chat".into()),
        agent_session_id: None,
        created_at: now,
        updated_at: now,
        model_id: None,
        archived: false,
        effort: None,
        mode: None,
        permission_policy: None,
        scheduled,
        context_usage: None,
    };
    update_sessions_index(|list| {
        list.insert(0, meta.clone());
        Ok(())
    })?;
    let dir = session_dir(&id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    write_json(&dir.join("messages.json"), &Vec::<ChatMessageStored>::new())?;
    Ok(meta)
}

pub fn update_session_meta(meta: &SessionMeta) -> Result<(), String> {
    update_sessions_index(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == meta.id) {
            *s = meta.clone();
        } else {
            list.insert(0, meta.clone());
        }
        Ok(())
    })
}

pub fn delete_session(id: &str) -> Result<(), String> {
    update_sessions_index(|list| {
        list.retain(|s| s.id != id);
        Ok(())
    })?;
    let dir = session_dir(id);
    let _ = fs::remove_dir_all(dir);
    if let Err(error) = crate::session_search::remove_session(id) {
        tracing::warn!("session search remove failed id={id}: {error}");
    }
    Ok(())
}

pub fn rename_session(id: &str, title: &str) -> Result<SessionMeta, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("title empty".into());
    }
    update_sessions_index(|list| {
        let s = list
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| "session not found".to_string())?;
        s.title = title.to_string();
        s.updated_at = Utc::now();
        Ok(s.clone())
    })
}

pub fn set_session_scheduled(id: &str, scheduled: bool) -> Result<SessionMeta, String> {
    update_sessions_index(|list| {
        let s = list
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| "session not found".to_string())?;
        s.scheduled = scheduled;
        s.updated_at = Utc::now();
        Ok(s.clone())
    })
}

pub fn set_session_archived(id: &str, archived: bool) -> Result<SessionMeta, String> {
    update_sessions_index(|list| {
        let s = list
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| "session not found".to_string())?;
        s.archived = archived;
        s.updated_at = Utc::now();
        Ok(s.clone())
    })
}

/// Bind (or clear) a session's project folder. Used to attach orphan / legacy
/// chats to a project added later.
pub fn set_session_project(id: &str, project_id: Option<String>) -> Result<SessionMeta, String> {
    let pid = project_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(ref p) = pid {
        // Ensure project exists in the projects list.
        let projects = load_projects();
        if !projects.iter().any(|x| x.id == *p) {
            return Err(format!("project not found: {p}"));
        }
    }
    update_sessions_index(|list| {
        let s = list
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| "session not found".to_string())?;
        s.project_id = pid;
        s.updated_at = Utc::now();
        Ok(s.clone())
    })
}

/// Archive every non-archived session under a project.
pub fn archive_project_sessions(project_id: &str) -> Result<usize, String> {
    update_sessions_index(|list| {
        let mut n = 0usize;
        for s in list.iter_mut() {
            if s.project_id.as_deref() == Some(project_id) && !s.archived {
                s.archived = true;
                s.updated_at = Utc::now();
                n += 1;
            }
        }
        Ok(n)
    })
}

pub fn load_messages(session_id: &str) -> Vec<ChatMessageStored> {
    read_json_recover(&session_dir(session_id).join("messages.json"))
}

pub fn save_messages(session_id: &str, messages: &[ChatMessageStored]) -> Result<(), String> {
    write_json(&session_dir(session_id).join("messages.json"), &messages)?;
    if let Err(error) = crate::session_search::reindex_session(session_id) {
        tracing::warn!("session search update failed id={session_id}: {error}");
    }
    Ok(())
}

pub(crate) fn update_messages<R>(
    session_id: &str,
    update: impl FnOnce(&mut Vec<ChatMessageStored>) -> Result<R, String>,
) -> Result<R, String> {
    let result = crate::store_lock::update_json_locked(
        &session_dir(session_id).join("messages.json"),
        Vec::<ChatMessageStored>::new,
        update,
    );
    if result.is_ok() {
        if let Err(error) = crate::session_search::reindex_session(session_id) {
            tracing::warn!("session search update failed id={session_id}: {error}");
        }
    }
    result
}

pub fn append_message(session_id: &str, msg: ChatMessageStored) -> Result<(), String> {
    update_messages(session_id, |msgs| {
        // Upsert by id — never double-insert the same host message (stream complete +
        // reconnect edge cases). Keeps journal length honest for multi-turn chats.
        if let Some(slot) = msgs.iter_mut().find(|m| m.id == msg.id) {
            *slot = msg;
        } else {
            msgs.push(msg);
        }
        Ok(())
    })
}

/// End index (exclusive) of the full turn for `user_prompt_index` (0-based).
/// Turn = that user message + following non-user rows until the next user.
pub fn end_index_through_user_prompt(
    messages: &[ChatMessageStored],
    user_prompt_index: u32,
) -> Option<usize> {
    let mut user_i = 0u32;
    for (i, m) in messages.iter().enumerate() {
        if m.role != "user" {
            continue;
        }
        if user_i == user_prompt_index {
            let mut j = i + 1;
            while j < messages.len() && messages[j].role != "user" {
                j += 1;
            }
            return Some(j);
        }
        user_i = user_i.saturating_add(1);
    }
    None
}

/// Keep messages through the end of the selected user turn (ACP `/rewind` semantics).
pub fn truncate_through_user_prompt(
    messages: &[ChatMessageStored],
    user_prompt_index: u32,
) -> Result<Vec<ChatMessageStored>, String> {
    let end = end_index_through_user_prompt(messages, user_prompt_index)
        .ok_or_else(|| format!("user prompt index out of range: {user_prompt_index}"))?;
    Ok(messages[..end].to_vec())
}

/// Fork a session: new journal + meta, same project, no agent session id.
/// `through_user_prompt_index`: when set, copy only through that user turn (inclusive).
pub fn fork_session(
    source_id: &str,
    through_user_prompt_index: Option<u32>,
    title: Option<String>,
) -> Result<SessionMeta, String> {
    let list = load_sessions_index();
    let source = list
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| format!("session not found: {source_id}"))?
        .clone();

    let mut msgs = load_messages(source_id);
    if let Some(idx) = through_user_prompt_index {
        msgs = truncate_through_user_prompt(&msgs, idx)?;
    }

    let fork_title = title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let base = source.title.trim();
            let base = if base.is_empty() { "chat" } else { base };
            if base.to_ascii_lowercase().starts_with("fork of ") {
                base.to_string()
            } else {
                format!("Fork of {base}")
            }
        });

    let mut meta = create_session(source.project_id.clone(), Some(fork_title), false)?;
    // Inherit composer prefs from source so the fork feels continuous.
    meta.model_id = source.model_id.clone();
    meta.effort = source.effort.clone();
    meta.mode = source.mode.clone();
    meta.permission_policy = source.permission_policy.clone();
    meta.updated_at = Utc::now();
    update_session_meta(&meta)?;

    // Remap ids so the fork is independent of the source journal ids.
    let prefix = format!("fork-{}", &meta.id[..meta.id.len().min(8)]);
    let original_ids: Vec<String> = msgs.iter().map(|message| message.id.clone()).collect();
    let compact_src = crate::context_compact::load_v1(source_id).ok().flatten();
    let forked: Vec<ChatMessageStored> = msgs
        .into_iter()
        .enumerate()
        .map(|(i, mut m)| {
            m.id = format!("{prefix}-{i}");
            m
        })
        .collect();
    save_messages(&meta.id, &forked)?;
    if let Some(src) = compact_src {
        let id_map: HashMap<String, String> = original_ids
            .into_iter()
            .enumerate()
            .map(|(index, old)| (old, format!("{prefix}-{index}")))
            .collect();
        if let Some(remapped) = crate::context_compact::remap_artifact_ids(&src, &id_map) {
            if let Err(error) = crate::context_compact::save_v1(&meta.id, &remapped) {
                tracing::warn!("copy compact sidecar onto fork {}: {error}", meta.id);
            }
        }
    }
    Ok(meta)
}

// ─── Automations (scheduled tasks shell) ───────────────────────────────────

/// Host-side scheduled automation. The default kernel ignites due claims from
/// Host; this store is the source of truth for the list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: String,
    pub title: String,
    /// Natural-language prompt / instructions for the agent when the task runs.
    pub prompt: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub project_id: Option<String>,
    pub model_id: Option<String>,
    pub effort: Option<String>,
    /// `daily` | `weekly` | `weekdays` | `once`
    #[serde(default = "default_frequency")]
    pub frequency: String,
    /// Local wall-clock time `HH:MM` (24h).
    #[serde(default = "default_time")]
    pub time: String,
    /// For `weekly`: 0=Sun … 6=Sat (JS Date convention).
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// `all` | `failures` | `none`
    #[serde(default = "default_notify")]
    pub notify: String,
    /// `skip` | `run_once` for occurrences missed while the Host was offline.
    #[serde(default = "default_missed_run_policy")]
    pub missed_run_policy: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    /// Minutes between runs when `frequency` is `interval`. Minimum 15.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_minutes: Option<u32>,
    /// Skills to load at fire time. Missing/stale hashes fail closed.
    #[serde(default)]
    pub skill_ids: Vec<AutomationSkillRefV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSkillRefV1 {
    pub id: String,
    pub tree_hash: String,
}

fn default_true() -> bool {
    true
}
fn default_frequency() -> String {
    "daily".into()
}
fn default_time() -> String {
    "09:00".into()
}
fn default_notify() -> String {
    "all".into()
}
fn default_missed_run_policy() -> String {
    "run_once".into()
}

fn normalize_interval_minutes(value: Option<u32>) -> Result<Option<u32>, String> {
    match value {
        None => Ok(None),
        Some(0) => Ok(None),
        Some(minutes) if minutes < 15 => Err("intervalMinutes must be at least 15".into()),
        Some(minutes) => Ok(Some(minutes)),
    }
}

fn normalize_missed_run_policy(value: Option<&str>) -> Result<String, String> {
    match value
        .unwrap_or("run_once")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "skip" => Ok("skip".into()),
        "run_once" => Ok("run_once".into()),
        _ => Err("missedRunPolicy must be skip or run_once".into()),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationInput {
    pub title: String,
    pub prompt: String,
    pub enabled: Option<bool>,
    pub project_id: Option<String>,
    pub model_id: Option<String>,
    pub effort: Option<String>,
    pub frequency: Option<String>,
    pub time: Option<String>,
    pub weekdays: Option<Vec<u8>>,
    pub notify: Option<String>,
    pub missed_run_policy: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub interval_minutes: Option<u32>,
    pub skill_ids: Option<Vec<AutomationSkillRefV1>>,
}

pub fn load_automations() -> Vec<Automation> {
    let _ = ensure_app_dirs();
    let mut list: Vec<Automation> = read_json(&automations_file());
    list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    list
}

pub fn save_automations(list: &[Automation]) -> Result<(), String> {
    let _ = ensure_app_dirs();
    write_json(&automations_file(), &list)
}

pub(crate) fn update_automations<R>(
    update: impl FnOnce(&mut Vec<Automation>) -> Result<R, String>,
) -> Result<R, String> {
    let _ = ensure_app_dirs();
    crate::store_lock::update_json_locked(&automations_file(), Vec::<Automation>::new, update)
}

pub(crate) fn scheduler_advance_automation(
    id: &str,
    attempted_at: DateTime<Utc>,
    next_run_at: Option<DateTime<Utc>>,
    disable: bool,
) -> Result<Automation, String> {
    update_automations(|list| {
        let automation = list
            .iter_mut()
            .find(|automation| automation.id == id)
            .ok_or_else(|| "automation not found".to_string())?;
        automation.last_run_at = Some(attempted_at);
        automation.next_run_at = next_run_at;
        if disable {
            automation.enabled = false;
        }
        automation.updated_at = Utc::now();
        Ok(automation.clone())
    })
}

pub fn create_automation(input: AutomationInput) -> Result<Automation, String> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err("title empty".into());
    }
    let prompt = input.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("prompt empty".into());
    }
    let now = Utc::now();
    let auto = Automation {
        id: Uuid::new_v4().to_string(),
        title,
        prompt,
        enabled: input.enabled.unwrap_or(true),
        project_id: input.project_id,
        model_id: input.model_id,
        effort: input.effort,
        frequency: input
            .frequency
            .unwrap_or_else(default_frequency)
            .trim()
            .to_string(),
        time: input.time.unwrap_or_else(default_time).trim().to_string(),
        weekdays: input.weekdays.unwrap_or_default(),
        notify: input
            .notify
            .unwrap_or_else(default_notify)
            .trim()
            .to_string(),
        missed_run_policy: normalize_missed_run_policy(input.missed_run_policy.as_deref())?,
        created_at: now,
        updated_at: now,
        last_run_at: None,
        next_run_at: input.next_run_at,
        interval_minutes: normalize_interval_minutes(input.interval_minutes)?,
        skill_ids: input.skill_ids.unwrap_or_default(),
    };
    update_automations(|list| {
        list.insert(0, auto.clone());
        Ok(auto)
    })
}

pub fn update_automation(id: &str, input: AutomationInput) -> Result<Automation, String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("title empty".into());
    }
    let prompt = input.prompt.trim();
    if prompt.is_empty() {
        return Err("prompt empty".into());
    }
    let title = title.to_string();
    let prompt = prompt.to_string();
    update_automations(|list| {
        let auto = list
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| "automation not found".to_string())?;
        auto.title = title;
        auto.prompt = prompt;
        if let Some(e) = input.enabled {
            auto.enabled = e;
        }
        auto.project_id = input.project_id;
        auto.model_id = input.model_id;
        auto.effort = input.effort;
        if let Some(f) = input.frequency {
            auto.frequency = f.trim().to_string();
        }
        if let Some(t) = input.time {
            auto.time = t.trim().to_string();
        }
        if let Some(w) = input.weekdays {
            auto.weekdays = w;
        }
        if let Some(n) = input.notify {
            auto.notify = n.trim().to_string();
        }
        if let Some(policy) = input.missed_run_policy {
            auto.missed_run_policy = normalize_missed_run_policy(Some(&policy))?;
        }
        if input.next_run_at.is_some() {
            auto.next_run_at = input.next_run_at;
        }
        if input.interval_minutes.is_some() {
            auto.interval_minutes = normalize_interval_minutes(input.interval_minutes)?;
        }
        if let Some(skills) = input.skill_ids {
            auto.skill_ids = skills;
        }
        auto.updated_at = Utc::now();
        Ok(auto.clone())
    })
}

pub fn set_automation_enabled(id: &str, enabled: bool) -> Result<Automation, String> {
    update_automations(|list| {
        let auto = list
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| "automation not found".to_string())?;
        auto.enabled = enabled;
        auto.updated_at = Utc::now();
        Ok(auto.clone())
    })
}

pub fn mark_automation_run(
    id: &str,
    last_run_at: DateTime<Utc>,
    next_run_at: Option<DateTime<Utc>>,
) -> Result<Automation, String> {
    update_automations(|list| {
        let auto = list
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| "automation not found".to_string())?;
        auto.last_run_at = Some(last_run_at);
        auto.next_run_at = next_run_at;
        auto.updated_at = Utc::now();
        Ok(auto.clone())
    })
}

pub fn delete_automation(id: &str) -> Result<(), String> {
    update_automations(|list| {
        let before = list.len();
        list.retain(|a| a.id != id);
        if list.len() == before {
            return Err("automation not found".into());
        }
        Ok(())
    })
}

/// Load app secrets (API keys). Backend-agnostic: OS keychain preferred, file fallback.
/// See [`crate::secrets`] for migration and storage details. Callers must not log values.
pub fn load_secrets() -> SecretsFile {
    crate::secrets::load_secrets()
}

/// Persist app secrets. Prefer OS keychain for API keys; metadata may remain in secrets.json.
pub fn save_secrets(s: &SecretsFile) -> Result<(), String> {
    crate::secrets::save_secrets(s)
}

/// Redact secrets from a string for logs/Doctor export.
pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    let secrets = load_secrets();
    for key in [
        secrets.official_api_key.as_deref(),
        secrets.relay_api_key.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if key.len() >= 8 {
            out = out.replace(key, "[REDACTED]");
        }
    }
    // common token scrubbing without regex crate
    let mut cleaned = String::with_capacity(out.len());
    for word in out.split_whitespace() {
        if word.len() > 20
            && (word.starts_with("sk-") || word.starts_with("xai-") || word.contains("Bearer"))
        {
            cleaned.push_str("[REDACTED]");
        } else {
            cleaned.push_str(word);
        }
        cleaned.push(' ');
    }
    cleaned
}

fn global_prefs(settings: &AppSettings) -> (String, String, String, String) {
    (
        settings
            .model_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| crate::providers::OFFICIAL_DEFAULT_MODEL.into()),
        settings
            .effort
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "medium".into()),
        if settings.mode.trim().is_empty() {
            "agent".into()
        } else {
            settings.mode.clone()
        },
        if settings.permission_policy.trim().is_empty() {
            "ask".into()
        } else {
            settings.permission_policy.clone()
        },
    )
}

/// Resolve effective composer prefs for the active project/session + configured scope.
///
/// Model / effort / mode follow `composer_prefs_scope`.
/// Permission always cascades session → project → global (L10), and untrusted
/// projects force Ask regardless of stored tiers.
pub fn resolve_composer_prefs(project_id: Option<&str>, session_id: Option<&str>) -> ComposerPrefs {
    use crate::permission::effective_permission_policy;

    let settings = load_settings();
    let scope = ComposerPrefsScope::parse(&settings.composer_prefs_scope);
    let (g_model, g_effort, g_mode, g_policy) = global_prefs(&settings);

    let sess = session_id.and_then(|id| load_sessions_index().into_iter().find(|s| s.id == id));
    let proj = sess
        .as_ref()
        .and_then(|s| s.project_id.as_deref())
        .or(project_id)
        .and_then(|id| load_projects().into_iter().find(|p| p.id == id));

    // Permission: always cascade (independent of model/effort memory scope).
    let permission_policy = effective_permission_policy(
        &g_policy,
        proj.as_ref().map(|p| p.trusted),
        proj.as_ref().and_then(|p| p.permission_policy.as_deref()),
        sess.as_ref().and_then(|s| s.permission_policy.as_deref()),
    )
    .as_str()
    .to_string();

    match scope {
        ComposerPrefsScope::Global => ComposerPrefs {
            model_id: g_model,
            effort: g_effort,
            mode: g_mode,
            permission_policy,
            scope: scope.as_str().into(),
            source: "global".into(),
        },
        ComposerPrefsScope::Project => {
            if let Some(p) = proj {
                ComposerPrefs {
                    model_id: p.model_id.filter(|s| !s.is_empty()).unwrap_or(g_model),
                    effort: p.effort.filter(|s| !s.is_empty()).unwrap_or(g_effort),
                    mode: p.mode.filter(|s| !s.is_empty()).unwrap_or(g_mode),
                    permission_policy,
                    scope: scope.as_str().into(),
                    source: "project".into(),
                }
            } else {
                ComposerPrefs {
                    model_id: g_model,
                    effort: g_effort,
                    mode: g_mode,
                    permission_policy,
                    scope: scope.as_str().into(),
                    source: "global".into(),
                }
            }
        }
        ComposerPrefsScope::Session => {
            let p_model = proj
                .as_ref()
                .and_then(|p| p.model_id.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or(g_model.clone());
            let p_effort = proj
                .as_ref()
                .and_then(|p| p.effort.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or(g_effort.clone());
            let p_mode = proj
                .as_ref()
                .and_then(|p| p.mode.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or(g_mode.clone());

            if let Some(s) = sess {
                ComposerPrefs {
                    model_id: s.model_id.filter(|x| !x.is_empty()).unwrap_or(p_model),
                    effort: s.effort.filter(|x| !x.is_empty()).unwrap_or(p_effort),
                    mode: s.mode.filter(|x| !x.is_empty()).unwrap_or(p_mode),
                    permission_policy,
                    scope: scope.as_str().into(),
                    source: "session".into(),
                }
            } else {
                ComposerPrefs {
                    model_id: p_model,
                    effort: p_effort,
                    mode: p_mode,
                    permission_policy,
                    scope: scope.as_str().into(),
                    source: if proj.is_some() { "project" } else { "global" }.into(),
                }
            }
        }
    }
}

/// Persist a partial composer prefs update at the configured scope.
pub fn save_composer_prefs(
    project_id: Option<&str>,
    session_id: Option<&str>,
    model_id: Option<String>,
    effort: Option<String>,
    mode: Option<String>,
    permission_policy: Option<String>,
) -> Result<ComposerPrefs, String> {
    let settings = load_settings();
    let scope = ComposerPrefsScope::parse(&settings.composer_prefs_scope);

    match scope {
        ComposerPrefsScope::Global => {
            patch_global_composer_prefs(model_id, effort, mode, permission_policy)?;
        }
        ComposerPrefsScope::Project => {
            let pid = project_id.filter(|s| !s.is_empty());
            if let Some(pid) = pid {
                let mut list = load_projects();
                if let Some(p) = list.iter_mut().find(|p| p.id == pid) {
                    if let Some(v) = model_id.clone() {
                        p.model_id = Some(v);
                    }
                    if let Some(v) = effort.clone() {
                        p.effort = Some(v);
                    }
                    if let Some(v) = mode.clone() {
                        p.mode = Some(v);
                    }
                    if let Some(v) = permission_policy.clone() {
                        p.permission_policy = Some(v);
                    }
                    save_projects(&list)?;
                }
            }
            // Always mirror to global so orphan UIs / new projects still have a default.
            patch_global_composer_prefs(model_id, effort, mode, permission_policy)?;
        }
        ComposerPrefsScope::Session => {
            let sid = session_id.filter(|s| !s.is_empty());
            if let Some(sid) = sid {
                let updated = update_sessions_index(|list| {
                    let Some(sess) = list.iter_mut().find(|s| s.id == sid) else {
                        return Ok(false);
                    };
                    if let Some(v) = model_id.clone() {
                        sess.model_id = Some(v);
                    }
                    if let Some(v) = effort.clone() {
                        sess.effort = Some(v);
                    }
                    if let Some(v) = mode.clone() {
                        sess.mode = Some(v);
                    }
                    if let Some(v) = permission_policy.clone() {
                        sess.permission_policy = Some(v);
                    }
                    sess.updated_at = Utc::now();
                    Ok(true)
                })?;
                if !updated {
                    // No session row yet — fall back to global so the chip still sticks.
                    patch_global_composer_prefs(model_id, effort, mode, permission_policy)?;
                }
            } else {
                patch_global_composer_prefs(model_id, effort, mode, permission_policy)?;
            }
        }
    }

    Ok(resolve_composer_prefs(project_id, session_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn settings_test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sunsetz-settings-{label}-{}-{}.json",
            std::process::id(),
            Uuid::new_v4()
        ))
    }

    fn remove_settings_test_path(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(crate::store_lock::lock_path_for(path));
    }

    fn remove_transaction_test_path(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(crate::store_lock::lock_path_for(path));
    }

    #[test]
    fn redact_scrubs_long_tokenish() {
        let s = "header Bearer sk-abcdefghijklmnopqrstuvwxyz123456 tail";
        let r = redact_text(s);
        assert!(
            !r.contains("sk-abcdefghijklmnopqrstuvwxyz123456")
                || r.contains("REDACTED")
                || r.contains("sk-")
        );
        // at least function is callable
        assert!(!r.is_empty());
    }

    #[test]
    fn session_preview_summary_is_visible_bounded_and_collapsed() {
        let long = format!("  first\n\nsecond   {}  ", "字".repeat(300));
        let summary = bounded_visible_summary(&long).expect("summary");
        assert!(summary.starts_with("first second"));
        assert!(summary.chars().count() <= SESSION_PREVIEW_TEXT_LIMIT + 1);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn session_preview_summary_rejects_empty_text() {
        assert_eq!(bounded_visible_summary(" \n\t "), None);
    }

    #[test]
    fn default_settings_independent_mode() {
        let s = AppSettings::default();
        assert_eq!(s.session_data_mode, "independent");
        assert_eq!(s.permission_policy, "ask");
        assert_eq!(s.theme, "dark");
        assert_eq!(s.max_concurrent_agents, 3);
        assert_eq!(s.agent_idle_minutes, 30);
        assert_eq!(s.stream_stall_seconds, 120);
        assert_eq!(s.runtime_backend, "sunsetz");
    }

    #[test]
    fn settings_patch_supports_explicit_null_and_rejects_unknown_fields() {
        let path = settings_test_path("patch-validation");
        let mut initial = AppSettings::default();
        initial.manual_cli_path = Some("/tmp/runtime".into());
        write_json(&path, &initial).unwrap();

        let (_, patched) = patch_settings_at(
            &path,
            serde_json::json!({"manualCliPath": null, "theme": "light"}),
        )
        .unwrap();
        assert_eq!(patched.manual_cli_path, None);
        assert_eq!(patched.theme, "light");

        let before = fs::read_to_string(&path).unwrap();
        let error = patch_settings_at(&path, serde_json::json!({"apiKey": "secret"})).unwrap_err();
        assert_eq!(error, "SETTINGS_PATCH_UNKNOWN_FIELD:apiKey");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        remove_settings_test_path(&path);
    }

    #[test]
    fn concurrent_settings_patches_keep_disjoint_fields() {
        let path = settings_test_path("patch-concurrent");
        write_json(&path, &AppSettings::default()).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for patch in [
            serde_json::json!({"theme": "light"}),
            serde_json::json!({"locale": "en"}),
        ] {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                patch_settings_at(&path, patch).unwrap();
            }));
        }
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        let final_settings: AppSettings = read_json(&path);
        assert_eq!(final_settings.theme, "light");
        assert_eq!(final_settings.locale, "en");
        remove_settings_test_path(&path);
    }

    #[test]
    fn legacy_full_object_rejects_stale_multi_field_snapshot() {
        let path = settings_test_path("legacy-stale");
        let initial = AppSettings::default();
        write_json(&path, &initial).unwrap();

        let mut stale_request = initial.clone();
        stale_request.theme = "light".into();
        patch_settings_at(&path, serde_json::json!({"locale": "en"})).unwrap();

        let current: AppSettings = read_json(&path);
        let error = legacy_settings_patch_v1(&current, &stale_request).unwrap_err();
        assert!(error.starts_with("SETTINGS_SET_REJECTED_USE_SETTINGS_PATCH_V1"));
        let unchanged: AppSettings = read_json(&path);
        assert_eq!(unchanged.locale, "en");
        assert_eq!(unchanged.theme, "dark");

        let mut single_field_request = current;
        single_field_request.theme = "light".into();
        assert_eq!(
            legacy_settings_patch_v1(&unchanged, &single_field_request).unwrap(),
            Some(serde_json::json!({"theme": "light"}))
        );
        remove_settings_test_path(&path);
    }

    #[test]
    fn legacy_zero_diff_is_a_true_no_op_inside_keychain_transaction() {
        let path = settings_test_path("legacy-no-op");
        let transaction = path.with_extension("keychain-transaction");
        let requested = AppSettings::default();
        write_json(&path, &requested).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let (previous, current) = patch_legacy_settings_with_keychain_transaction_at(
            &path,
            &transaction,
            &requested,
            |_| panic!("no-diff legacy request must not migrate keychain state"),
        )
        .unwrap();

        assert_eq!(previous.theme, current.theme);
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        remove_settings_test_path(&path);
        remove_transaction_test_path(&transaction);
    }

    #[test]
    fn keychain_true_false_migrations_are_serialized() {
        let path = settings_test_path("keychain-serialized");
        let transaction = path.with_extension("keychain-transaction");
        write_json(&path, &AppSettings::default()).unwrap();

        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_path = path.clone();
        let first_transaction = transaction.clone();
        let first = std::thread::spawn(move || {
            patch_settings_with_keychain_transaction_at(
                &first_path,
                &first_transaction,
                serde_json::json!({"storeApiKeysInKeychain": true}),
                |enabled| {
                    assert!(enabled);
                    first_entered_tx.send(()).unwrap();
                    release_first_rx.recv().unwrap();
                    Ok(())
                },
            )
        });
        first_entered_rx.recv().unwrap();

        let (second_started_tx, second_started_rx) = mpsc::channel();
        let (second_finished_tx, second_finished_rx) = mpsc::channel();
        let second_path = path.clone();
        let second_transaction = transaction.clone();
        let second = std::thread::spawn(move || {
            second_started_tx.send(()).unwrap();
            let result = patch_settings_with_keychain_transaction_at(
                &second_path,
                &second_transaction,
                serde_json::json!({"storeApiKeysInKeychain": false}),
                |enabled| {
                    assert!(!enabled);
                    Ok(())
                },
            );
            second_finished_tx.send(()).unwrap();
            result
        });
        second_started_rx.recv().unwrap();
        assert!(second_finished_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());

        release_first_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        let final_settings: AppSettings = read_json(&path);
        assert!(!final_settings.store_api_keys_in_keychain);

        remove_settings_test_path(&path);
        remove_transaction_test_path(&transaction);
    }

    #[test]
    fn failed_keychain_migration_cannot_aba_rollback_same_value_writer() {
        let path = settings_test_path("keychain-rollback-aba");
        let transaction = path.with_extension("keychain-transaction");
        write_json(&path, &AppSettings::default()).unwrap();

        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_path = path.clone();
        let first_transaction = transaction.clone();
        let first = std::thread::spawn(move || {
            patch_settings_with_keychain_transaction_at(
                &first_path,
                &first_transaction,
                serde_json::json!({"storeApiKeysInKeychain": true}),
                |_| {
                    first_entered_tx.send(()).unwrap();
                    release_first_rx.recv().unwrap();
                    Err("injected migration failure".into())
                },
            )
        });
        first_entered_rx.recv().unwrap();

        let (second_started_tx, second_started_rx) = mpsc::channel();
        let (second_finished_tx, second_finished_rx) = mpsc::channel();
        let second_path = path.clone();
        let second_transaction = transaction.clone();
        let second = std::thread::spawn(move || {
            second_started_tx.send(()).unwrap();
            let mut requested = AppSettings::default();
            requested.store_api_keys_in_keychain = true;
            let result = patch_legacy_settings_with_keychain_transaction_at(
                &second_path,
                &second_transaction,
                &requested,
                |enabled| {
                    assert!(enabled);
                    Ok(())
                },
            );
            second_finished_tx.send(()).unwrap();
            result
        });
        second_started_rx.recv().unwrap();
        assert!(second_finished_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());

        release_first_tx.send(()).unwrap();
        assert!(first
            .join()
            .unwrap()
            .unwrap_err()
            .contains("injected migration failure"));
        second.join().unwrap().unwrap();
        let final_settings: AppSettings = read_json(&path);
        assert!(final_settings.store_api_keys_in_keychain);

        remove_settings_test_path(&path);
        remove_transaction_test_path(&transaction);
    }

    #[test]
    fn keychain_rollback_compares_only_its_field() {
        let path = settings_test_path("keychain-cas");
        let mut initial = AppSettings::default();
        initial.theme = "light".into();
        initial.store_api_keys_in_keychain = true;
        write_json(&path, &initial).unwrap();

        assert!(compare_restore_keychain_preference_at(&path, true, false).unwrap());
        assert!(!compare_restore_keychain_preference_at(&path, true, false).unwrap());
        let current: AppSettings = read_json(&path);
        assert_eq!(current.theme, "light");
        assert!(!current.store_api_keys_in_keychain);
        remove_settings_test_path(&path);
    }

    #[test]
    fn message_attachments_survive_disk_round_trip_and_old_rows_stay_none() {
        let root = std::env::temp_dir().join(format!(
            "sunsetz-message-attachment-store-{}",
            Uuid::new_v4()
        ));
        let path = root.join("messages.json");
        let message = ChatMessageStored {
            id: "message-with-attachment".into(),
            role: "user".into(),
            content: "hello".into(),
            thought: None,
            created_at: Utc::now(),
            is_error: false,
            attachments: Some(vec![MessageAttachmentStored {
                path: "/tmp/reference.png".into(),
                name: "Reference image".into(),
                is_dir: true,
            }]),
            marker: None,
        };
        write_json(&path, &vec![message]).unwrap();
        let loaded: Vec<ChatMessageStored> = read_json_recover(&path);
        let attachment = loaded[0].attachments.as_ref().unwrap().first().unwrap();
        assert_eq!(attachment.path, "/tmp/reference.png");
        assert_eq!(attachment.name, "Reference image");
        assert!(attachment.is_dir);

        let old_row = serde_json::json!([{
            "id": "legacy-message",
            "role": "user",
            "content": "legacy",
            "thought": null,
            "createdAt": Utc::now(),
            "isError": false
        }]);
        fs::write(&path, serde_json::to_vec_pretty(&old_row).unwrap()).unwrap();
        let legacy_loaded: Vec<ChatMessageStored> = read_json_recover(&path);
        assert!(legacy_loaded[0].attachments.is_none());

        fs::remove_dir_all(root).unwrap();
    }

    struct IsolatedHome {
        home: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl IsolatedHome {
        fn new(label: &str) -> Self {
            let lock = crate::runtime_compat::lock_test_process_env();
            let home = std::env::temp_dir().join(format!(
                "sunsetz-store-{label}-{}-{}",
                std::process::id(),
                Uuid::new_v4()
            ));
            fs::create_dir_all(&home).unwrap();
            std::env::set_var("SUNSETZ_HOME", &home);
            Self { home, _lock: lock }
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            std::env::remove_var("SUNSETZ_HOME");
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    #[test]
    fn remove_project_deletes_its_sessions_and_keeps_the_folder() {
        let isolated = IsolatedHome::new("remove-project");
        let folder = isolated.home.join("proj");
        fs::create_dir_all(&folder).unwrap();
        let project = add_project(folder.display().to_string(), true).expect("add");
        let orphan = create_session(None, Some("orphan".into()), false).expect("orphan");
        let owned =
            create_session(Some(project.id.clone()), Some("owned".into()), false).expect("owned");
        let archived = create_session(Some(project.id.clone()), Some("archived".into()), false)
            .expect("archived");
        update_sessions_index(|list| {
            if let Some(session) = list.iter_mut().find(|item| item.id == archived.id) {
                session.archived = true;
            }
            Ok(())
        })
        .expect("archive");

        remove_project(&project.id).expect("remove");

        assert!(folder.is_dir(), "disk folder must stay");
        assert!(load_projects().iter().all(|item| item.id != project.id));
        let remaining = load_sessions_index();
        assert!(remaining.iter().any(|item| item.id == orphan.id));
        assert!(!remaining.iter().any(|item| item.id == owned.id));
        assert!(!remaining.iter().any(|item| item.id == archived.id));
        assert!(!session_dir(&owned.id).exists());
        assert!(!session_dir(&archived.id).exists());
        assert!(session_dir(&orphan.id).exists());
    }
}
