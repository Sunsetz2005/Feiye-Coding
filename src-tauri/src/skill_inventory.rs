//! Bounded, metadata-only inventory for Skills reported by the live Runtime.
//!
//! Runtime inspect output is discovery evidence, not filesystem authority. This
//! module re-proves every path against the live `GROK_HOME` (and an optional
//! trusted project), rejects symbolic links, and emits no local paths or Skill
//! bodies. Inventory results are suggestions only; callers must explicitly
//! select an item and compare its tree hash immediately before use.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::skill_candidates::{SkillCandidateOwnerV1, SkillCandidateStatusV1, SkillCandidateV1};

pub const SKILL_INVENTORY_VERSION: u8 = 1;

const MAX_RUNTIME_CANDIDATES: usize = 128;
const MAX_SKILL_FILES: usize = 64;
const MAX_SKILL_ENTRIES: usize = 256;
const MAX_SKILL_DEPTH: usize = 32;
const MAX_SKILL_FILE_BYTES: usize = 512 * 1024;
const MAX_SKILL_TOTAL_BYTES: usize = 2 * 1024 * 1024;
const MAX_NAME_BYTES: usize = 160;
const MAX_INSPECT_DESCRIPTION_BYTES: usize = 4 * 1024;
const MAX_DESCRIPTION_BYTES: usize = 2 * 1024;
const MAX_WHEN_TO_USE_BYTES: usize = 4 * 1024;
const MAX_METADATA_TOTAL_BYTES: usize = 128 * 1024;
const MAX_PATH_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSkillCandidateV1 {
    pub name: String,
    pub description: String,
    pub source: String,
    pub path: Option<String>,
    pub user_invocable: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillInventoryBuildRequestV1 {
    pub version: u8,
    pub candidates: Vec<RuntimeSkillCandidateV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillInventorySourceV1 {
    User,
    Project,
    Plugin,
}

impl SkillInventorySourceV1 {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillMetadataInventoryItemV1 {
    pub id: String,
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub source: SkillInventorySourceV1,
    pub tree_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_candidate_id: Option<String>,
    pub user_invocable: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillMetadataInventoryV1 {
    pub version: u8,
    pub items: Vec<SkillMetadataInventoryItemV1>,
}

#[derive(Debug, Clone)]
struct AllowedRoot {
    source: SkillInventorySourceV1,
    configured: PathBuf,
    canonical: PathBuf,
}

#[derive(Debug)]
struct HashedSkillTree {
    tree_hash: String,
    skill_md: Vec<u8>,
}

/// Build a metadata-only inventory from the latest live Runtime inspect result.
///
/// A supplied project root must still be present in Sunsetz's trusted project
/// index. Remote ACP mode fails closed because it has no shared-filesystem
/// capability contract.
pub fn build_inventory_v1(
    request: SkillInventoryBuildRequestV1,
) -> Result<SkillMetadataInventoryV1, String> {
    if request.version != SKILL_INVENTORY_VERSION {
        return Err("unsupported Skill inventory version".into());
    }
    let settings = crate::store::load_settings();
    let live_grok_home = crate::paths::resolve_local_runtime_grok_home(
        &settings.session_data_mode,
        settings.acp_server_addr.as_deref(),
    )?;
    let project_root = request
        .project_root
        .as_deref()
        .map(crate::resource_handles::require_trusted_project_root)
        .transpose()?;
    let approved_candidates = crate::skill_candidates::list()?;
    build_inventory_at(
        request.candidates,
        &live_grok_home,
        project_root.as_deref(),
        &approved_candidates,
        &crate::store::redact_text,
    )
}

/// Compare an explicit UI selection with a freshly built Host inventory.
///
/// Callers should rebuild the inventory immediately before this comparison.
/// This function never invokes a Skill and never changes candidate state.
pub fn verify_selection_v1(
    id: &str,
    expected_tree_hash: &str,
    inventory: &SkillMetadataInventoryV1,
) -> Result<SkillMetadataInventoryItemV1, String> {
    if inventory.version != SKILL_INVENTORY_VERSION {
        return Err("unsupported Skill inventory version".into());
    }
    if !valid_sha256(id) || !valid_sha256(expected_tree_hash) {
        return Err("invalid Skill inventory selection".into());
    }
    if inventory.items.len() > MAX_RUNTIME_CANDIDATES {
        return Err("Skill inventory exceeds its item limit".into());
    }

    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut selected = None;
    for item in &inventory.items {
        if !valid_sha256(&item.id) || !valid_sha256(&item.tree_hash) {
            return Err("invalid Skill inventory item".into());
        }
        if !ids.insert(item.id.as_str()) || !names.insert(item.name.trim().to_lowercase()) {
            return Err("duplicate Skill inventory identity".into());
        }
        if item.id == id {
            selected = Some(item);
        }
    }
    let item = selected.ok_or_else(|| "Skill inventory selection is stale".to_string())?;
    if item.tree_hash != expected_tree_hash {
        return Err("Skill inventory tree hash is stale".into());
    }
    Ok(item.clone())
}

/// Total attached Skill prompt characters for one Host-kernel turn.
pub const HOST_SKILL_PROMPT_BUDGET_CHARS: usize = 16_000;

/// Bounded SKILL.md fragment for the in-process kernel. Never returned from
/// inventory list DTOs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSkillPromptFragmentV1 {
    pub name: String,
    pub tree_hash: String,
    pub fragment: String,
}

/// Metadata inventory from Host-trusted Skill directories.
///
/// This scans the same user / project / plugin trees `build_inventory_v1`
/// already proves. It does not spawn a grok binary and does not treat
/// GROK_HOME as the product kernel — that remains `agent_loop`.
pub fn build_host_trusted_inventory_v1(
    project_root: Option<String>,
) -> Result<SkillMetadataInventoryV1, String> {
    let scanned = scan_host_trusted_skills(project_root.as_deref())?;
    let prefs = crate::extensions::load_prefs();
    let items = scanned
        .into_iter()
        .map(|skill| {
            let mut item = skill.item;
            item.enabled = crate::extensions::is_enabled(&prefs.skills, &item.name);
            item
        })
        .collect();
    Ok(SkillMetadataInventoryV1 {
        version: SKILL_INVENTORY_VERSION,
        items,
    })
}

/// Load one explicitly selected SKILL.md from Host-trusted directories.
pub fn load_skill_md_v1(
    skill_id: &str,
    expected_tree_hash: &str,
    project_root: Option<&str>,
) -> Result<HostSkillPromptFragmentV1, String> {
    let scanned = scan_host_trusted_skills(project_root)?;
    let prefs = crate::extensions::load_prefs();
    load_skill_md_from_scan(skill_id, expected_tree_hash, &scanned, |name| {
        crate::extensions::is_enabled(&prefs.skills, name)
    })
}

/// Load every selected Skill body and join them under the turn budget.
pub fn load_host_skill_fragments_v1(
    selections: &[(String, String)],
    project_root: Option<&str>,
) -> Result<String, String> {
    if selections.len() > crate::skill_feedback::MAX_SKILLS_PER_TURN {
        return Err(format!(
            "SKILL_USE_LIMIT: at most {} Skills per turn",
            crate::skill_feedback::MAX_SKILLS_PER_TURN
        ));
    }
    let mut combined = String::new();
    for (skill_id, tree_hash) in selections {
        let loaded = load_skill_md_v1(skill_id, tree_hash, project_root)?;
        append_host_skill_fragment(&mut combined, &loaded.fragment)?;
    }
    Ok(combined)
}

fn append_host_skill_fragment(combined: &mut String, fragment: &str) -> Result<(), String> {
    if !combined.is_empty() {
        combined.push('\n');
    }
    combined.push_str(fragment);
    if combined.chars().count() > HOST_SKILL_PROMPT_BUDGET_CHARS {
        return Err(format!(
            "SKILL_USE_LIMIT: attached Skill text exceeds {HOST_SKILL_PROMPT_BUDGET_CHARS} characters"
        ));
    }
    Ok(())
}

struct HostTrustedSkill {
    item: SkillMetadataInventoryItemV1,
    skill_md: String,
}

fn scan_host_trusted_skills(project_root: Option<&str>) -> Result<Vec<HostTrustedSkill>, String> {
    let settings = crate::store::load_settings();
    let live_home = crate::paths::resolve_local_runtime_grok_home(
        &settings.session_data_mode,
        settings.acp_server_addr.as_deref(),
    )?;
    let project = project_root
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(crate::resource_handles::require_trusted_project_root)
        .transpose()?;
    let approved = crate::skill_candidates::list()?;
    scan_host_trusted_skills_at(
        &live_home,
        project.as_deref(),
        &approved,
        &crate::store::redact_text,
    )
}

fn scan_host_trusted_skills_at(
    live_home: &Path,
    project_root: Option<&Path>,
    approved_candidates: &[SkillCandidateV1],
    redact: &dyn Fn(&str) -> String,
) -> Result<Vec<HostTrustedSkill>, String> {
    let Ok(canonical_live_home) = canonical_directory(live_home, "Host-trusted Skill home") else {
        return Ok(Vec::new());
    };
    let project_roots = project_root
        .map(|root| {
            canonical_directory(root, "verified project root").map(|canonical| (root, canonical))
        })
        .transpose()?;
    let roots = allowed_roots(
        live_home,
        &canonical_live_home,
        project_roots
            .as_ref()
            .map(|(configured, canonical)| (*configured, canonical.as_path())),
    )?;
    let known_local_roots = roots
        .iter()
        .map(|root| root.canonical.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut metadata_bytes = 0_usize;
    for root in &roots {
        for skill_root in discover_skill_dirs(root)? {
            if items.len() >= MAX_RUNTIME_CANDIDATES {
                return Err("Host-trusted Skill inventory exceeds its item limit".into());
            }
            let hashed = hash_skill_tree(&skill_root)?;
            let skill_md = std::str::from_utf8(&hashed.skill_md)
                .map_err(|_| "SKILL.md is not valid UTF-8".to_string())?;
            reject_sensitive_material(skill_md, redact)?;
            let (frontmatter_name, description, when_to_use) = parse_skill_metadata(skill_md)?;
            validate_returned_metadata(
                &description,
                MAX_DESCRIPTION_BYTES,
                &known_local_roots,
                redact,
            )?;
            validate_returned_metadata(
                &when_to_use,
                MAX_WHEN_TO_USE_BYTES,
                &known_local_roots,
                redact,
            )?;
            let relative = skill_root
                .strip_prefix(&root.canonical)
                .map_err(|_| "Host-trusted Skill escaped its allowed root".to_string())?;
            let relative = relative
                .to_str()
                .ok_or_else(|| "Host-trusted Skill path is not valid UTF-8".to_string())?;
            if relative.is_empty() || relative.chars().any(char::is_control) {
                return Err("invalid Host-trusted Skill identity path".into());
            }
            let id = opaque_skill_id(root.source, &root.canonical, relative);
            let name = host_skill_display_name(root.source, relative, &frontmatter_name);
            let normalized_name = name.trim().to_lowercase();
            if !ids.insert(id.clone()) {
                return Err("duplicate Host-trusted Skill identity".into());
            }
            if !names.insert(normalized_name) {
                return Err("duplicate Host-trusted Skill name".into());
            }
            metadata_bytes = metadata_bytes
                .checked_add(name.len())
                .and_then(|total| total.checked_add(description.len()))
                .and_then(|total| total.checked_add(when_to_use.len()))
                .ok_or_else(|| "Skill inventory metadata is too large".to_string())?;
            if metadata_bytes > MAX_METADATA_TOTAL_BYTES {
                return Err("Skill inventory metadata is too large".into());
            }
            let source_candidate_id =
                source_candidate_id(approved_candidates, &skill_root, &hashed.tree_hash);
            items.push(HostTrustedSkill {
                item: SkillMetadataInventoryItemV1 {
                    id,
                    name,
                    description,
                    when_to_use,
                    source: root.source,
                    tree_hash: hashed.tree_hash,
                    source_candidate_id,
                    user_invocable: true,
                    enabled: true,
                },
                skill_md: skill_md.to_string(),
            });
        }
    }
    items.sort_by(|left, right| {
        left.item
            .name
            .to_lowercase()
            .cmp(&right.item.name.to_lowercase())
            .then_with(|| left.item.id.cmp(&right.item.id))
    });
    Ok(items)
}

fn host_skill_display_name(
    source: SkillInventorySourceV1,
    relative: &str,
    frontmatter_name: &str,
) -> String {
    if source == SkillInventorySourceV1::Plugin {
        if let Some((plugin, rest)) = relative.split_once('/') {
            if rest.starts_with("skills/") || rest == "SKILL.md" {
                let plugin = plugin.trim();
                if !plugin.is_empty() && plugin != frontmatter_name {
                    return format!("{plugin}:{frontmatter_name}");
                }
            }
        }
    }
    frontmatter_name.to_string()
}

fn discover_skill_dirs(root: &AllowedRoot) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    collect_skill_dirs(&root.canonical, &root.canonical, 0, root.source, &mut out)?;
    Ok(out)
}

fn collect_skill_dirs(
    allowed: &Path,
    current: &Path,
    depth: usize,
    source: SkillInventorySourceV1,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if depth > 3 {
        return Ok(());
    }
    if out.len() >= MAX_RUNTIME_CANDIDATES {
        return Err("Host-trusted Skill inventory exceeds its item limit".into());
    }
    reject_symlink_chain(allowed, current)?;
    let metadata = fs::symlink_metadata(current)
        .map_err(|_| "Host-trusted Skill directory is unavailable".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Host-trusted Skill path is not a direct directory".into());
    }
    let skill_md = current.join("SKILL.md");
    if depth > 0 {
        match fs::symlink_metadata(&skill_md) {
            Ok(meta) if !meta.file_type().is_symlink() && meta.is_file() => {
                let canonical = current
                    .canonicalize()
                    .map_err(|_| "Host-trusted Skill root cannot be resolved".to_string())?;
                if canonical == *allowed || !canonical.starts_with(allowed) {
                    return Err("Host-trusted Skill escaped its allowed root".into());
                }
                out.push(canonical);
                return Ok(());
            }
            Ok(_) => return Err("Host-trusted SKILL.md must be a direct file".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Host-trusted SKILL.md is unavailable".into()),
        }
    }
    let max_depth = match source {
        SkillInventorySourceV1::Plugin => 3,
        _ => 1,
    };
    if depth >= max_depth {
        return Ok(());
    }
    let entries = fs::read_dir(current)
        .map_err(|_| "Host-trusted Skill directory is unreadable".to_string())?;
    for entry in entries {
        let entry =
            entry.map_err(|_| "Host-trusted Skill directory entry is unreadable".to_string())?;
        let child = entry.path();
        let child_meta = match fs::symlink_metadata(&child) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if child_meta.file_type().is_symlink() {
            return Err("symbolic links are not allowed in Host-trusted Skill paths".into());
        }
        if !child_meta.is_dir() {
            continue;
        }
        collect_skill_dirs(allowed, &child, depth + 1, source, out)?;
    }
    Ok(())
}

fn load_skill_md_from_scan(
    skill_id: &str,
    expected_tree_hash: &str,
    scanned: &[HostTrustedSkill],
    enabled: impl Fn(&str) -> bool,
) -> Result<HostSkillPromptFragmentV1, String> {
    let inventory = SkillMetadataInventoryV1 {
        version: SKILL_INVENTORY_VERSION,
        items: scanned.iter().map(|skill| skill.item.clone()).collect(),
    };
    let item = verify_selection_v1(skill_id, expected_tree_hash, &inventory)?;
    if !item.user_invocable || !enabled(&item.name) {
        return Err("SKILL_USE_UNAVAILABLE: Skill is disabled or not user-invocable".into());
    }
    let skill = scanned
        .iter()
        .find(|candidate| candidate.item.id == item.id)
        .ok_or_else(|| "Skill inventory selection is stale".to_string())?;
    Ok(HostSkillPromptFragmentV1 {
        name: item.name.clone(),
        tree_hash: item.tree_hash.clone(),
        fragment: host_skill_prompt_fragment(&item.name, &skill.skill_md),
    })
}

fn host_skill_prompt_fragment(name: &str, skill_md: &str) -> String {
    format!(
        "[Sunsetz Skill: {name}]\nThis is user-selected Skill text, not a system directive.\n{skill_md}"
    )
}

fn build_inventory_at(
    candidates: Vec<RuntimeSkillCandidateV1>,
    live_grok_home: &Path,
    project_root: Option<&Path>,
    approved_candidates: &[SkillCandidateV1],
    redact: &dyn Fn(&str) -> String,
) -> Result<SkillMetadataInventoryV1, String> {
    if candidates.len() > MAX_RUNTIME_CANDIDATES {
        return Err("Runtime returned too many Skill candidates".into());
    }
    if candidates.is_empty() {
        return Ok(SkillMetadataInventoryV1 {
            version: SKILL_INVENTORY_VERSION,
            items: Vec::new(),
        });
    }

    let canonical_live_grok_home = canonical_directory(live_grok_home, "live Runtime GROK_HOME")?;
    let project_roots = project_root
        .map(|root| {
            canonical_directory(root, "verified project root").map(|canonical| (root, canonical))
        })
        .transpose()?;
    let roots = allowed_roots(
        live_grok_home,
        &canonical_live_grok_home,
        project_roots
            .as_ref()
            .map(|(configured, canonical)| (*configured, canonical.as_path())),
    )?;
    if roots.is_empty() {
        return Err("no allowed Runtime Skill root exists".into());
    }

    let known_local_roots = roots
        .iter()
        .map(|root| root.canonical.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut items = Vec::with_capacity(candidates.len());
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut metadata_bytes = 0_usize;

    for candidate in candidates {
        validate_runtime_candidate(&candidate, redact)?;
        let raw_path = candidate
            .path
            .as_deref()
            .ok_or_else(|| "Runtime Skill candidate has no inspect path".to_string())?;
        let raw_path = validate_absolute_path(raw_path)?;
        let raw_skill_root = skill_root_from_inspect_path(&raw_path)?;
        let skill_root = canonical_directory(&raw_skill_root, "Runtime Skill root")?;
        let allowed = select_allowed_root(&candidate.source, &raw_skill_root, &skill_root, &roots)?;
        if raw_skill_root.starts_with(&allowed.configured) {
            reject_symlink_chain(&allowed.configured, &raw_skill_root)?;
        } else if raw_skill_root.starts_with(&allowed.canonical) {
            reject_symlink_chain(&allowed.canonical, &raw_skill_root)?;
        } else {
            return Err("Runtime Skill path is not rooted in the configured Runtime".into());
        }
        if skill_root == allowed.canonical || !skill_root.starts_with(&allowed.canonical) {
            return Err("Runtime Skill escaped its allowed root".into());
        }

        let relative = skill_root
            .strip_prefix(&allowed.canonical)
            .map_err(|_| "Runtime Skill escaped its allowed root".to_string())?;
        let relative = relative
            .to_str()
            .ok_or_else(|| "Runtime Skill path is not valid UTF-8".to_string())?;
        if relative.is_empty() || relative.chars().any(char::is_control) {
            return Err("invalid Runtime Skill identity path".into());
        }

        let hashed = hash_skill_tree(&skill_root)?;
        let skill_md = std::str::from_utf8(&hashed.skill_md)
            .map_err(|_| "SKILL.md is not valid UTF-8".to_string())?;
        reject_sensitive_material(skill_md, redact)?;
        let (frontmatter_name, description, when_to_use) = parse_skill_metadata(skill_md)?;
        validate_runtime_name_matches(&candidate.name, &frontmatter_name, allowed.source)?;
        validate_returned_metadata(
            &description,
            MAX_DESCRIPTION_BYTES,
            &known_local_roots,
            redact,
        )?;
        validate_returned_metadata(
            &when_to_use,
            MAX_WHEN_TO_USE_BYTES,
            &known_local_roots,
            redact,
        )?;

        let id = opaque_skill_id(allowed.source, &allowed.canonical, relative);
        let normalized_name = candidate.name.trim().to_lowercase();
        if !ids.insert(id.clone()) {
            return Err("duplicate Runtime Skill identity".into());
        }
        if !names.insert(normalized_name) {
            return Err("duplicate Runtime Skill name".into());
        }

        metadata_bytes = metadata_bytes
            .checked_add(candidate.name.len())
            .and_then(|total| total.checked_add(description.len()))
            .and_then(|total| total.checked_add(when_to_use.len()))
            .ok_or_else(|| "Skill inventory metadata is too large".to_string())?;
        if metadata_bytes > MAX_METADATA_TOTAL_BYTES {
            return Err("Skill inventory metadata is too large".into());
        }

        let source_candidate_id =
            source_candidate_id(approved_candidates, &skill_root, &hashed.tree_hash);
        items.push(SkillMetadataInventoryItemV1 {
            id,
            name: candidate.name.trim().to_string(),
            description,
            when_to_use,
            source: allowed.source,
            tree_hash: hashed.tree_hash,
            source_candidate_id,
            user_invocable: candidate.user_invocable,
            enabled: candidate.enabled,
        });
    }

    items.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(SkillMetadataInventoryV1 {
        version: SKILL_INVENTORY_VERSION,
        items,
    })
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("{label} is unavailable"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("{label} is not a direct directory"));
    }
    path.canonicalize()
        .map_err(|_| format!("{label} cannot be resolved"))
}

fn allowed_roots(
    configured_live_grok_home: &Path,
    canonical_live_grok_home: &Path,
    project_roots: Option<(&Path, &Path)>,
) -> Result<Vec<AllowedRoot>, String> {
    let mut roots = Vec::new();
    add_allowed_root(
        &mut roots,
        SkillInventorySourceV1::User,
        configured_live_grok_home,
        canonical_live_grok_home,
        Path::new("skills"),
    )?;
    if let Some((configured_project_root, canonical_project_root)) = project_roots {
        add_allowed_root(
            &mut roots,
            SkillInventorySourceV1::Project,
            configured_project_root,
            canonical_project_root,
            Path::new(".grok").join("skills").as_path(),
        )?;
    }
    // Current Grok Build uses GROK_HOME/plugins. Keep installed-plugins as a
    // narrow compatibility root for older Runtime inspect output already
    // recognized elsewhere in Sunsetz.
    add_allowed_root(
        &mut roots,
        SkillInventorySourceV1::Plugin,
        configured_live_grok_home,
        canonical_live_grok_home,
        Path::new("plugins"),
    )?;
    add_allowed_root(
        &mut roots,
        SkillInventorySourceV1::Plugin,
        configured_live_grok_home,
        canonical_live_grok_home,
        Path::new("installed-plugins"),
    )?;
    Ok(roots)
}

fn add_allowed_root(
    roots: &mut Vec<AllowedRoot>,
    source: SkillInventorySourceV1,
    configured_base: &Path,
    canonical_base: &Path,
    suffix: &Path,
) -> Result<(), String> {
    let path = configured_base.join(suffix);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("configured Runtime Skill root is not a direct directory".into());
            }
            reject_symlink_chain(configured_base, &path)?;
            let canonical = path
                .canonicalize()
                .map_err(|_| "configured Runtime Skill root cannot be resolved".to_string())?;
            if canonical == canonical_base || !canonical.starts_with(canonical_base) {
                return Err("configured Runtime Skill root escaped its base".into());
            }
            if roots.iter().any(|root| root.canonical == canonical) {
                return Err("duplicate Runtime Skill root".into());
            }
            roots.push(AllowedRoot {
                source,
                configured: path,
                canonical,
            });
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("configured Runtime Skill root is unavailable".into()),
    }
}

fn validate_runtime_candidate(
    candidate: &RuntimeSkillCandidateV1,
    redact: &dyn Fn(&str) -> String,
) -> Result<(), String> {
    let name = candidate.name.trim();
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
    {
        return Err("invalid Runtime Skill name".into());
    }
    if candidate.description.len() > MAX_INSPECT_DESCRIPTION_BYTES
        || candidate
            .description
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err("invalid Runtime Skill description".into());
    }
    reject_sensitive_material(&candidate.description, redact)?;
    let source = candidate.source.trim();
    if source.is_empty()
        || source.len() > 128
        || source.chars().any(char::is_control)
        || source.contains('/')
        || source.contains('\\')
    {
        return Err("invalid Runtime Skill source".into());
    }
    Ok(())
}

fn validate_absolute_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.len() > MAX_PATH_BYTES || raw.chars().any(char::is_control) {
        return Err("invalid Runtime Skill path".into());
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("Runtime Skill path must be absolute and normalized".into());
    }
    Ok(path)
}

