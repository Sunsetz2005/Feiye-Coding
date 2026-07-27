//! Validated, atomic persistence for skills generated from a conversation.

use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const MAX_NAME_BYTES: usize = 80;
const MAX_DESCRIPTION_BYTES: usize = 1_024;
const MAX_SKILL_MD_BYTES: usize = 512 * 1024;
const MAX_REFERENCE_BYTES: usize = 256 * 1024;
const MAX_TOTAL_BYTES: usize = 1024 * 1024;
const MAX_REFERENCES: usize = 24;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillDraftScope {
    Project,
    User,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillDraftReference {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDraftSaveRequest {
    pub name: String,
    pub description: String,
    pub skill_md: String,
    #[serde(default)]
    pub references: Vec<SkillDraftReference>,
    pub scope: SkillDraftScope,
    pub project_path: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillDraftSaveResult {
    pub path: String,
    pub slug: String,
    pub scope: String,
    pub overwritten: bool,
}

fn slugify(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Skill name is required".to_string());
    }
    if trimmed.len() > MAX_NAME_BYTES {
        return Err(format!(
            "Skill name exceeds {MAX_NAME_BYTES} bytes"
        ));
    }
    let mut slug = String::with_capacity(trimmed.len());
    let mut pending_dash = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch.is_ascii_whitespace() {
            pending_dash = true;
        } else {
            return Err(
                "Skill name may only contain ASCII letters, numbers, spaces, '_' and '-'"
                    .to_string(),
            );
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() || slug.len() > 64 {
        return Err("Skill slug must contain 1–64 ASCII characters".to_string());
    }
    if !slug.ends_with("-skill") {
        slug.push_str("-skill");
    }
    Ok(slug)
}

fn parse_frontmatter_value<'a>(frontmatter: &'a str, key: &str) -> Option<&'a str> {
    frontmatter.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        if candidate.trim().eq_ignore_ascii_case(key) {
            let value = value.trim().trim_matches(['"', '\'']);
            if !value.is_empty() {
                return Some(value);
            }
        }
        None
    })
}

fn skill_frontmatter(skill_md: &str) -> Result<&str, String> {
    let normalized = skill_md.strip_prefix('\u{feff}').unwrap_or(skill_md);
    let rest = normalized
        .strip_prefix("---\n")
        .or_else(|| normalized.strip_prefix("---\r\n"))
        .ok_or_else(|| "SKILL.md must start with YAML frontmatter".to_string())?;
    let end = rest
        .find("\n---")
        .ok_or_else(|| "SKILL.md frontmatter is not closed".to_string())?;
    Ok(&rest[..end])
}

fn validate_frontmatter(
    skill_md: &str,
    expected_name: &str,
    expected_description: &str,
) -> Result<(), String> {
    let frontmatter = skill_frontmatter(skill_md)?;
    let name = parse_frontmatter_value(frontmatter, "name")
        .ok_or_else(|| "SKILL.md frontmatter requires a name".to_string())?;
    let description = parse_frontmatter_value(frontmatter, "description")
        .ok_or_else(|| "SKILL.md frontmatter requires a description".to_string())?;
    if name != expected_name.trim() {
        return Err("SKILL.md frontmatter name does not match the draft name".to_string());
    }
    if description != expected_description.trim() {
        return Err(
            "SKILL.md frontmatter description does not match the draft description"
                .to_string(),
        );
    }
    Ok(())
}

fn suspicious_secret(text: &str) -> Option<&'static str> {
    const DIRECT_PATTERNS: &[(&str, &str)] = &[
        ("-----begin private key-----", "private key"),
        ("-----begin rsa private key-----", "private key"),
        ("-----begin openssh private key-----", "private key"),
        ("authorization: bearer ", "bearer token"),
        ("xoxb-", "Slack token"),
        ("xoxp-", "Slack token"),
        ("ghp_", "GitHub token"),
        ("github_pat_", "GitHub token"),
        ("sk-proj-", "API key"),
        ("xai-", "API key"),
    ];
    let lower = text.to_ascii_lowercase();
    for (pattern, label) in DIRECT_PATTERNS {
        if lower.contains(pattern) {
            return Some(label);
        }
    }
    for line in lower.lines() {
        let compact = line.replace(' ', "");
        for key in [
            "api_key=",
            "apikey=",
            "api-token=",
            "access_token=",
            "client_secret=",
            "secret_key=",
            "password=",
        ] {
            if let Some((_, value)) = compact.split_once(key) {
                let value = value.trim_matches(['"', '\'', '`']);
                if value.len() >= 8 && !value.contains('<') && !value.contains("${") {
                    return Some("credential assignment");
                }
            }
        }
    }
    None
}

