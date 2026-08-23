//! Clean-room, metadata-only cross-Agent capability exchange.
//!
//! The manifest describes stable interfaces and availability. It never carries
//! source code, prompts, executable payloads, filesystem paths, or third-party
//! implementation details.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityDescriptorV1 {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub version: Option<String>,
    pub source: String,
    pub interface_hash: String,
    pub implementation_omitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityManifestV1 {
    pub schema: String,
    pub version: u8,
    pub producer: String,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub capabilities: Vec<CapabilityDescriptorV1>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityManifestValidationV1 {
    pub version: u8,
    pub valid: bool,
    pub errors: Vec<String>,
}

fn interface_hash(
    id: &str,
    kind: &str,
    state: &str,
    version: Option<&str>,
    source: &str,
) -> String {
    let canonical = serde_json::json!({
        "id": id,
        "kind": kind,
        "state": state,
        "version": version,
        "source": source,
        "implementationOmitted": true,
    });
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&canonical).unwrap_or_default());
    hex::encode(hasher.finalize())
}

fn descriptor(
    id: String,
    kind: &str,
    state: String,
    version: Option<String>,
    source: &str,
) -> CapabilityDescriptorV1 {
    let version = version.filter(|value| safe_version(value));
    let hash = interface_hash(&id, kind, &state, version.as_deref(), source);
    CapabilityDescriptorV1 {
        id,
        kind: kind.into(),
        state,
        version,
        source: source.into(),
        interface_hash: hash,
        implementation_omitted: true,
    }
}

fn safe_plugin_id(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let suffix = &hex::encode(hasher.finalize())[..12];
    let slug: String = slug.chars().take(64).collect();
    if slug.is_empty() {
        format!("unnamed-{suffix}")
    } else {
        format!("{slug}-{suffix}")
    }
}

fn safe_protocol_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'-' | b'+' | b' ' | b'(' | b')')
        })
}

fn display_id(id: &str) -> String {
    let mut value: String = id.chars().take(80).collect();
    if id.chars().count() > 80 {
        value.push('…');
    }
    value
}

pub fn build(
    host: crate::host_features::HostCapabilities,
    runtime: crate::runtime_compat::RuntimeCapabilitiesV1,
    plugins: Vec<crate::commands::PluginDto>,
) -> CapabilityManifestV1 {
    let mut capabilities = Vec::new();
    for (id, capability) in host.capabilities {
        let state = serde_json::to_value(capability.state)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".into());
        capabilities.push(descriptor(
            format!("host.{id}"),
            "host_contract",
            state,
            capability.version,
            "sunsetz_host",
        ));
    }
    let runtime_features = [
        (
            "sandbox",
            runtime.sandbox.state.clone(),
            Some(runtime.sandbox.applied.clone()),
            "runtime_spawn".to_string(),
        ),
        (
            "memory",
            runtime.memory.state.clone(),
            None,
            runtime.memory.source.clone(),
        ),
        (
            "plugin_catalog",
            runtime.plugin_catalog.state.clone(),
            None,
            runtime.plugin_catalog.source.clone(),
        ),
        (
            "hooks_inventory",
            runtime.hooks_inventory.state.clone(),
            None,
            runtime.hooks_inventory.source.clone(),
        ),
        (
            "mcp",
            runtime.mcp.state.clone(),
            None,
            runtime.mcp.source.clone(),
        ),
    ];
    for (id, feature, version, source) in runtime_features {
        capabilities.push(descriptor(
            format!("runtime.{id}"),
            "runtime_contract",
            feature,
            version,
            &source,
        ));
    }
    for plugin in plugins {
        let state = if plugin.enabled {
            "available"
        } else {
            "disabled"
        };
        capabilities.push(descriptor(
            format!("plugin.{}", safe_plugin_id(&plugin.name)),
            "plugin_inventory",
            state.into(),
            plugin.version,
            "runtime_cli_inventory",
        ));
    }
    capabilities.sort_by(|left, right| left.id.cmp(&right.id));
    capabilities.dedup_by(|left, right| left.id == right.id);
    CapabilityManifestV1 {
        schema: "sunsetz.capabilities.v1".into(),
        version: 1,
        producer: "sunsetz-desktop".into(),
        generated_at: chrono::Utc::now(),
        capabilities,
    }
}

pub fn validate(manifest: &CapabilityManifestV1) -> CapabilityManifestValidationV1 {
    let mut errors = Vec::new();
    if manifest.schema != "sunsetz.capabilities.v1" || manifest.version != 1 {
        errors.push("unsupported capability manifest schema".into());
    }
    if manifest.capabilities.len() > 2_000 {
        errors.push("capability manifest exceeds 2000 entries".into());
    }
    if !safe_protocol_token(&manifest.producer, 80) {
        errors.push("invalid capability manifest producer".into());
    }
    let mut seen = std::collections::HashSet::new();
    for capability in manifest.capabilities.iter().take(2_000) {
        let shown_id = display_id(&capability.id);
        if !safe_protocol_token(&capability.id, 160) || !seen.insert(&capability.id) {
            errors.push(format!("invalid or duplicate capability id: {shown_id}"));
            continue;
        }
        if !matches!(
            capability.kind.as_str(),
            "host_contract" | "runtime_contract" | "plugin_inventory"
        ) {
            errors.push(format!("{shown_id} has an invalid capability kind"));
        }
        if !safe_protocol_token(&capability.state, 64) {
            errors.push(format!("{shown_id} has an invalid capability state"));
        }
        if !safe_protocol_token(&capability.source, 80) {
            errors.push(format!("{shown_id} has an invalid metadata source"));
        }
        if capability
            .version
            .as_deref()
            .is_some_and(|version| !safe_version(version))
        {
            errors.push(format!("{shown_id} has an invalid version"));
        }
        if capability.interface_hash.len() != 64
            || !capability
                .interface_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            errors.push(format!("{shown_id} has an invalid interface hash"));
        }
        if !capability.implementation_omitted {
            errors.push(format!("{shown_id} attempts to include an implementation"));
        }
        let expected = interface_hash(
            &capability.id,
            &capability.kind,
            &capability.state,
            capability.version.as_deref(),
            &capability.source,
        );
        if expected != capability.interface_hash {
            errors.push(format!("{shown_id} interface hash mismatch"));
        }
        if errors.len() >= 100 {
            errors.push("additional validation errors omitted".into());
            break;
        }
    }
    CapabilityManifestValidationV1 {
        version: 1,
        valid: errors.is_empty(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_implementation_payload_flag() {
        let mut item = descriptor(
            "host.test".into(),
            "host_contract",
            "available".into(),
            Some("1".into()),
            "test",
        );
        item.implementation_omitted = false;
        let manifest = CapabilityManifestV1 {
            schema: "sunsetz.capabilities.v1".into(),
            version: 1,
            producer: "test".into(),
            generated_at: chrono::Utc::now(),
            capabilities: vec![item],
        };
        assert!(!validate(&manifest).valid);
    }

    #[test]
    fn validation_rejects_paths_and_unbounded_metadata() {
        let item = descriptor(
            "host.test".into(),
            "host_contract",
            "available".into(),
            Some("1".into()),
            "test",
        );
        let mut manifest = CapabilityManifestV1 {
            schema: "sunsetz.capabilities.v1".into(),
            version: 1,
            producer: "test".into(),
            generated_at: chrono::Utc::now(),
            capabilities: vec![item],
        };
        manifest.capabilities[0].source = "/Users/example/plugin".into();
        assert!(!validate(&manifest).valid);
    }
}