fn skill_root_from_inspect_path(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "Runtime Skill inspect path is unavailable".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Runtime Skill inspect path is a symbolic link".into());
    }
    if metadata.is_dir() {
        return Ok(path.to_path_buf());
    }
    if metadata.is_file() && path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
        return path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "invalid Runtime Skill inspect path".to_string());
    }
    Err("Runtime Skill inspect path must be a directory or SKILL.md".into())
}

fn select_allowed_root<'a>(
    reported_source: &str,
    configured_skill_root: &Path,
    canonical_skill_root: &Path,
    roots: &'a [AllowedRoot],
) -> Result<&'a AllowedRoot, String> {
    let matches = roots
        .iter()
        .filter(|root| {
            (configured_skill_root.starts_with(&root.configured)
                || canonical_skill_root.starts_with(&root.canonical))
                && source_matches(reported_source, root.source)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err("Runtime Skill path/source is outside the allowed roots".into());
    }
    Ok(matches[0])
}

fn source_matches(reported: &str, expected: SkillInventorySourceV1) -> bool {
    let reported = reported.trim().to_ascii_lowercase();
    match expected {
        SkillInventorySourceV1::User => reported == "user",
        SkillInventorySourceV1::Project => reported == "project",
        SkillInventorySourceV1::Plugin => reported == "plugin" || reported.starts_with("plugin:"),
    }
}

