//! Sunsetz-owned application data roots.

use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

pub fn app_data_root() -> PathBuf {
    let home = crate::process_util::user_home();
    let temp = std::env::temp_dir();
    if let Some(custom) = crate::runtime_compat::product_home_override() {
        if user_data_root_is_isolated(&custom, &home, &temp) {
            return custom;
        }
        tracing::warn!(
            target: "sunsetz::paths",
            path = %custom.display(),
            "SUNSETZ_HOME is not isolated to the current user (shared folder, other home, or git checkout); ignoring override"
        );
    }
    if let Some(proj) = ProjectDirs::from("dev", "sunsetz", "desktop") {
        let dir = proj.data_dir().to_path_buf();
        if user_data_root_is_isolated(&dir, &home, &temp) {
            return dir;
        }
    }
    dirs_fallback()
}

/// True when `path` may hold this user's MCP, model, and connector data.
///
/// Allowed: current user home, or the process temp dir (tests / native smoke).
/// Denied: another user's home, `/Users/Shared`, world-shared public folders,
/// and any git checkout (so local models/MCP are never staged for GitHub).
pub fn user_data_root_is_isolated(path: &Path, user_home: &Path, temp_dir: &Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    if is_inside_git_worktree(path) {
        return false;
    }
    path_is_within(path, user_home) || path_is_within(path, temp_dir)
}

fn path_is_within(path: &Path, parent: &Path) -> bool {
    let path = normalize_for_compare(path);
    let parent = normalize_for_compare(parent);
    path == parent || path.starts_with(&parent)
}

fn normalize_for_compare(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        }
    })
}

fn is_inside_git_worktree(path: &Path) -> bool {
    let start = normalize_for_compare(path);
    for ancestor in start.ancestors() {
        if ancestor.join(".git").exists() {
            return true;
        }
    }
    false
}

fn dirs_fallback() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("Sunsetz");
        }
    }
    crate::process_util::user_home().join(".sunsetz")
}

pub fn ensure_app_dirs() -> std::io::Result<PathBuf> {
    let root = app_data_root();
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&root, fs::Permissions::from_mode(0o700));
    }
    std::fs::create_dir_all(root.join("projects"))?;
    std::fs::create_dir_all(root.join("sessions"))?;
    std::fs::create_dir_all(root.join("logs"))?;
    // Agent profile (config.toml / optional auth) when session_data_mode=independent.
    std::fs::create_dir_all(root.join("agent-home"))?;
    // Clipboard paste / picker-written attachment files.
    std::fs::create_dir_all(root.join("attachments").join("paste"))?;
    // Multi-account auth snapshots.
    std::fs::create_dir_all(root.join("accounts"))?;
    Ok(root)
}

/// Directory for pasted / saved composer attachments (absolute paths for `@path` refs).
pub fn attachments_paste_dir() -> PathBuf {
    let dir = app_data_root().join("attachments").join("paste");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// GROK_HOME for independent mode: App-owned agent profile (providers, config).
pub fn agent_home_dir() -> PathBuf {
    app_data_root().join("agent-home")
}

pub fn agent_config_toml() -> PathBuf {
    agent_home_dir().join("config.toml")
}

/// Resolve GROK_HOME for a spawned agent process.
pub fn resolve_agent_grok_home(session_data_mode: &str) -> PathBuf {
    if session_data_mode == "shared" {
        return crate::process_util::user_home().join(".grok");
    }
    let _ = ensure_app_dirs();
    agent_home_dir()
}

/// Resolve the filesystem root used by the live local Runtime.
///
/// ACP server mode has no negotiated shared-filesystem contract. Treating a
/// local path as remotely visible would report successful Skill writes that
/// the connected Runtime may never discover, so filesystem-backed operations
/// must fail closed until such a capability exists.
pub fn resolve_local_runtime_grok_home(
    session_data_mode: &str,
    acp_server_addr: Option<&str>,
) -> Result<PathBuf, String> {
    if acp_server_addr.is_some_and(|value| !value.trim().is_empty()) {
        return Err(
            "RUNTIME_FILESYSTEM_UNVERIFIED: ACP server mode does not declare a shared Runtime filesystem"
                .into(),
        );
    }
    Ok(resolve_agent_grok_home(session_data_mode))
}

pub fn projects_file() -> PathBuf {
    app_data_root().join("projects.json")
}

pub fn sessions_index_file() -> PathBuf {
    app_data_root().join("sessions_index.json")
}

pub fn settings_file() -> PathBuf {
    app_data_root().join("settings.json")
}

/// On-disk secrets metadata (+ API-key fallback when OS keychain is unavailable).
/// Sensitive keys prefer the OS keychain; see [`crate::secrets`].
pub fn secrets_file() -> PathBuf {
    app_data_root().join("secrets.json")
}

pub fn session_dir(session_id: &str) -> PathBuf {
    app_data_root().join("sessions").join(session_id)
}

/// Host-side scheduled automations (shell list; execution via agent sessions).
pub fn automations_file() -> PathBuf {
    app_data_root().join("automations.json")
}

pub fn automation_runs_file() -> PathBuf {
    app_data_root().join("automation-runs.v1.json")
}

/// App MCP/Skills enable prefs (`extensions.json`).
pub fn extensions_file() -> PathBuf {
    app_data_root().join("extensions.json")
}

/// Percent-encode a path the way Grok Build names session folders under
/// `GROK_HOME/sessions/` (encodeURIComponent of the absolute cwd).
pub fn percent_encode_path_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0xf) as usize]));
            }
        }
    }
    out
}