fn validate_reference_path(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw.trim());
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("Reference paths must be relative".to_string());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            _ => return Err("Reference path traversal is not allowed".to_string()),
        }
    }
    if normalized
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        != Some("references")
    {
        return Err("Reference files must live under references/".to_string());
    }
    if normalized.file_name().is_none() {
        return Err("Reference path must name a file".to_string());
    }
    Ok(normalized)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("Inspect path: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Refusing to write through symbolic link: {}",
            path.display()
        ));
    }
    Ok(())
}

fn reject_tree_symlinks(root: &Path) -> Result<(), String> {
    reject_symlink(root)?;
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|error| format!("Inspect skill: {error}"))? {
        let entry = entry.map_err(|error| format!("Inspect skill: {error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Inspect skill path: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Existing skill contains a symbolic link: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            reject_tree_symlinks(&path)?;
        }
    }
    Ok(())
}

fn validate_request(
    request: &SkillDraftSaveRequest,
) -> Result<(String, Vec<(PathBuf, &[u8])>), String> {
    let name = request.name.trim();
    let description = request.description.trim();
    if description.is_empty() {
        return Err("Skill description is required".to_string());
    }
    if description.len() > MAX_DESCRIPTION_BYTES {
        return Err(format!(
            "Skill description exceeds {MAX_DESCRIPTION_BYTES} bytes"
        ));
    }
    if request.skill_md.is_empty() || request.skill_md.len() > MAX_SKILL_MD_BYTES {
        return Err(format!(
            "SKILL.md must be between 1 and {MAX_SKILL_MD_BYTES} bytes"
        ));
    }
    validate_frontmatter(&request.skill_md, name, description)?;
    if let Some(kind) = suspicious_secret(&request.skill_md) {
        return Err(format!("SKILL.md appears to contain a {kind}"));
    }
    if request.references.len() > MAX_REFERENCES {
        return Err(format!("A skill may contain at most {MAX_REFERENCES} references"));
    }

    let slug = slugify(name)?;
    let mut files = vec![(PathBuf::from("SKILL.md"), request.skill_md.as_bytes())];
    let mut paths = HashSet::new();
    paths.insert(PathBuf::from("SKILL.md"));
    let mut total = request.skill_md.len();
    for reference in &request.references {
        let path = validate_reference_path(&reference.path)?;
        if !paths.insert(path.clone()) {
            return Err(format!("Duplicate reference path: {}", path.display()));
        }
        if reference.content.len() > MAX_REFERENCE_BYTES {
            return Err(format!(
                "Reference {} exceeds {MAX_REFERENCE_BYTES} bytes",
                path.display()
            ));
        }
        if let Some(kind) = suspicious_secret(&reference.content) {
            return Err(format!(
                "Reference {} appears to contain a {kind}",
                path.display()
            ));
        }
        total = total.saturating_add(reference.content.len());
        if total > MAX_TOTAL_BYTES {
            return Err(format!("Skill content exceeds {MAX_TOTAL_BYTES} bytes"));
        }
        files.push((path, reference.content.as_bytes()));
    }
    Ok((slug, files))
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Create skill directory: {error}"))?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("Create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("Write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("Sync {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Set permissions on {}: {error}", path.display()))?;
    }
    Ok(())
}

fn save_to_base(
    request: &SkillDraftSaveRequest,
    base: &Path,
) -> Result<SkillDraftSaveResult, String> {
    let (slug, files) = validate_request(request)?;
    reject_symlink(base)?;
    fs::create_dir_all(base).map_err(|error| format!("Create skills directory: {error}"))?;
    reject_symlink(base)?;

    let target = base.join(&slug);
    if target.exists() {
        reject_tree_symlinks(&target)?;
        if !target.is_dir() {
            return Err(format!("Skill target is not a directory: {}", target.display()));
        }
        if !request.overwrite {
            return Err(format!("SKILL_EXISTS:{}", target.display()));
        }
    }

    let nonce = uuid::Uuid::new_v4();
    let staging = base.join(format!(".{slug}.tmp-{nonce}"));
    let backup = base.join(format!(".{slug}.backup-{nonce}"));
    fs::create_dir(&staging).map_err(|error| format!("Create skill staging area: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Set staging permissions: {error}"))?;
    }

    let write_result = (|| -> Result<(), String> {
        for (relative, bytes) in files {
            write_new_file(&staging.join(relative), bytes)?;
        }
        File::open(&staging)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("Sync skill staging directory: {error}"))?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    let had_existing = target.exists();
    if had_existing {
        fs::rename(&target, &backup)
            .map_err(|error| format!("Prepare skill overwrite: {error}"))?;
    }
    if let Err(error) = fs::rename(&staging, &target) {
        if had_existing {
            let _ = fs::rename(&backup, &target);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("Commit skill atomically: {error}"));
    }
    if had_existing {
        // Backup is app-created and was verified symlink-free before the swap.
        let _ = fs::remove_dir_all(&backup);
    }

    Ok(SkillDraftSaveResult {
        path: target.to_string_lossy().into_owned(),
        slug,
        scope: match request.scope {
            SkillDraftScope::Project => "project",
            SkillDraftScope::User => "user",
        }
        .to_string(),
        overwritten: had_existing,
    })
}

pub fn save(request: SkillDraftSaveRequest) -> Result<SkillDraftSaveResult, String> {
    let base = match request.scope {
        SkillDraftScope::Project => {
            let raw = request
                .project_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "Project scope requires projectPath".to_string())?;
            let project = PathBuf::from(raw)
                .canonicalize()
                .map_err(|error| format!("Resolve project path: {error}"))?;
            if !project.is_dir() {
                return Err("Project path is not a directory".to_string());
            }
            // A project-controlled `.grok` symlink could otherwise redirect
            // an apparently project-scoped save outside the selected project.
            reject_symlink(&project.join(".grok"))?;
            project.join(".grok").join("skills")
        }
        SkillDraftScope::User => {
            let home = BaseDirs::new()
                .ok_or_else(|| "Unable to resolve the user home directory".to_string())?
                .home_dir()
                .to_path_buf();
            reject_symlink(&home.join(".grok"))?;
            home.join(".grok").join("skills")
        }
    };
    save_to_base(&request, &base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sunsetz-skill-draft-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn request() -> SkillDraftSaveRequest {
        SkillDraftSaveRequest {
            name: "Review Helper".to_string(),
            description: "Review a local change safely.".to_string(),
            skill_md: "---\nname: Review Helper\ndescription: Review a local change safely.\n---\n\n# Instructions\n\nInspect the requested change.\n".to_string(),
            references: vec![SkillDraftReference {
                path: "references/checklist.md".to_string(),
                content: "# Checklist\n\n- Verify tests\n".to_string(),
            }],
            scope: SkillDraftScope::User,
            project_path: None,
            overwrite: false,
        }
    }

    #[test]
    fn saves_complete_skill_directory() {
        let base = temp_dir("save");
        let result = save_to_base(&request(), &base).unwrap();
        let root = PathBuf::from(result.path);
        assert_eq!(result.slug, "review-helper-skill");
        assert!(root.join("SKILL.md").is_file());
        assert!(root.join("references/checklist.md").is_file());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn refuses_conflict_without_explicit_overwrite() {
        let base = temp_dir("conflict");
        save_to_base(&request(), &base).unwrap();
        let error = save_to_base(&request(), &base).unwrap_err();
        assert!(error.starts_with("SKILL_EXISTS:"));
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn explicit_overwrite_replaces_prior_content() {
        let base = temp_dir("overwrite");
        save_to_base(&request(), &base).unwrap();
        let mut next = request();
        next.overwrite = true;
        next.skill_md.push_str("\nUpdated.\n");
        let result = save_to_base(&next, &base).unwrap();
        let text = fs::read_to_string(PathBuf::from(result.path).join("SKILL.md")).unwrap();
        assert!(text.contains("Updated."));
        assert!(result.overwritten);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn rejects_reference_path_traversal() {
        let mut value = request();
        value.references[0].path = "references/../../outside.md".to_string();
        assert!(validate_request(&value).unwrap_err().contains("traversal"));
    }

    #[test]
    fn rejects_secret_patterns() {
        let mut value = request();
        value.skill_md.push_str("\napi_key = \"actual-secret-value\"\n");
        assert!(validate_request(&value).unwrap_err().contains("credential"));
    }

    #[test]
    fn rejects_frontmatter_mismatch() {
        let mut value = request();
        value.skill_md = value
            .skill_md
            .replace("name: Review Helper", "name: Other");
        assert!(validate_request(&value).unwrap_err().contains("does not match"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_symlink_tree() {
        use std::os::unix::fs::symlink;
        let base = temp_dir("symlink");
        let target = base.join("review-helper-skill");
        fs::create_dir_all(&target).unwrap();
        symlink("/tmp", target.join("references")).unwrap();
        let mut value = request();
        value.overwrite = true;
        assert!(save_to_base(&value, &base)
            .unwrap_err()
            .contains("symbolic link"));
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_project_grok_parent_symlink() {
        use std::os::unix::fs::symlink;
        let project = temp_dir("project-parent-symlink");
        let outside = temp_dir("project-parent-outside");
        symlink(&outside, project.join(".grok")).unwrap();
        let mut value = request();
        value.scope = SkillDraftScope::Project;
        value.project_path = Some(project.to_string_lossy().into_owned());
        assert!(save(value).unwrap_err().contains("symbolic link"));
        fs::remove_file(project.join(".grok")).unwrap();
        fs::remove_dir_all(project).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