fn reject_symlink_chain(root: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "Runtime Skill escaped its allowed root".to_string())?;
    let mut current = root.to_path_buf();
    let root_metadata = fs::symlink_metadata(&current)
        .map_err(|_| "Runtime Skill root is unavailable".to_string())?;
    if root_metadata.file_type().is_symlink() {
        return Err("symbolic links are not allowed in Runtime Skill paths".into());
    }
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err("Runtime Skill path is not normalized".into());
        };
        current.push(part);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| "Runtime Skill path is unavailable".to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("symbolic links are not allowed in Runtime Skill paths".into());
        }
    }
    Ok(())
}

fn hash_skill_tree(root: &Path) -> Result<HashedSkillTree, String> {
    let root = canonical_directory(root, "Runtime Skill root")?;
    let skill_md_path = root.join("SKILL.md");
    let skill_md_metadata = fs::symlink_metadata(&skill_md_path)
        .map_err(|_| "Runtime Skill has no SKILL.md".to_string())?;
    if skill_md_metadata.file_type().is_symlink() || !skill_md_metadata.is_file() {
        return Err("Runtime Skill SKILL.md must be a direct file".into());
    }

    fn collect(
        root: &Path,
        current: &Path,
        depth: usize,
        entries: &mut usize,
        total_bytes: &mut usize,
        files: &mut Vec<(String, Vec<u8>)>,
        skill_md: &mut Option<Vec<u8>>,
    ) -> Result<(), String> {
        if depth > MAX_SKILL_DEPTH {
            return Err("Runtime Skill directory nesting is too deep".into());
        }
        *entries = entries
            .checked_add(1)
            .ok_or_else(|| "Runtime Skill contains too many entries".to_string())?;
        if *entries > MAX_SKILL_ENTRIES {
            return Err("Runtime Skill contains too many entries".into());
        }

        let before = fs::symlink_metadata(current)
            .map_err(|_| "Runtime Skill entry is unavailable".to_string())?;
        if before.file_type().is_symlink() {
            return Err("Runtime Skill contains a symbolic link".into());
        }
        if before.is_dir() {
            for entry in fs::read_dir(current)
                .map_err(|_| "Runtime Skill directory is unreadable".to_string())?
            {
                let entry =
                    entry.map_err(|_| "Runtime Skill directory entry is unreadable".to_string())?;
                collect(
                    root,
                    &entry.path(),
                    depth + 1,
                    entries,
                    total_bytes,
                    files,
                    skill_md,
                )?;
            }
            let after = fs::symlink_metadata(current)
                .map_err(|_| "Runtime Skill directory changed while hashing".to_string())?;
            if after.file_type().is_symlink() || !after.is_dir() {
                return Err("Runtime Skill directory changed while hashing".into());
            }
            return Ok(());
        }
        if !before.is_file() {
            return Err("Runtime Skill contains an unsupported filesystem entry".into());
        }
        if files.len() >= MAX_SKILL_FILES {
            return Err("Runtime Skill contains too many files".into());
        }
        if before.len() > MAX_SKILL_FILE_BYTES as u64 {
            return Err("Runtime Skill file exceeds its size limit".into());
        }

        let mut bytes = Vec::with_capacity(before.len() as usize);
        File::open(current)
            .map_err(|_| "Runtime Skill file is unreadable".to_string())?
            .take((MAX_SKILL_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "Runtime Skill file is unreadable".to_string())?;
        if bytes.len() > MAX_SKILL_FILE_BYTES {
            return Err("Runtime Skill file exceeds its size limit".into());
        }
        *total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| "Runtime Skill tree exceeds its size limit".to_string())?;
        if *total_bytes > MAX_SKILL_TOTAL_BYTES {
            return Err("Runtime Skill tree exceeds its size limit".into());
        }

        let after = fs::symlink_metadata(current)
            .map_err(|_| "Runtime Skill file changed while hashing".to_string())?;
        if after.file_type().is_symlink() || !after.is_file() || after.len() != before.len() {
            return Err("Runtime Skill file changed while hashing".into());
        }
        let canonical = current
            .canonicalize()
            .map_err(|_| "Runtime Skill file changed while hashing".to_string())?;
        if !canonical.starts_with(root) {
            return Err("Runtime Skill file escaped its root".into());
        }

        let relative = current
            .strip_prefix(root)
            .map_err(|_| "invalid Runtime Skill tree path".to_string())?
            .to_str()
            .ok_or_else(|| "Runtime Skill tree path is not valid UTF-8".to_string())?
            .replace('\\', "/");
        if relative.chars().any(char::is_control) {
            return Err("invalid Runtime Skill tree path".into());
        }
        if relative == "SKILL.md" {
            *skill_md = Some(bytes.clone());
        }
        files.push((relative, bytes));
        Ok(())
    }

    let mut entries = 0;
    let mut total_bytes = 0;
    let mut files = Vec::new();
    let mut skill_md = None;
    collect(
        &root,
        &root,
        0,
        &mut entries,
        &mut total_bytes,
        &mut files,
        &mut skill_md,
    )?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (relative, bytes) in files {
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    Ok(HashedSkillTree {
        tree_hash: hex::encode(hasher.finalize()),
        skill_md: skill_md.ok_or_else(|| "Runtime Skill has no SKILL.md".to_string())?,
    })
}

fn parse_skill_metadata(skill_md: &str) -> Result<(String, String, String), String> {
    if skill_md.len() > MAX_SKILL_FILE_BYTES {
        return Err("SKILL.md exceeds its size limit".into());
    }
    if skill_md
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err("SKILL.md contains unsupported control characters".into());
    }
    let normalized = skill_md.strip_prefix('\u{feff}').unwrap_or(skill_md);
    let rest = normalized
        .strip_prefix("---\n")
        .or_else(|| normalized.strip_prefix("---\r\n"))
        .ok_or_else(|| "SKILL.md must start with YAML frontmatter".to_string())?;
    let end = rest
        .find("\n---")
        .ok_or_else(|| "SKILL.md frontmatter is not closed".to_string())?;
    if end > 16 * 1024 {
        return Err("SKILL.md frontmatter is too large".into());
    }
    let frontmatter = &rest[..end];
    let name = frontmatter_value(frontmatter, "name")
        .ok_or_else(|| "SKILL.md frontmatter requires a name".to_string())?;
    let description = frontmatter_value(frontmatter, "description")
        .ok_or_else(|| "SKILL.md frontmatter requires a description".to_string())?;
    let description = normalize_metadata_text(&description)?;
    let when_to_use = extract_when_to_use(normalized)?
        .map(|value| normalize_metadata_text(&value))
        .transpose()?
        .unwrap_or_else(|| description.clone());
    Ok((name, description, when_to_use))
}

