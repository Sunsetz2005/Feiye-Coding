//! Permission scope_key rules (§17.3) + session allow cache.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    /// Grok Build `default` — ask every tool that needs approval (unless session cache hits).
    Ask,
    AllowOnce,
    AllowForSession,
    /// Grok Build `dontAsk` — deny anything not pre-approved (no interactive prompt).
    DontAsk,
    /// Grok Build `acceptEdits` — auto-approve file edit tools inside project.
    AcceptEdits,
    Deny,
    /// Grok Build `bypassPermissions` / YOLO — settings only, never default chip.
    AlwaysApprove,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self::Ask
    }
}

impl PermissionPolicy {
    pub fn parse(s: &str) -> Self {
        match s {
            "allow_for_session" | "allow_session" => Self::AllowForSession,
            "allow_once" => Self::AllowOnce,
            "deny" => Self::Deny,
            "dont_ask" | "dontask" => Self::DontAsk,
            "accept_edits" | "acceptedits" => Self::AcceptEdits,
            "always_approve" | "always" | "bypass_permissions" | "bypasspermissions" | "yolo" => {
                Self::AlwaysApprove
            }
            _ => Self::Ask,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AllowOnce => "allow_once",
            Self::AllowForSession => "allow_for_session",
            Self::DontAsk => "dont_ask",
            Self::AcceptEdits => "accept_edits",
            Self::Deny => "deny",
            Self::AlwaysApprove => "always_approve",
        }
    }
}

/// Tools treated as file edits for `acceptEdits` mode (aligned with Grok Build docs).
pub fn is_edit_tool(tool_name: &str) -> bool {
    let t = tool_name.to_lowercase();
    matches!(
        t.as_str(),
        "search_replace"
            | "write"
            | "edit"
            | "apply_patch"
            | "str_replace"
            | "strreplace"
            | "create_file"
            | "delete_file"
            | "notebook_edit"
            | "editnotebook"
    ) || t.contains("edit")
        || t.contains("write")
        || t.contains("replace")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub request_id: u64,
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub title: String,
    pub preview: String,
    pub scope_key: String,
    pub outside_project: bool,
}

/// Build scope_key = tool_name + ":" + normalize(path_or_command_prefix).
pub fn scope_key(tool_name: &str, path_or_command: &str) -> String {
    let norm = if is_shell_scope_tool(tool_name) {
        normalize_command_scope(path_or_command)
    } else {
        normalize_resource_scope(path_or_command, None)
    };
    format!("{tool_name}:{norm}")
}

pub fn normalize_scope_target(raw: &str) -> String {
    normalize_command_scope(raw)
}

fn normalize_command_scope(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "*".into();
    }
    // Preserve the complete command instead of widening `npm test` to every
    // `npm` invocation. Only insignificant whitespace is collapsed.
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_shell_scope_tool(tool_name: &str) -> bool {
    let tool = tool_name.trim().to_ascii_lowercase();
    ["shell", "bash", "terminal", "command", "execute", "run"]
        .iter()
        .any(|part| tool.contains(part))
}

fn normalize_resource_scope(raw: &str, project_root: Option<&Path>) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "*".into();
    }
    let raw_path = PathBuf::from(trimmed);
    let candidate = if raw_path.is_absolute() {
        raw_path
    } else if let Some(root) = project_root {
        root.join(raw_path)
    } else {
        raw_path
    };
    candidate
        .canonicalize()
        .unwrap_or_else(|_| lexical_normalize(&candidate))
        .to_string_lossy()
        .replace('\\', "/")
}

/// Build an exact, normalized scope for one permission request.
pub fn permission_scope_key(
    tool_name: &str,
    path_target: &str,
    shell_command: &str,
    project_root: Option<&Path>,
    _fallback_title: &str,
) -> String {
    let target = if is_shell_scope_tool(tool_name) && !shell_command.trim().is_empty() {
        normalize_command_scope(shell_command)
    } else if !path_target.trim().is_empty() {
        normalize_resource_scope(path_target, project_root)
    } else if !shell_command.trim().is_empty() {
        normalize_command_scope(shell_command)
    } else {
        "*".into()
    };
    format!("{tool_name}:{target}")
}

/// Lexically resolve `.` / `..` without requiring the path to exist on disk.
/// Prevents `proj/../../.ssh/id_rsa` from looking "under" proj via naive starts_with.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // Escape above root of relative path — keep a marker so starts_with fails.
                    out.push("..");
                }
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

