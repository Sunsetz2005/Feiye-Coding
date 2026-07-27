//! Live **model** catalog from Grok CLI cache only.
//!
//! Providers / relays are **channels** managed on the Providers settings page —
//! they must never appear as selectable model chips.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths::resolve_agent_grok_home;
use crate::store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    pub reasoning_efforts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModel {
    pub id: String,
    pub label: String,
    /// Always "official" for catalog entries (providers are not models).
    pub source: String,
    #[serde(default)]
    pub is_default: bool,
    /// Only controls that map to a concrete Runtime argument are advertised.
    pub capabilities: Option<ModelCapabilities>,
}

fn known_model_capabilities(model_id: &str) -> Option<ModelCapabilities> {
    match model_id {
        // This is the only model whose reasoning-effort contract is currently
        // known to the Host. Unknown catalog entries must not inherit controls
        // merely because they happen to share the same cache shape.
        "grok-4.5" => Some(ModelCapabilities {
            reasoning_efforts: vec!["low".into(), "medium".into(), "high".into()],
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModelsResult {
    pub models: Vec<AvailableModel>,
    pub default_model_id: String,
    pub origin: Option<String>,
    pub fetched_at: Option<String>,
}

fn user_grok_home() -> PathBuf {
    crate::process_util::user_home().join(".grok")
}

fn read_models_cache(
    path: &PathBuf,
) -> Option<(BTreeMap<String, String>, Option<String>, Option<String>)> {
    let raw = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let models_obj = v.get("models")?.as_object()?;
    let mut map = BTreeMap::new();
    for (id, body) in models_obj {
        if id.trim().is_empty() {
            continue;
        }
        let hidden = body
            .pointer("/info/hidden")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        if hidden {
            continue;
        }
        // Skip entries that look like provider routes (have a custom base_url override
        // without being the official chat-proxy catalog shape). Official cache entries
        // expose info.model / info.name from cli-chat-proxy.
        let label = body
            .pointer("/info/name")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(id)
            .to_string();
        map.insert(id.clone(), label);
    }
    let origin = v
        .get("origin")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let fetched_at = v
        .get("fetched_at")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    Some((map, origin, fetched_at))
}

/// Models the user can select in the composer.
///
/// **Only** official Grok Build catalog IDs from `models_cache.json`.
/// Custom providers (`[model.*]` in config.toml) are channels — switch them under
/// Settings → Account → Providers, not here.
pub fn list_available_models() -> AvailableModelsResult {
    let settings = store::load_settings();
    let agent_home = resolve_agent_grok_home(&settings.session_data_mode);

    let mut by_id: BTreeMap<String, AvailableModel> = BTreeMap::new();
    let mut origin = None;
    let mut fetched_at = None;

    // Prefer agent-home cache (GROK_HOME for independent mode), then ~/.grok.
    // Do NOT merge agent config.toml [model.*] provider routes into this list.
    for cache in [
        agent_home.join("models_cache.json"),
        user_grok_home().join("models_cache.json"),
    ] {
        if let Some((map, o, f)) = read_models_cache(&cache) {
            if origin.is_none() {
                origin = o;
            }
            if fetched_at.is_none() {
                fetched_at = f;
            }
            for (id, label) in map {
                let capabilities = known_model_capabilities(&id);
                by_id.entry(id.clone()).or_insert(AvailableModel {
                    id,
                    label: crate::runtime_compat::public_model_label(&label),
                    source: "official".into(),
                    is_default: false,
                    capabilities,
                });
            }
            if !by_id.is_empty() {
                break;
            }
        }
    }

    // Hard fallback — known-good official default when cache is empty / offline.
    if by_id.is_empty() {
        by_id.insert(
            "grok-4.5".into(),
            AvailableModel {
                id: "grok-4.5".into(),
                label: "Sunsetz 4.5".into(),
                source: "official".into(),
                is_default: true,
                capabilities: known_model_capabilities("grok-4.5"),
            },
        );
    }

    // Prefer catalog default over a stale settings.model_id that might be a
    // provider route id (e.g. "yunyi") from an older build.
    let preferred = by_id
        .keys()
        .find(|k| k.as_str() == "grok-4.5")
        .cloned()
        .or_else(|| {
            settings
                .model_id
                .clone()
                .filter(|s| by_id.contains_key(s))
        })
        .unwrap_or_else(|| {
            by_id
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "grok-4.5".into())
        });

    let mut models: Vec<AvailableModel> = by_id.into_values().collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    for m in &mut models {
        m.is_default = m.id == preferred;
    }

    AvailableModelsResult {
        models,
        default_model_id: preferred,
        origin,
        fetched_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_cache_parses_official_entry() {
        let dir = std::env::temp_dir().join(format!(
            "grok-app-models-test-{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("models_cache.json");
        fs::write(
            &path,
            r#"{
              "fetched_at": "2026-07-23T00:00:00Z",
              "origin": "https://cli-chat-proxy.grok.com/v1/models",
              "models": {
                "grok-4.5": {
                  "info": { "id": "grok-4.5", "name": "Grok 4.5", "hidden": false }
                }
              }
            }"#,
        )
        .unwrap();
        let (map, origin, _) = read_models_cache(&path).expect("cache");
        assert_eq!(map.get("grok-4.5").map(String::as_str), Some("Grok 4.5"));
        assert!(origin.unwrap().contains("cli-chat-proxy"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_models_do_not_inherit_reasoning_controls() {
        assert!(known_model_capabilities("grok-4.5").is_some());
        assert!(known_model_capabilities("future-model").is_none());
    }
}