fn frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    frontmatter.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        if !candidate.trim().eq_ignore_ascii_case(key) {
            return None;
        }
        let value = value.trim();
        if value.is_empty() || matches!(value, "|" | ">" | "|-" | ">-") {
            return None;
        }
        let value = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        (!value.trim().is_empty()).then(|| value.trim().to_string())
    })
}

fn extract_when_to_use(skill_md: &str) -> Result<Option<String>, String> {
    let mut collecting = false;
    let mut collected = String::new();
    for line in skill_md.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if collecting {
                break;
            }
            let heading = heading.trim().trim_end_matches(':').trim();
            if heading.eq_ignore_ascii_case("when to use") {
                collecting = true;
            }
            continue;
        }
        if collecting && trimmed.starts_with('#') {
            break;
        }
        if collecting {
            if collected.len().saturating_add(line.len() + 1) > MAX_WHEN_TO_USE_BYTES * 2 {
                return Err("SKILL.md When to use section is too large".into());
            }
            collected.push_str(line);
            collected.push('\n');
        }
    }
    let collected = collected.trim();
    if collecting && !collected.is_empty() {
        Ok(Some(collected.to_string()))
    } else {
        Ok(None)
    }
}

fn normalize_metadata_text(value: &str) -> Result<String, String> {
    if value
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err("Skill metadata contains unsupported control characters".into());
    }
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("Skill metadata is empty".into());
    }
    Ok(normalized)
}

