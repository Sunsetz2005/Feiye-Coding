//! Opaque, short-lived handles for local preview resources.
//!
//! The renderer must not be able to turn an arbitrary absolute path into a
//! privileged read. Paths are accepted only when they come from a trusted
//! project, an App-owned attachment, a known Runtime session artifact, a
//! persisted message attachment, or a native OS picker/drop event.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

const HANDLE_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const USER_GRANT_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_HANDLES: usize = 1024;
const MAX_USER_GRANTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOriginV1 {
    TrustedProject,
    AppAttachment,
    SessionArtifact,
    MessageAttachment,
    UserSelected,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceHandleV1 {
    pub version: u8,
    pub id: String,
    pub name: String,
    pub size: u64,
    pub origin: ResourceOriginV1,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct HandleEntry {
    path: PathBuf,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct UserGrant {
    path: PathBuf,
    is_dir: bool,
    expires_at: Instant,
}

static HANDLES: OnceLock<Mutex<HashMap<String, HandleEntry>>> = OnceLock::new();
static USER_GRANTS: OnceLock<Mutex<Vec<UserGrant>>> = OnceLock::new();

fn handles() -> &'static Mutex<HashMap<String, HandleEntry>> {
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn user_grants() -> &'static Mutex<Vec<UserGrant>> {
    USER_GRANTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn canonical_existing(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("resource path not found: {error}"))
}

fn is_within(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn canonical_root_contains(path: &Path, root: &Path) -> bool {
    root.canonicalize()
        .map(|root| is_within(path, &root))
        .unwrap_or(false)
}

fn user_selected_origin(path: &Path) -> Option<ResourceOriginV1> {
    let now = Instant::now();
    let mut grants = user_grants().lock().unwrap_or_else(|e| e.into_inner());
    grants.retain(|grant| grant.expires_at > now);
    grants.iter().find_map(|grant| {
        let allowed = if grant.is_dir {
            is_within(path, &grant.path)
        } else {
            path == grant.path
        };
        allowed.then_some(ResourceOriginV1::UserSelected)
    })
}

fn trusted_project_origin(path: &Path) -> Option<ResourceOriginV1> {
    crate::store::load_projects()
        .into_iter()
        .filter(|project| project.trusted)
        .any(|project| canonical_root_contains(path, Path::new(&project.path)))
        .then_some(ResourceOriginV1::TrustedProject)
}

fn session_artifact_origin(path: &Path) -> Option<ResourceOriginV1> {
    let settings = crate::store::load_settings();
    let projects = crate::store::load_projects();
    crate::store::load_sessions_index()
        .into_iter()
        .filter_map(|session| {
            let agent_id = session.agent_session_id?;
            let cwd = session.project_id.as_ref().and_then(|project_id| {
                projects
                    .iter()
                    .find(|project| &project.id == project_id)
                    .map(|project| project.path.as_str())
            });
            crate::paths::find_agent_session_dir(&agent_id, cwd, &settings.session_data_mode)
        })
        .any(|root| canonical_root_contains(path, &root))
        .then_some(ResourceOriginV1::SessionArtifact)
}

fn message_attachment_origin(path: &Path) -> Option<ResourceOriginV1> {
    crate::store::load_sessions_index()
        .into_iter()
        .flat_map(|session| crate::store::load_messages(&session.id))
        .filter_map(|message| message.attachments)
        .flatten()
        .any(|attachment| {
            canonical_existing(Path::new(&attachment.path))
                .map(|known| known == path)
                .unwrap_or(false)
        })
        .then_some(ResourceOriginV1::MessageAttachment)
}

fn authorized_origin(path: &Path) -> Option<ResourceOriginV1> {
    if canonical_root_contains(path, &crate::paths::attachments_paste_dir()) {
        return Some(ResourceOriginV1::AppAttachment);
    }
    user_selected_origin(path)
        .or_else(|| trusted_project_origin(path))
        .or_else(|| session_artifact_origin(path))
        .or_else(|| message_attachment_origin(path))
}

pub fn authorize_path(path: &Path) -> Result<(PathBuf, ResourceOriginV1), String> {
    let canonical = canonical_existing(path)?;
    if !canonical.is_file() {
        return Err("resource is not a file".into());
    }
    if let Some(origin) = authorized_origin(&canonical) {
        return Ok((canonical, origin));
    }
    Err("RESOURCE_DENIED: path is not a trusted project, attachment, session artifact, or user-selected file".into())
}

/// Validate a Composer attachment against a current Host provenance grant.
/// This never creates or extends a grant.
pub fn authorize_composer_attachment(path: &Path, is_dir: bool) -> Result<PathBuf, String> {
    let canonical = canonical_existing(path)?;
    if (is_dir && !canonical.is_dir()) || (!is_dir && !canonical.is_file()) {
        return Err("RESOURCE_DENIED: composer attachment type changed".into());
    }
    authorized_origin(&canonical)
        .map(|_| canonical)
        .ok_or_else(|| "RESOURCE_DENIED: composer attachment has no trusted provenance".to_string())
}

/// Revalidate a previously Host-authorized, canonical Composer reference.
/// The recovery sidecar is the authority record; this check only confirms the
/// path still resolves to the same object kind and has not been retargeted.
pub fn validate_persisted_composer_attachment(path: &str, is_dir: bool) -> bool {
    let stored = PathBuf::from(path);
    canonical_existing(&stored).is_ok_and(|canonical| {
        canonical == stored && ((is_dir && canonical.is_dir()) || (!is_dir && canonical.is_file()))
    })
}

pub fn require_trusted_project_root(path: &str) -> Result<PathBuf, String> {
    let canonical = canonical_existing(Path::new(path))?;
    if !canonical.is_dir() {
        return Err("project root is not a directory".into());
    }
    let trusted = crate::store::load_projects().into_iter().any(|project| {
        project.trusted
            && canonical_existing(Path::new(&project.path))
                .map(|known| known == canonical)
                .unwrap_or(false)
    });
    if trusted {
        Ok(canonical)
    } else {
        Err("RESOURCE_DENIED: project is not trusted".into())
    }
}

pub fn grant_user_selected<I, P>(paths: I)
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let now = Instant::now();
    let mut grants = user_grants().lock().unwrap_or_else(|e| e.into_inner());
    grants.retain(|grant| grant.expires_at > now);
    for path in paths {
        let Ok(canonical) = canonical_existing(path.as_ref()) else {
            continue;
        };
        let is_dir = canonical.is_dir();
        if !is_dir && !canonical.is_file() {
            continue;
        }
        if let Some(existing) = grants.iter_mut().find(|grant| grant.path == canonical) {
            existing.expires_at = now + USER_GRANT_TTL;
            existing.is_dir = is_dir;
        } else {
            grants.push(UserGrant {
                path: canonical,
                is_dir,
                expires_at: now + USER_GRANT_TTL,
            });
        }
    }
    if grants.len() > MAX_USER_GRANTS {
        let drop_count = grants.len() - MAX_USER_GRANTS;
        grants.drain(0..drop_count);
    }
}

pub fn open_path(path: &str) -> Result<ResourceHandleV1, String> {
    let (path, origin) = authorize_path(Path::new(path.trim()))?;
    let id = Uuid::new_v4().to_string();
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "resource".into());
    let size = path.metadata().map(|meta| meta.len()).unwrap_or(0);
    let now = Instant::now();
    let mut registry = handles().lock().unwrap_or_else(|e| e.into_inner());
    registry.retain(|_, entry| entry.expires_at > now);
    registry.insert(
        id.clone(),
        HandleEntry {
            path,
            expires_at: now + HANDLE_TTL,
        },
    );
    if registry.len() > MAX_HANDLES {
        if let Some(oldest) = registry.keys().next().cloned() {
            registry.remove(&oldest);
        }
    }
    Ok(ResourceHandleV1 {
        version: 1,
        id,
        name,
        size,
        origin,
        expires_at: Utc::now()
            + chrono::Duration::from_std(HANDLE_TTL)
                .unwrap_or_else(|_| chrono::Duration::hours(12)),
    })
}

pub fn resolve_handle(id: &str) -> Result<PathBuf, String> {
    let now = Instant::now();
    let mut registry = handles().lock().unwrap_or_else(|e| e.into_inner());
    registry.retain(|_, entry| entry.expires_at > now);
    let stored = registry
        .get(id.trim())
        .map(|entry| entry.path.clone())
        .ok_or_else(|| {
            "RESOURCE_HANDLE_EXPIRED: resource handle is unknown or expired".to_string()
        })?;
    let current = canonical_existing(&stored)?;
    if current != stored || !current.is_file() {
        registry.remove(id.trim());
        return Err("RESOURCE_DENIED: resource changed after authorization".into());
    }
    Ok(current)
}

pub fn read_handle(id: &str) -> Result<crate::fs_browser::FsReadResult, String> {
    let path = resolve_handle(id)?;
    let mut result = crate::fs_browser::read_absolute_file(&path.to_string_lossy())?;
    result.resource_handle_id = Some(id.trim().to_string());
    Ok(result)
}

pub fn attach_handle(result: &mut crate::fs_browser::FsReadResult) -> Result<(), String> {
    let handle = open_path(&result.absolute_path)?;
    result.resource_handle_id = Some(handle.id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_boundary_does_not_accept_prefix_sibling() {
        assert!(is_within(
            Path::new("/tmp/project/a"),
            Path::new("/tmp/project")
        ));
        assert!(!is_within(
            Path::new("/tmp/project-secret/a"),
            Path::new("/tmp/project")
        ));
    }

    #[test]
    fn native_file_grant_is_exact() {
        let dir = std::env::temp_dir().join(format!("sunsetz-resource-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let selected = dir.join("selected.txt");
        let other = dir.join("other.txt");
        std::fs::write(&selected, "selected").unwrap();
        std::fs::write(&other, "other").unwrap();
        grant_user_selected([&selected]);
        assert_eq!(
            authorize_path(&selected).unwrap().1,
            ResourceOriginV1::UserSelected
        );
        assert!(authorize_path(&other).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn handle_rejects_file_replaced_by_symlink() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!("sunsetz-resource-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let selected = dir.join("selected.txt");
        let outside = dir.join("outside.txt");
        std::fs::write(&selected, "selected").unwrap();
        std::fs::write(&outside, "outside").unwrap();
        grant_user_selected([&selected]);
        let handle = open_path(&selected.to_string_lossy()).unwrap();
        std::fs::remove_file(&selected).unwrap();
        symlink(&outside, &selected).unwrap();
        assert!(resolve_handle(&handle.id).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
