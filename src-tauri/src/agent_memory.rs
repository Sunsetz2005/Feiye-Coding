//! Bounded, Host-owned auto memory for the Sunsetz kernel.
//!
//! This is a separate contract from review-only Memory candidates and from FTS
//! session search. The agent may add, replace, or remove notes and user-profile
//! entries without the permission dock. The live snapshot is injected into the
//! Sunsetz system prompt when enabled. Visible journals never store the body.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::store_lock::update_json_locked;

pub const AGENT_MEMORY_VERSION: u8 = 1;
pub const MAX_NOTES_CHARS: usize = 2_200;
pub const MAX_USER_PROFILE_CHARS: usize = 1_375;
pub const MAX_ENTRY_CHARS: usize = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMemoryTargetV1 {
    Notes,
    UserProfile,
}

impl AgentMemoryTargetV1 {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "notes" | "memory" | "note" => Ok(Self::Notes),
            "user_profile" | "user" | "profile" => Ok(Self::UserProfile),
            _ => Err("memory target must be notes or user_profile".into()),
        }
    }

    fn cap(self) -> usize {
        match self {
            Self::Notes => MAX_NOTES_CHARS,
            Self::UserProfile => MAX_USER_PROFILE_CHARS,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Notes => "NOTES",
            Self::UserProfile => "USER PROFILE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMemoryEntryV1 {
    pub id: String,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMemoryStoreV1 {
    pub version: u8,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub notes: Vec<AgentMemoryEntryV1>,
    #[serde(default)]
    pub user_profile: Vec<AgentMemoryEntryV1>,
}

fn default_enabled() -> bool {
    true
}

impl Default for AgentMemoryStoreV1 {
    fn default() -> Self {
        Self {
            version: AGENT_MEMORY_VERSION,
            enabled: true,
            notes: Vec::new(),
            user_profile: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMemoryMutationV1 {
    pub action: String,
    pub target: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub old_text: Option<String>,
}

fn store_path() -> std::path::PathBuf {
    crate::paths::app_data_root().join("agent-memory.v1.json")
}

pub fn load() -> Result<AgentMemoryStoreV1, String> {
    load_at(&store_path())
}

pub fn is_enabled() -> bool {
    load().map(|store| store.enabled).unwrap_or(true)
}

pub fn snapshot_prompt() -> String {
    match load() {
        Ok(store) if store.enabled => render_snapshot(&store),
        _ => String::new(),
    }
}

pub fn set_enabled(enabled: bool) -> Result<AgentMemoryStoreV1, String> {
    update_at(&store_path(), |store| {
        store.enabled = enabled;
        Ok(())
    })
}

pub fn mutate(request: AgentMemoryMutationV1) -> Result<AgentMemoryStoreV1, String> {
    mutate_at(&store_path(), request, &crate::store::redact_text)
}

pub fn clear_entries() -> Result<AgentMemoryStoreV1, String> {
    update_at(&store_path(), |store| {
        store.notes.clear();
        store.user_profile.clear();
        Ok(())
    })
}

pub fn apply_tool(arguments: &serde_json::Value) -> String {
    let action = arguments
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if action == "list" {
        return match load() {
            Ok(store) => render_tool_list(&store),
            Err(error) => error,
        };
    }
    let request = AgentMemoryMutationV1 {
        action: action.clone(),
        target: arguments
            .get("target")
            .and_then(|value| value.as_str())
            .unwrap_or("notes")
            .to_string(),
        content: arguments
            .get("content")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        old_text: arguments
            .get("old_text")
            .or_else(|| arguments.get("oldText"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    };
    match mutate(request) {
        Ok(store) => render_tool_list(&store),
        Err(error) => error,
    }
}

fn load_at(path: &std::path::Path) -> Result<AgentMemoryStoreV1, String> {
    if !path.exists() {
        return Ok(AgentMemoryStoreV1::default());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("agent memory is unreadable: {error}"))?;
    if raw.trim().is_empty() {
        return Ok(AgentMemoryStoreV1::default());
    }
    let store: AgentMemoryStoreV1 = serde_json::from_str(&raw)
        .map_err(|error| format!("agent memory is invalid JSON: {error}"))?;
    if store.version != AGENT_MEMORY_VERSION {
        return Err("unsupported agent memory version".into());
    }
    Ok(store)
}

fn update_at(
    path: &std::path::Path,
    update: impl FnOnce(&mut AgentMemoryStoreV1) -> Result<(), String>,
) -> Result<AgentMemoryStoreV1, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("agent memory directory is unavailable: {error}"))?;
    }
    update_json_locked(path, AgentMemoryStoreV1::default, |store| {
        if store.version != AGENT_MEMORY_VERSION {
            store.version = AGENT_MEMORY_VERSION;
        }
        update(store)?;
        Ok(store.clone())
    })
}

fn mutate_at(
    path: &std::path::Path,
    request: AgentMemoryMutationV1,
    redact: &dyn Fn(&str) -> String,
) -> Result<AgentMemoryStoreV1, String> {
    let target = AgentMemoryTargetV1::parse(&request.target)?;
    let action = request.action.trim().to_ascii_lowercase();
    update_at(path, |store| {
        match action.as_str() {
            "add" => {
                let content = normalize_entry(request.content.as_deref().unwrap_or(""), redact)?;
                let entries = entries_mut(store, target);
                if entries.iter().any(|entry| entry.content == content) {
                    return Ok(());
                }
                ensure_fits(entries, target, content.chars().count(), 0)?;
                entries.push(AgentMemoryEntryV1 {
                    id: format!("agent-memory-{}", Uuid::new_v4()),
                    content,
                    updated_at: Utc::now(),
                });
            }
            "replace" => {
                let old_text = request
                    .old_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "replace requires old_text".to_string())?;
                let content = normalize_entry(request.content.as_deref().unwrap_or(""), redact)?;
                let index = unique_match(entries_mut(store, target), old_text)?;
                let previous = entries_mut(store, target)[index].content.chars().count();
                ensure_fits(
                    entries_mut(store, target),
                    target,
                    content.chars().count(),
                    previous,
                )?;
                let entry = &mut entries_mut(store, target)[index];
                entry.content = content;
                entry.updated_at = Utc::now();
            }
            "remove" => {
                let old_text = request
                    .old_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "remove requires old_text".to_string())?;
                let index = unique_match(entries_mut(store, target), old_text)?;
                entries_mut(store, target).remove(index);
            }
            _ => return Err("memory action must be add, replace, remove, or list".into()),
        }
        Ok(())
    })
}

fn entries_mut(
    store: &mut AgentMemoryStoreV1,
    target: AgentMemoryTargetV1,
) -> &mut Vec<AgentMemoryEntryV1> {
    match target {
        AgentMemoryTargetV1::Notes => &mut store.notes,
        AgentMemoryTargetV1::UserProfile => &mut store.user_profile,
    }
}

fn unique_match(entries: &[AgentMemoryEntryV1], old_text: &str) -> Result<usize, String> {
    let matches: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.content.contains(old_text))
        .map(|(index, _)| index)
        .collect();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err("no memory entry matches old_text".into()),
        _ => Err("old_text matches more than one memory entry".into()),
    }
}