fn validate_runtime_name_matches(
    runtime_name: &str,
    frontmatter_name: &str,
    source: SkillInventorySourceV1,
) -> Result<(), String> {
    let runtime_name = runtime_name.trim();
    let matches = runtime_name == frontmatter_name
        || (source == SkillInventorySourceV1::Plugin
            && runtime_name
                .rsplit_once(':')
                .is_some_and(|(_, name)| name == frontmatter_name));
    if !matches {
        return Err("Runtime Skill name does not match SKILL.md metadata".into());
    }
    Ok(())
}

fn validate_returned_metadata(
    value: &str,
    max_bytes: usize,
    known_local_roots: &[String],
    redact: &dyn Fn(&str) -> String,
) -> Result<(), String> {
    if value.is_empty() || value.len() > max_bytes {
        return Err("Skill metadata exceeds its size limit".into());
    }
    reject_sensitive_material(value, redact)?;
    if known_local_roots
        .iter()
        .filter(|root| root.len() >= 2)
        .any(|root| value.contains(root))
        || contains_obvious_local_absolute_path(value)
    {
        return Err("Skill metadata contains a local absolute path".into());
    }
    Ok(())
}

fn reject_sensitive_material(value: &str, redact: &dyn Fn(&str) -> String) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    let strong_marker = lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("aws_secret_access_key")
        || lower.contains("authorization: bearer ")
        || value
            .split(|ch: char| {
                ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '=' | ':' | ',' | ';')
            })
            .any(|token| {
                token.len() > 20
                    && (token.starts_with("sk-")
                        || token.starts_with("xai-")
                        || token.starts_with("AKIA"))
            });
    let redacted = redact(value);
    if strong_marker || redacted.contains("[REDACTED]") {
        return Err("Skill metadata contains sensitive material".into());
    }
    Ok(())
}