/// Locate the on-disk agent session directory for a given agent session id.
/// Layout: `{GROK_HOME}/sessions/{percent-encoded-cwd}/{agent_session_id}/`
///
/// `cwd_hint` (project path) avoids a directory scan when known.
pub fn find_agent_session_dir(
    agent_session_id: &str,
    cwd_hint: Option<&str>,
    session_data_mode: &str,
) -> Option<PathBuf> {
    if agent_session_id.is_empty() {
        return None;
    }
    let home = resolve_agent_grok_home(session_data_mode);
    let sessions = home.join("sessions");
    if !sessions.is_dir() {
        return None;
    }

    if let Some(cwd) = cwd_hint.filter(|s| !s.is_empty()) {
        let encoded = percent_encode_path_component(cwd);
        let candidate = sessions.join(encoded).join(agent_session_id);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    // Fallback: scan cwd folders for this agent session id
    let Ok(entries) = fs::read_dir(&sessions) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let candidate = path.join(agent_session_id);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Join agent session root + relative path like `images/1.jpg`.
/// Rejects `..` segments. Returns None if the resolved file is missing.
pub fn resolve_session_relative_media(session_root: &Path, relative: &str) -> Option<PathBuf> {
    let rel = relative.trim().trim_start_matches("./");
    if rel.is_empty() {
        return None;
    }
    if Path::new(rel).is_absolute() {
        return None;
    }
    let mut clean = PathBuf::new();
    for comp in Path::new(rel).components() {
        use std::path::Component;
        match comp {
            Component::Normal(s) => clean.push(s),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return None;
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return None;
    }
    let full = session_root.join(clean);
    if full.is_file() {
        Some(full)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_root_is_absolute_or_relative_path() {
        let p = app_data_root();
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn user_data_root_rejects_git_checkout_and_shared_folder() {
        let home = PathBuf::from("/Users/alice");
        let temp = PathBuf::from("/tmp/sunsetz-test-temp");
        assert!(user_data_root_is_isolated(
            &home.join("Library/Application Support/dev.sunsetz.desktop"),
            &home,
            &temp,
        ));
        assert!(user_data_root_is_isolated(
            &temp.join("smoke"),
            &home,
            &temp
        ));
        assert!(!user_data_root_is_isolated(
            Path::new("/Users/Shared/Files From d.localized/Coding/Sunsetz"),
            &home,
            &temp,
        ));
        assert!(!user_data_root_is_isolated(
            Path::new("/Users/bob/.sunsetz"),
            &home,
            &temp,
        ));
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        assert!(
            !user_data_root_is_isolated(&repo.join(".sunsetz-local"), &home, &temp),
            "data inside this git checkout must not be treated as isolated"
        );
    }

    #[test]
    fn percent_encode_matches_encode_uri_component_style() {
        let cwd = "/Users/me/Downloads/5 天 course";
        let enc = percent_encode_path_component(cwd);
        assert!(enc.starts_with("%2FUsers%2Fme%2FDownloads%2F5%20"));
        assert!(enc.contains("%E5%A4%A9") || enc.contains("%"));
        assert!(!enc.contains('/'));
    }

    #[test]
    fn resolve_session_relative_rejects_parent() {
        let root = PathBuf::from("/tmp/session");
        assert!(resolve_session_relative_media(&root, "../etc/passwd").is_none());
        assert!(resolve_session_relative_media(&root, "/etc/passwd").is_none());
    }

    #[test]
    fn local_runtime_home_matches_independent_and_shared_spawn_roots() {
        assert_eq!(
            resolve_local_runtime_grok_home("independent", None).unwrap(),
            agent_home_dir()
        );
        assert_eq!(
            resolve_local_runtime_grok_home("shared", None).unwrap(),
            crate::process_util::user_home().join(".grok")
        );
    }

    #[test]
    fn acp_server_runtime_filesystem_fails_closed() {
        for mode in ["independent", "shared"] {
            let error = resolve_local_runtime_grok_home(mode, Some("127.0.0.1:8799")).unwrap_err();
            assert!(error.starts_with("RUNTIME_FILESYSTEM_UNVERIFIED:"));
        }
        assert!(resolve_local_runtime_grok_home("independent", Some("   ")).is_ok());
    }
}