fn ensure_fits(
    entries: &[AgentMemoryEntryV1],
    target: AgentMemoryTargetV1,
    incoming: usize,
    replacing: usize,
) -> Result<(), String> {
    let used = total_chars(entries).saturating_sub(replacing);
    let cap = target.cap();
    if used.saturating_add(incoming) > cap {
        let listing = entries
            .iter()
            .map(|entry| entry.content.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!(
            "Memory at {used}/{cap} chars. Adding this entry ({incoming} chars) would exceed the limit. Consolidate now: use replace to merge overlapping entries or remove stale ones, then retry. current_entries: [{listing}]"
        ));
    }
    Ok(())
}

fn total_chars(entries: &[AgentMemoryEntryV1]) -> usize {
    entries
        .iter()
        .map(|entry| entry.content.chars().count())
        .sum()
}

fn normalize_entry(content: &str, redact: &dyn Fn(&str) -> String) -> Result<String, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("memory content is empty".into());
    }
    if trimmed.chars().count() > MAX_ENTRY_CHARS {
        return Err(format!("memory entry exceeds {MAX_ENTRY_CHARS} characters"));
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err("memory content contains unsupported control characters".into());
    }
    if redact(trimmed).contains("[REDACTED]") {
        return Err("memory content contains sensitive material".into());
    }
    Ok(trimmed.to_string())
}

fn render_snapshot(store: &AgentMemoryStoreV1) -> String {
    let mut out = String::new();
    append_snapshot_block(&mut out, AgentMemoryTargetV1::Notes, &store.notes);
    append_snapshot_block(
        &mut out,
        AgentMemoryTargetV1::UserProfile,
        &store.user_profile,
    );
    out
}

fn append_snapshot_block(
    out: &mut String,
    target: AgentMemoryTargetV1,
    entries: &[AgentMemoryEntryV1],
) {
    if entries.is_empty() {
        return;
    }
    let used = total_chars(entries);
    let cap = target.cap();
    let pct = if cap == 0 { 0 } else { (used * 100) / cap };
    out.push_str(&format!(
        "\n\n{bar}\n{label} [{pct}% — {used}/{cap} chars]\n{bar}\n",
        bar = "══════════════════════════════════════════════",
        label = target.label(),
    ));
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            out.push_str("\n§\n");
        }
        out.push_str(&entry.content);
    }
}