/// Project-outside paths must never be covered by session allow (§17.3).
pub fn is_outside_project(project_root: &Path, target: &str) -> bool {
    let t = target.trim();
    if t.is_empty() || t == "*" {
        return false;
    }
    // Bare commands (shell) — path policy N/A; not treated as outside.
    if !t.contains('/') && !t.contains('\\') {
        return false;
    }

    let Ok(proj) = project_root.canonicalize() else {
        return true; // fail closed
    };
    let proj = lexical_normalize(&proj);

    let target_path = PathBuf::from(t);
    let joined = if target_path.is_absolute() {
        target_path
    } else {
        proj.join(&target_path)
    };

    // Prefer real canonicalize when path exists; always also lexical-clean.
    let target_norm = if joined.exists() {
        lexical_normalize(&joined.canonicalize().unwrap_or_else(|_| joined.clone()))
    } else {
        lexical_normalize(&joined)
    };

    // If after cleaning we still have `..` as a component, we escaped the root.
    if target_norm
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return true;
    }

    !target_norm.starts_with(&proj)
}

/// Extract a path-like target from ACP permission / tool_call payload.
pub fn extract_path_target(raw: &serde_json::Value) -> String {
    let candidates = [
        raw.pointer("/toolCall/locations/0/path"),
        raw.pointer("/toolCall/rawInput/path"),
        raw.pointer("/toolCall/rawInput/file_path"),
        raw.pointer("/toolCall/path"),
        raw.pointer("/locations/0/path"),
        raw.pointer("/rawInput/path"),
        raw.pointer("/rawInput/file_path"),
        raw.pointer("/path"),
        raw.pointer("/file_path"),
    ];
    for c in candidates {
        if let Some(s) = c.and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    if let Some(title) = raw
        .pointer("/toolCall/title")
        .or_else(|| raw.get("title"))
        .and_then(|v| v.as_str())
    {
        if title.contains('/') || title.contains('\\') {
            return title.to_string();
        }
    }
    String::new()
}

/// Extract shell command text from ACP permission / tool_call payload.
pub fn extract_shell_command(raw: &serde_json::Value) -> String {
    let candidates = [
        raw.pointer("/toolCall/rawInput/command"),
        raw.pointer("/rawInput/command"),
        raw.pointer("/toolCall/command"),
        raw.pointer("/command"),
    ];
    for c in candidates {
        if let Some(s) = c.and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    String::new()
}

/// Decide whether Host may auto-approve without UI.
///
/// Rules (H05 + §17.3 + Grok Build permission modes):
/// - Outside project → never auto (even with session cache; AlwaysApprove is the only global YOLO)
/// - Deny / DontAsk policy → never auto-allow
/// - Session cache hit + in-project → auto (even when chip policy is Ask — "Allow for session")
/// - AcceptEdits → auto for edit tools in-project
/// - AlwaysApprove → auto (settings YOLO / bypassPermissions)
/// - else → false (must prompt)
pub fn may_auto_allow(
    policy: PermissionPolicy,
    cache: &SessionAllowCache,
    scope: &str,
    project_root: Option<&Path>,
    path_target: &str,
    tool_name: &str,
    _command: &str,
) -> bool {
    let outside = if path_target.is_empty() {
        false
    } else {
        project_root
            .map(|p| is_outside_project(p, path_target))
            .unwrap_or(true) // no project → fail closed for path-bearing tools
    };

    // §17.3: 项目外路径永不被 session allow 覆盖
    if outside {
        return matches!(policy, PermissionPolicy::AlwaysApprove);
    }

    if matches!(
        policy,
        PermissionPolicy::Deny | PermissionPolicy::DontAsk
    ) {
        return false;
    }

    // A Runtime payload shape we do not understand must not turn AcceptEdits
    // into a project-external write bypass.
    if matches!(policy, PermissionPolicy::AcceptEdits)
        && is_edit_tool(tool_name)
        && path_target.trim().is_empty()
    {
        return false;
    }

    // H05: once user chose "Allow for session", cache hits auto-allow under Ask chip too.
    if !scope.ends_with(":*") && cache.is_allowed(scope) {
        return true;
    }

    if matches!(policy, PermissionPolicy::AcceptEdits) && is_edit_tool(tool_name) {
        return true;
    }

    matches!(policy, PermissionPolicy::AlwaysApprove)
}

/// `dontAsk`: deny without interactive prompt when not auto-allowed.
pub fn may_auto_deny(policy: PermissionPolicy) -> bool {
    matches!(policy, PermissionPolicy::DontAsk | PermissionPolicy::Deny)
}

/// Resolve effective permission tier for a project/session context (L10).
///
/// Cascade (most specific wins): session override → project tier → global default.
/// Untrusted projects always force **Ask** so a stored relaxed tier cannot apply
/// before the user trusts the folder.
pub fn effective_permission_policy(
    global: &str,
    project_trusted: Option<bool>,
    project_policy: Option<&str>,
    session_policy: Option<&str>,
) -> PermissionPolicy {
    if project_trusted == Some(false) {
        return PermissionPolicy::Ask;
    }
    if let Some(s) = session_policy.map(str::trim).filter(|s| !s.is_empty()) {
        return PermissionPolicy::parse(s);
    }
    if let Some(p) = project_policy.map(str::trim).filter(|s| !s.is_empty()) {
        return PermissionPolicy::parse(p);
    }
    let g = global.trim();
    if g.is_empty() {
        PermissionPolicy::Ask
    } else {
        PermissionPolicy::parse(g)
    }
}

/// Pick optionId from ACP permission options by preferred kind.
pub fn pick_option_id(options: &serde_json::Value, prefer: &str) -> Option<String> {
    let arr = options.as_array()?;
    let prefer = prefer.to_lowercase();
    for o in arr {
        let kind = o
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        if kind == prefer {
            return o
                .get("optionId")
                .or_else(|| o.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    for o in arr {
        let name = o
            .get("name")
            .or_else(|| o.get("label"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        if name.contains(&prefer) || name.contains(&prefer.replace('_', " ")) {
            return o
                .get("optionId")
                .or_else(|| o.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    None
}

#[derive(Debug, Default)]
pub struct SessionAllowCache {
    keys: HashSet<String>,
}

impl SessionAllowCache {
    pub fn allow(&mut self, key: String) {
        self.keys.insert(key);
    }

    pub fn is_allowed(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    pub fn clear(&mut self) {
        self.keys.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_key_shell_preserves_normalized_command() {
        assert_eq!(
            scope_key("shell", " npm   install foo "),
            "shell:npm install foo"
        );
        assert_eq!(scope_key("shell", "cargo test"), "shell:cargo test");
    }

    #[test]
    fn scope_key_fs_write_normalizes_path() {
        let k = scope_key("fs.write", "/Users/me/proj//src/a.rs");
        assert_eq!(k, "fs.write:/Users/me/proj/src/a.rs");
    }

    #[test]
    fn permission_scope_resolves_relative_resource_against_project() {
        let root = std::env::temp_dir().join("sunsetz-perm-scope-root");
        let _ = std::fs::create_dir_all(root.join("src"));
        let key = permission_scope_key("write", "src/../src/a.rs", "", Some(&root), "Write file");
        assert_eq!(key, format!("write:{}/src/a.rs", root.to_string_lossy()));
    }

    #[test]
    fn permission_scope_keeps_complete_compound_command() {
        let first = permission_scope_key(
            "run_terminal_command",
            "",
            "npm test && npm run build",
            None,
            "Run",
        );
        let second = permission_scope_key(
            "run_terminal_command",
            "",
            "npm test && npm run deploy",
            None,
            "Run",
        );
        assert_ne!(first, second);
        assert!(first.ends_with("npm test && npm run build"));

        let with_location = permission_scope_key(
            "run_terminal_command",
            "/tmp/project",
            "npm test && npm run build",
            None,
            "Run",
        );
        assert_eq!(with_location, first);
    }

    #[test]
    fn unknown_scope_never_uses_session_auto_allow() {
        let scope = permission_scope_key("custom_tool", "", "", None, "Generic action");
        assert_eq!(scope, "custom_tool:*");
        let mut cache = SessionAllowCache::default();
        cache.allow(scope.clone());
        assert!(!may_auto_allow(
            PermissionPolicy::Ask,
            &cache,
            &scope,
            None,
            "",
            "custom_tool",
            "",
        ));
    }

    #[test]
    fn default_policy_is_ask_not_always() {
        assert_eq!(PermissionPolicy::default(), PermissionPolicy::Ask);
    }

    #[test]
    fn effective_permission_defaults_to_ask() {
        assert_eq!(
            effective_permission_policy("", None, None, None),
            PermissionPolicy::Ask
        );
        assert_eq!(
            effective_permission_policy("always_approve", None, None, None),
            PermissionPolicy::AlwaysApprove
        );
    }

    #[test]
    fn effective_permission_cascades_session_over_project_over_global() {
        assert_eq!(
            effective_permission_policy(
                "ask",
                Some(true),
                Some("accept_edits"),
                Some("always_approve"),
            ),
            PermissionPolicy::AlwaysApprove
        );
        assert_eq!(
            effective_permission_policy("ask", Some(true), Some("accept_edits"), None),
            PermissionPolicy::AcceptEdits
        );
        assert_eq!(
            effective_permission_policy("dont_ask", Some(true), None, None),
            PermissionPolicy::DontAsk
        );
    }

    #[test]
    fn untrusted_project_forces_ask_despite_relaxed_tiers() {
        assert_eq!(
            effective_permission_policy(
                "always_approve",
                Some(false),
                Some("always_approve"),
                Some("always_approve"),
            ),
            PermissionPolicy::Ask
        );
        assert_eq!(
            effective_permission_policy("accept_edits", Some(false), Some("accept_edits"), None),
            PermissionPolicy::Ask
        );
    }

    #[test]
    fn empty_session_or_project_tier_falls_through() {
        assert_eq!(
            effective_permission_policy("ask", Some(true), Some(""), Some("  ")),
            PermissionPolicy::Ask
        );
        assert_eq!(
            effective_permission_policy("accept_edits", Some(true), Some(""), None),
            PermissionPolicy::AcceptEdits
        );
    }

    #[test]
    fn session_cache_roundtrip() {
        let mut c = SessionAllowCache::default();
        c.allow("shell:npm".into());
        assert!(c.is_allowed("shell:npm"));
        assert!(!c.is_allowed("shell:rm"));
    }

    #[test]
    fn ask_with_session_cache_auto_allows_in_project() {
        // H05: Allow for session under default Ask chip
        let mut c = SessionAllowCache::default();
        let root = std::env::temp_dir().join("sunsetz-perm-ask-cache");
        let _ = std::fs::create_dir_all(root.join("src"));
        let inside = root.join("src/a.rs");
        let _ = std::fs::write(&inside, "x");
        let sk = scope_key("fs.write", &inside.to_string_lossy());
        c.allow(sk.clone());

        assert!(
            may_auto_allow(
                PermissionPolicy::Ask,
                &c,
                &sk,
                Some(&root),
                &inside.to_string_lossy(),
                "write",
                "",
            ),
            "Ask + session cache hit + in-project must auto-allow (H05)"
        );
    }

    #[test]
    fn ask_with_session_cache_never_outside() {
        let mut c = SessionAllowCache::default();
        let root = std::env::temp_dir().join("sunsetz-perm-ask-out");
        let _ = std::fs::create_dir_all(&root);
        let outside = "/etc/passwd";
        let sk_out = scope_key("fs.write", outside);
        c.allow(sk_out.clone());
        assert!(!may_auto_allow(
            PermissionPolicy::Ask,
            &c,
            &sk_out,
            Some(&root),
            outside,
            "write",
            "",
        ));
    }

    #[test]
    fn ask_without_cache_does_not_auto() {
        let c = SessionAllowCache::default();
        let root = std::env::temp_dir().join("sunsetz-perm-ask-empty");
        let _ = std::fs::create_dir_all(&root);
        let inside = root.join("f.txt");
        assert!(!may_auto_allow(
            PermissionPolicy::Ask,
            &c,
            "fs.write:/x",
            Some(&root),
            &inside.to_string_lossy(),
            "write",
            "",
        ));
    }

    #[test]
    fn accept_edits_auto_allows_edit_tools() {
        let c = SessionAllowCache::default();
        let root = std::env::temp_dir().join("sunsetz-perm-accept");
        let _ = std::fs::create_dir_all(&root);
        let inside = root.join("f.txt");
        let _ = std::fs::write(&inside, "x");
        assert!(may_auto_allow(
            PermissionPolicy::AcceptEdits,
            &c,
            "write:x",
            Some(&root),
            &inside.to_string_lossy(),
            "search_replace",
            "",
        ));
        assert!(!may_auto_allow(
            PermissionPolicy::AcceptEdits,
            &c,
            "bash:x",
            Some(&root),
            "ls",
            "run_terminal_command",
            "ls -la",
        ));
        assert!(!may_auto_allow(
            PermissionPolicy::AcceptEdits,
            &c,
            "write:unknown",
            Some(&root),
            "",
            "search_replace",
            "",
        ));
    }

    #[test]
    fn download_into_project_requires_explicit_approval() {
        let c = SessionAllowCache::default();
        let root = std::env::temp_dir().join("sunsetz-perm-dl");
        let _ = std::fs::create_dir_all(root.join("outputs"));
        let dest = root.join("outputs/kitten.png");
        let cmd = format!(
            "mkdir -p {}/outputs && curl -sL -o {} \"https://example.com/a.png\"",
            root.display(),
            dest.display()
        );
        assert!(!may_auto_allow(
            PermissionPolicy::AllowForSession,
            &c,
            "execute:run_terminal_command",
            Some(&root),
            "",
            "run_terminal_command",
            &cmd,
        ));
        assert!(!may_auto_allow(
            PermissionPolicy::Ask,
            &c,
            "execute:run_terminal_command",
            Some(&root),
            "",
            "run_terminal_command",
            &cmd,
        ));
    }

    #[test]
    fn download_outside_project_not_auto_allowed() {
        let c = SessionAllowCache::default();
        let root = std::env::temp_dir().join("sunsetz-perm-dl-out");
        let _ = std::fs::create_dir_all(&root);
        let cmd = "curl -sL -o /etc/passwd https://example.com/x";
        assert!(!may_auto_allow(
            PermissionPolicy::AllowForSession,
            &c,
            "execute:run_terminal_command",
            Some(&root),
            "",
            "run_terminal_command",
            cmd,
        ));
    }

    #[test]
    fn extract_shell_command_from_raw() {
        let raw = serde_json::json!({
            "toolCall": {
                "rawInput": { "command": "curl -o out.png https://x" }
            }
        });
        assert_eq!(extract_shell_command(&raw), "curl -o out.png https://x");
    }

    #[test]
    fn relative_traversal_is_outside_project() {
        let root = std::env::temp_dir().join("sunsetz-perm-trav");
        let _ = std::fs::create_dir_all(&root);
        // Non-existent relative escape
        assert!(
            is_outside_project(&root, "../../.ssh/id_rsa"),
            "relative .. escape must be outside"
        );
        assert!(is_outside_project(
            &root,
            &format!("{}/../../.ssh/id_rsa", root.display())
        ));
        // Inside relative
        let _ = std::fs::create_dir_all(root.join("src"));
        assert!(!is_outside_project(&root, "src/a.rs"));
    }

    #[test]
    fn lexical_normalize_pops_parent() {
        let p = PathBuf::from("/Users/me/proj/src/../../.ssh/id_rsa");
        let n = lexical_normalize(&p);
        assert_eq!(n, PathBuf::from("/Users/me/.ssh/id_rsa"));
    }

    #[test]
    fn outside_project_never_auto_via_session_cache() {
        let mut c = SessionAllowCache::default();
        let root = std::env::temp_dir().join("sunsetz-perm-proj");
        let _ = std::fs::create_dir_all(&root);
        let inside = root.join("src/a.rs");
        let _ = std::fs::create_dir_all(inside.parent().unwrap());
        let _ = std::fs::write(&inside, "x");
        let sk = scope_key("fs.write", &inside.to_string_lossy());
        c.allow(sk.clone());
        assert!(may_auto_allow(
            PermissionPolicy::AllowForSession,
            &c,
            &sk,
            Some(&root),
            &inside.to_string_lossy(),
            "write",
            "",
        ));
        let outside = "/etc/passwd";
        let sk_out = scope_key("fs.write", outside);
        c.allow(sk_out.clone());
        assert!(!may_auto_allow(
            PermissionPolicy::AllowForSession,
            &c,
            &sk_out,
            Some(&root),
            outside,
            "write",
            "",
        ));
    }

    #[test]
    fn pick_option_id_prefers_kind() {
        let opts = serde_json::json!([
            {"optionId": "allow-once", "name": "Allow once", "kind": "allow_once"},
            {"optionId": "allow-always", "name": "Allow always", "kind": "allow_always"},
            {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}
        ]);
        assert_eq!(
            pick_option_id(&opts, "allow_once").as_deref(),
            Some("allow-once")
        );
        assert_eq!(
            pick_option_id(&opts, "reject_once").as_deref(),
            Some("reject-once")
        );
    }

    #[test]
    fn extract_path_from_tool_call_payload() {
        let raw = serde_json::json!({
            "toolCall": {
                "toolCallId": "c1",
                "title": "Write file",
                "locations": [{"path": "/Users/me/proj/a.txt"}]
            }
        });
        assert_eq!(extract_path_target(&raw), "/Users/me/proj/a.txt");
    }
}