fn contains_obvious_local_absolute_path(value: &str) -> bool {
    value.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | ',' | ';'
            )
        });
        token.starts_with("/Users/")
            || token.starts_with("/home/")
            || token.starts_with("/private/")
            || token.starts_with("/tmp/")
            || token.starts_with("\\\\")
            || (token.len() >= 3
                && token.as_bytes()[0].is_ascii_alphabetic()
                && token.as_bytes()[1] == b':'
                && matches!(token.as_bytes()[2], b'\\' | b'/'))
    })
}

fn opaque_skill_id(source: SkillInventorySourceV1, allowed_root: &Path, relative: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sunsetz-skill-inventory-id-v1\0");
    hasher.update(source.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(allowed_root.as_os_str().to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(relative.replace('\\', "/").as_bytes());
    hex::encode(hasher.finalize())
}

fn source_candidate_id(
    candidates: &[SkillCandidateV1],
    skill_root: &Path,
    tree_hash: &str,
) -> Option<String> {
    let mut matched = None;
    for candidate in candidates {
        if candidate.status != SkillCandidateStatusV1::Approved
            || !host_candidate_owner(&candidate.owner)
            || candidate.approved_content_hash.as_deref() != Some(tree_hash)
            || !valid_candidate_id(&candidate.id)
        {
            continue;
        }
        let Some(path) = candidate.approved_path.as_deref() else {
            continue;
        };
        let Ok(path) = Path::new(path).canonicalize() else {
            continue;
        };
        if path != skill_root {
            continue;
        }
        if matched.is_some() {
            // More than one exact proof is ambiguous; do not claim lineage.
            return None;
        }
        matched = Some(candidate.id.clone());
    }
    matched
}

fn host_candidate_owner(owner: &SkillCandidateOwnerV1) -> bool {
    matches!(owner.kind.as_str(), "host_candidate" | "host_generated")
        && owner.namespace == "sunsetz"
        && !owner.may_overwrite_external
}

fn valid_candidate_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::fs;
    use std::io::Write;

    use crate::skill_candidates::{SkillCandidateDraftV1, SkillCandidateSourceV1};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "sunsetz-skill-inventory-test-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn no_redaction(value: &str) -> String {
        value.to_string()
    }

    fn write_skill(root: &Path, name: &str, when: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Safe metadata for {name}.\n---\n\n# {name}\n\n## When to use\n\n{when}\n"
            ),
        )
        .unwrap();
    }

    fn runtime_candidate(name: &str, source: &str, path: &Path) -> RuntimeSkillCandidateV1 {
        RuntimeSkillCandidateV1 {
            name: name.to_string(),
            description: format!("Runtime description for {name}"),
            source: source.to_string(),
            path: Some(path.to_string_lossy().into_owned()),
            user_invocable: true,
            enabled: true,
        }
    }

    fn build_at(
        candidates: Vec<RuntimeSkillCandidateV1>,
        home: &Path,
        project: Option<&Path>,
        approved: &[SkillCandidateV1],
    ) -> Result<SkillMetadataInventoryV1, String> {
        build_inventory_at(candidates, home, project, approved, &no_redaction)
    }

    fn approved_candidate(id: &str, path: &Path, hash: &str, owner_kind: &str) -> SkillCandidateV1 {
        let now = Utc::now();
        SkillCandidateV1 {
            version: 1,
            id: id.to_string(),
            status: SkillCandidateStatusV1::Approved,
            created_at: now,
            updated_at: now,
            content_hash: "a".repeat(64),
            review_content_hash: Some("b".repeat(64)),
            source: SkillCandidateSourceV1 {
                session_id: "session".into(),
                session_title: "title".into(),
                message_ids: vec!["message".into()],
            },
            owner: SkillCandidateOwnerV1 {
                kind: owner_kind.into(),
                namespace: "sunsetz".into(),
                may_overwrite_external: false,
            },
            draft: SkillCandidateDraftV1 {
                name: "owned".into(),
                description: "owned".into(),
                skill_md: "owned".into(),
                references: Vec::new(),
            },
            approved_path: Some(path.to_string_lossy().into_owned()),
            approved_content_hash: Some(hash.to_string()),
            audit_events: Vec::new(),
        }
    }

    #[test]
    fn stable_tree_hash_and_selection_cas_detect_modification() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let first = home.join("skills").join("alpha");
        write_skill(&first, "alpha", "Use for alpha workflows.");

        let before = build_at(
            vec![runtime_candidate("alpha", "user", &first)],
            &home,
            None,
            &[],
        )
        .unwrap();
        let repeated = build_at(
            vec![runtime_candidate("alpha", "user", &first)],
            &home,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(before, repeated);

        fs::write(first.join("reference.txt"), "changed").unwrap();
        let after = build_at(
            vec![runtime_candidate("alpha", "user", &first)],
            &home,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(before.items[0].id, after.items[0].id);
        assert_ne!(before.items[0].tree_hash, after.items[0].tree_hash);
        assert!(
            verify_selection_v1(&before.items[0].id, &before.items[0].tree_hash, &after,).is_err()
        );
    }

    #[test]
    fn hash_is_deterministic_across_directory_enumeration_order() {
        let temp = TempDir::new();
        let left = temp.0.join("left");
        let right = temp.0.join("right");
        write_skill(&left, "same", "Use safely.");
        write_skill(&right, "same", "Use safely.");
        fs::write(left.join("z.txt"), "z").unwrap();
        fs::write(left.join("a.txt"), "a").unwrap();
        fs::write(right.join("a.txt"), "a").unwrap();
        fs::write(right.join("z.txt"), "z").unwrap();
        assert_eq!(
            hash_skill_tree(&left).unwrap().tree_hash,
            hash_skill_tree(&right).unwrap().tree_hash
        );
    }

    #[test]
    fn traversal_and_paths_outside_allowed_roots_are_rejected() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        fs::create_dir_all(home.join("skills")).unwrap();
        let outside = home.join("outside");
        write_skill(&outside, "outside", "Use outside.");
        let escaped = home
            .join("skills")
            .join("..")
            .join("outside")
            .join("SKILL.md");
        assert!(build_at(
            vec![runtime_candidate("outside", "user", &escaped)],
            &home,
            None,
            &[],
        )
        .is_err());
        assert!(build_at(
            vec![runtime_candidate("outside", "user", &outside)],
            &home,
            None,
            &[],
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_in_path_or_tree_are_rejected() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let home = temp.0.join("home");
        let skills = home.join("skills");
        let real = skills.join("real");
        write_skill(&real, "real", "Use real.");
        let alias = skills.join("alias");
        symlink(&real, &alias).unwrap();
        assert!(build_at(
            vec![runtime_candidate("real", "user", &alias)],
            &home,
            None,
            &[],
        )
        .is_err());

        let direct = skills.join("direct");
        write_skill(&direct, "direct", "Use direct.");
        symlink(real.join("SKILL.md"), direct.join("linked.md")).unwrap();
        assert!(build_at(
            vec![runtime_candidate("direct", "user", &direct)],
            &home,
            None,
            &[],
        )
        .is_err());

        let project = temp.0.join("project");
        let external_grok = temp.0.join("external-grok");
        let escaped_project_skill = external_grok.join("skills").join("escaped");
        write_skill(&escaped_project_skill, "escaped", "Use escaped.");
        fs::create_dir_all(&project).unwrap();
        symlink(&external_grok, project.join(".grok")).unwrap();
        assert!(build_at(
            vec![runtime_candidate(
                "escaped",
                "project",
                &project.join(".grok").join("skills").join("escaped"),
            )],
            &home,
            Some(&project),
            &[],
        )
        .is_err());
    }

    #[test]
    fn per_file_and_file_count_limits_are_enforced() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let large = home.join("skills").join("large");
        write_skill(&large, "large", "Use large.");
        fs::write(
            large.join("large.bin"),
            vec![b'x'; MAX_SKILL_FILE_BYTES + 1],
        )
        .unwrap();
        assert!(build_at(
            vec![runtime_candidate("large", "user", &large)],
            &home,
            None,
            &[],
        )
        .is_err());

        let many = home.join("skills").join("many");
        write_skill(&many, "many", "Use many.");
        for index in 0..MAX_SKILL_FILES {
            fs::write(many.join(format!("{index}.txt")), "x").unwrap();
        }
        assert!(build_at(
            vec![runtime_candidate("many", "user", &many)],
            &home,
            None,
            &[],
        )
        .is_err());
    }

    #[test]
    fn user_project_and_plugin_roots_are_proved_and_paths_are_not_returned() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let project = temp.0.join("project");
        let user = home.join("skills").join("user-skill");
        let project_skill = project.join(".grok").join("skills").join("project-skill");
        let plugin = home
            .join("plugins")
            .join("example")
            .join("skills")
            .join("plugin-skill");
        write_skill(&user, "user-skill", "Use for user work.");
        write_skill(&project_skill, "project-skill", "Use for project work.");
        write_skill(&plugin, "plugin-skill", "Use for plugin work.");

        let inventory = build_at(
            vec![
                runtime_candidate("user-skill", "user", &user.join("SKILL.md")),
                runtime_candidate("project-skill", "project", &project_skill),
                runtime_candidate("example:plugin-skill", "plugin:example", &plugin),
            ],
            &home,
            Some(&project),
            &[],
        )
        .unwrap();
        assert_eq!(inventory.items.len(), 3);
        let serialized = serde_json::to_string(&inventory).unwrap();
        assert!(!serialized.contains(temp.0.to_string_lossy().as_ref()));
        assert!(inventory
            .items
            .iter()
            .any(|item| item.source == SkillInventorySourceV1::User));
        assert!(inventory
            .items
            .iter()
            .any(|item| item.source == SkillInventorySourceV1::Project));
        assert!(inventory
            .items
            .iter()
            .any(|item| item.source == SkillInventorySourceV1::Plugin));

        let unverified_plugin = home
            .join("marketplace-cache")
            .join("plugin")
            .join("skills")
            .join("bad");
        write_skill(&unverified_plugin, "bad", "Use bad.");
        assert!(build_at(
            vec![runtime_candidate("bad", "plugin", &unverified_plugin)],
            &home,
            Some(&project),
            &[],
        )
        .is_err());
    }

    #[test]
    fn lineage_requires_approved_host_owner_exact_path_and_tree_hash() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let skill = home.join("skills").join("owned");
        write_skill(&skill, "owned", "Use owned.");
        let hash = hash_skill_tree(&skill).unwrap().tree_hash;
        let id = "0123456789abcdef0123456789abcdef";
        let approved = approved_candidate(id, &skill, &hash, "host_candidate");
        let inventory = build_at(
            vec![runtime_candidate("owned", "user", &skill)],
            &home,
            None,
            &[approved],
        )
        .unwrap();
        assert_eq!(inventory.items[0].source_candidate_id.as_deref(), Some(id));

        let legacy = approved_candidate(id, &skill, &hash, "host_generated");
        let inventory = build_at(
            vec![runtime_candidate("owned", "user", &skill)],
            &home,
            None,
            &[legacy],
        )
        .unwrap();
        assert_eq!(inventory.items[0].source_candidate_id.as_deref(), Some(id));

        let external = approved_candidate(id, &skill, &hash, "user");
        let inventory = build_at(
            vec![runtime_candidate("owned", "user", &skill)],
            &home,
            None,
            &[external],
        )
        .unwrap();
        assert!(inventory.items[0].source_candidate_id.is_none());

        let stale = approved_candidate(id, &skill, &"f".repeat(64), "host_generated");
        let inventory = build_at(
            vec![runtime_candidate("owned", "user", &skill)],
            &home,
            None,
            &[stale],
        )
        .unwrap();
        assert!(inventory.items[0].source_candidate_id.is_none());
    }

    #[test]
    fn duplicate_identity_or_name_is_rejected() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let first = home.join("skills").join("first");
        let second = home.join("skills").join("second");
        write_skill(&first, "same", "Use first.");
        write_skill(&second, "same", "Use second.");

        let same_path = runtime_candidate("same", "user", &first);
        assert!(build_at(vec![same_path.clone(), same_path], &home, None, &[],).is_err());
        assert!(build_at(
            vec![
                runtime_candidate("same", "user", &first),
                runtime_candidate("same", "user", &second),
            ],
            &home,
            None,
            &[],
        )
        .is_err());
    }

    #[test]
    fn sensitive_skill_body_and_absolute_metadata_paths_fail_closed() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let secret = home.join("skills").join("secret");
        write_skill(&secret, "secret", "Use safely.");
        fs::OpenOptions::new()
            .append(true)
            .open(secret.join("SKILL.md"))
            .unwrap()
            .write_all(b"\nToken: sk-abcdefghijklmnopqrstuvwxyz123456\n")
            .unwrap();
        assert!(build_at(
            vec![runtime_candidate("secret", "user", &secret)],
            &home,
            None,
            &[],
        )
        .is_err());

        let pathy = home.join("skills").join("pathy");
        write_skill(&pathy, "pathy", "Use /Users/alice/private/file.txt.");
        assert!(build_at(
            vec![runtime_candidate("pathy", "user", &pathy)],
            &home,
            None,
            &[],
        )
        .is_err());
    }

    #[test]
    fn strict_wire_schema_rejects_unknown_fields() {
        let request = serde_json::json!({
            "version": 1,
            "candidates": [{
                "name": "alpha",
                "description": "desc",
                "source": "user",
                "path": null,
                "userInvocable": true,
                "enabled": true,
                "unexpected": true
            }]
        });
        assert!(serde_json::from_value::<SkillInventoryBuildRequestV1>(request).is_err());
    }

    #[test]
    fn host_scan_loads_trusted_dirs_without_inspect_candidates() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let project = temp.0.join("project");
        let user = home.join("skills").join("review");
        let project_skill = project.join(".grok").join("skills").join("project-review");
        let plugin = home
            .join("plugins")
            .join("pack")
            .join("skills")
            .join("pack-review");
        write_skill(&user, "review", "Use for reviews.");
        write_skill(&project_skill, "project-review", "Use in the project.");
        write_skill(&plugin, "pack-review", "Use plugin help.");
        let scanned =
            scan_host_trusted_skills_at(&home, Some(&project), &[], &no_redaction).unwrap();
        assert_eq!(scanned.len(), 3);
        let inventory = SkillMetadataInventoryV1 {
            version: SKILL_INVENTORY_VERSION,
            items: scanned.iter().map(|skill| skill.item.clone()).collect(),
        };
        let serialized = serde_json::to_string(&inventory).unwrap();
        assert!(!serialized.contains("skillMd"));
        assert!(!serialized.contains("SKILL.md"));
        assert!(!serialized.contains(&temp.0.to_string_lossy().to_string()));

        let review = scanned
            .iter()
            .find(|skill| skill.item.name == "review")
            .unwrap();
        let loaded =
            load_skill_md_from_scan(&review.item.id, &review.item.tree_hash, &scanned, |_| true)
                .unwrap();
        assert!(loaded.fragment.contains("[Sunsetz Skill: review]"));
        assert!(loaded.fragment.contains("Use for reviews."));
        assert!(loaded.fragment.contains("not a system directive"));
        assert!(
            load_skill_md_from_scan(&review.item.id, &"a".repeat(64), &scanned, |_| true).is_err()
        );
        assert!(
            load_skill_md_from_scan(&review.item.id, &review.item.tree_hash, &scanned, |_| false)
                .is_err()
        );

        let without_project = scan_host_trusted_skills_at(&home, None, &[], &no_redaction).unwrap();
        assert!(!without_project
            .iter()
            .any(|skill| skill.item.name == "project-review"));
    }

    #[cfg(unix)]
    #[test]
    fn host_scan_refuses_symlinked_skill_dir() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let skills = home.join("skills");
        fs::create_dir_all(&skills).unwrap();
        let outside = home.join("outside");
        write_skill(&outside, "outside", "Use outside.");
        std::os::unix::fs::symlink(&outside, skills.join("alias")).unwrap();
        assert!(scan_host_trusted_skills_at(&home, None, &[], &no_redaction).is_err());
    }

    #[test]
    fn host_scan_ignores_marketplace_cache() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let cache = home
            .join("marketplace-cache")
            .join("plugin")
            .join("skills")
            .join("cached");
        write_skill(&cache, "cached", "Use cached.");
        write_skill(&home.join("skills").join("ok"), "ok", "Use ok.");
        let scanned = scan_host_trusted_skills_at(&home, None, &[], &no_redaction).unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].item.name, "ok");
    }

    #[test]
    fn host_skill_prompt_budget_fails_closed() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let first = home.join("skills").join("one");
        let second = home.join("skills").join("two");
        let bulky = "word ".repeat(2_000);
        write_skill(&first, "one", "Use one.");
        write_skill(&second, "two", "Use two.");
        fs::OpenOptions::new()
            .append(true)
            .open(first.join("SKILL.md"))
            .unwrap()
            .write_all(format!("\n## Body\n\n{bulky}\n").as_bytes())
            .unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(second.join("SKILL.md"))
            .unwrap()
            .write_all(format!("\n## Body\n\n{bulky}\n").as_bytes())
            .unwrap();
        let scanned = scan_host_trusted_skills_at(&home, None, &[], &no_redaction).unwrap();
        assert_eq!(scanned.len(), 2);
        let mut combined = String::new();
        let mut overflowed = false;
        for skill in &scanned {
            let loaded =
                load_skill_md_from_scan(&skill.item.id, &skill.item.tree_hash, &scanned, |_| true)
                    .unwrap();
            if append_host_skill_fragment(&mut combined, &loaded.fragment).is_err() {
                overflowed = true;
            }
        }
        assert!(overflowed);
    }
}