fn render_tool_list(store: &AgentMemoryStoreV1) -> String {
    fn block(label: &str, entries: &[AgentMemoryEntryV1], cap: usize) -> String {
        let used = total_chars(entries);
        let mut lines = vec![format!("{label} {used}/{cap}")];
        if entries.is_empty() {
            lines.push("(empty)".into());
        } else {
            for entry in entries {
                lines.push(format!("- {}", entry.content));
            }
        }
        lines.join("\n")
    }
    format!(
        "enabled={}\n{}\n{}",
        store.enabled,
        block("NOTES", &store.notes, MAX_NOTES_CHARS),
        block("USER PROFILE", &store.user_profile, MAX_USER_PROFILE_CHARS)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("sunsetz-agent-memory-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> std::path::PathBuf {
            self.0.join("agent-memory.v1.json")
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn identity_redact(text: &str) -> String {
        text.to_string()
    }
    fn redacts_keys(text: &str) -> String {
        if text.contains("sk-") {
            "[REDACTED]".into()
        } else {
            text.to_string()
        }
    }

    #[test]
    fn add_replace_remove_and_duplicate() {
        let temp = TempDir::new();
        let path = temp.file();
        let store = mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "add".into(),
                target: "notes".into(),
                content: Some("Project uses pnpm.".into()),
                old_text: None,
            },
            &identity_redact,
        )
        .unwrap();
        assert_eq!(store.notes.len(), 1);
        let again = mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "add".into(),
                target: "notes".into(),
                content: Some("Project uses pnpm.".into()),
                old_text: None,
            },
            &identity_redact,
        )
        .unwrap();
        assert_eq!(again.notes.len(), 1);
        mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "replace".into(),
                target: "notes".into(),
                old_text: Some("pnpm".into()),
                content: Some("Project uses pnpm and cargo.".into()),
            },
            &identity_redact,
        )
        .unwrap();
        mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "remove".into(),
                target: "notes".into(),
                old_text: Some("cargo".into()),
                content: None,
            },
            &identity_redact,
        )
        .unwrap();
        assert!(load_at(&path).unwrap().notes.is_empty());
    }

    #[test]
    fn overflow_is_explicit_and_secrets_fail_closed() {
        let temp = TempDir::new();
        let path = temp.file();
        let chunk = "n".repeat(760);
        for _ in 0..2 {
            mutate_at(
                &path,
                AgentMemoryMutationV1 {
                    action: "add".into(),
                    target: "notes".into(),
                    content: Some(format!("{chunk}-{}", Uuid::new_v4())),
                    old_text: None,
                },
                &identity_redact,
            )
            .unwrap();
        }
        let error = mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "add".into(),
                target: "notes".into(),
                content: Some(format!("{}-{}", "x".repeat(700), Uuid::new_v4())),
                old_text: None,
            },
            &identity_redact,
        )
        .unwrap_err();
        assert!(error.contains("exceed"), "{error}");
        assert!(error.contains("current_entries"), "{error}");
        let secret = mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "add".into(),
                target: "user_profile".into(),
                content: Some("key sk-secret-material".into()),
                old_text: None,
            },
            &redacts_keys,
        )
        .unwrap_err();
        assert!(secret.contains("sensitive"), "{secret}");
    }

    #[test]
    fn unique_substring_required_for_replace() {
        let temp = TempDir::new();
        let path = temp.file();
        mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "add".into(),
                target: "notes".into(),
                content: Some("Alpha uses rust.".into()),
                old_text: None,
            },
            &identity_redact,
        )
        .unwrap();
        mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "add".into(),
                target: "notes".into(),
                content: Some("Beta uses rust.".into()),
                old_text: None,
            },
            &identity_redact,
        )
        .unwrap();
        let error = mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "replace".into(),
                target: "notes".into(),
                old_text: Some("uses rust".into()),
                content: Some("Both use rustc.".into()),
            },
            &identity_redact,
        )
        .unwrap_err();
        assert!(error.contains("more than one"), "{error}");
    }

    #[test]
    fn snapshot_omits_empty_stores_and_lists_usage() {
        let store = AgentMemoryStoreV1 {
            notes: vec![AgentMemoryEntryV1 {
                id: "1".into(),
                content: "Uses pnpm.".into(),
                updated_at: Utc::now(),
            }],
            ..AgentMemoryStoreV1::default()
        };
        let snap = render_snapshot(&store);
        assert!(snap.contains("NOTES"));
        assert!(snap.contains("Uses pnpm."));
        assert!(!snap.contains("USER PROFILE"));
        let listed = render_tool_list(&store);
        assert!(listed.contains("enabled=true"));
        assert!(listed.contains("USER PROFILE 0/1375"));
    }

    #[test]
    fn apply_tool_list_and_add_round_trip_json() {
        let temp = TempDir::new();
        let path = temp.file();
        let store = mutate_at(
            &path,
            AgentMemoryMutationV1 {
                action: "add".into(),
                target: "user".into(),
                content: Some("Prefers concise answers.".into()),
                old_text: None,
            },
            &identity_redact,
        )
        .unwrap();
        assert_eq!(store.user_profile.len(), 1);
        let _ = json!({"action":"list"});
    }
}
