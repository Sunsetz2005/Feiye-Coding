//! Multi Sunsetz account switcher: snapshot/switch official CLI auth.json profiles.
//!
//! Layout under app data root:
//!   accounts/index.json
//!   accounts/<id>/auth.json
//!
//! Switching copies the selected auth into `~/.grok/auth.json` and agent-home.

use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::account::{self, AccountProfile};
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccount {
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    /// User-editable label (defaults to email / display name).
    pub label: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountsIndex {
    #[serde(default)]
    active_id: Option<String>,
    #[serde(default)]
    profiles: Vec<SavedAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsListResult {
    pub profiles: Vec<SavedAccount>,
    pub active_id: Option<String>,
}

fn accounts_root() -> PathBuf {
    paths::app_data_root().join("accounts")
}

fn index_path() -> PathBuf {
    accounts_root().join("index.json")
}

fn profile_auth_path(id: &str) -> PathBuf {
    accounts_root().join(id).join("auth.json")
}

fn load_index() -> AccountsIndex {
    let p = index_path();
    if !p.is_file() {
        return AccountsIndex::default();
    }
    match fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => AccountsIndex::default(),
    }
}

fn save_index(idx: &AccountsIndex) -> Result<(), String> {
    let root = accounts_root();
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(idx).map_err(|e| e.to_string())?;
    fs::write(index_path(), raw).map_err(|e| e.to_string())
}

fn label_from_profile(p: &AccountProfile) -> String {
    p.email
        .clone()
        .or_else(|| p.display_name.clone())
        .unwrap_or_else(|| "Sunsetz account".into())
}

/// List saved account snapshots + active id.
pub fn list_accounts() -> AccountsListResult {
    let idx = load_index();
    AccountsListResult {
        profiles: idx.profiles,
        active_id: idx.active_id,
    }
}

/// Snapshot current official CLI auth into the multi-account store.
/// If already signed in as a known email, update that slot; else create new.
pub fn save_current_account(label: Option<String>) -> Result<SavedAccount, String> {
    let src = current_auth_source()?;
    let profile = account::read_auth_profile();
    if !profile.signed_in {
        return Err("Not signed in — log in first, then save the account.".into());
    }

    let mut idx = load_index();
    let email = profile.email.clone();
    let now = Utc::now().to_rfc3339();

    // Prefer update by matching email
    if let Some(existing) = idx.profiles.iter_mut().find(|a| {
        email.is_some() && a.email.as_ref() == email.as_ref()
    }) {
        let id = existing.id.clone();
        let dest = profile_auth_path(&id);
        fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o600));
        }
        existing.email = profile.email.clone();
        existing.display_name = profile.display_name.clone();
        if let Some(l) = label.filter(|s| !s.trim().is_empty()) {
            existing.label = l.trim().to_string();
        } else if existing.label.is_empty() {
            existing.label = label_from_profile(&profile);
        }
        existing.updated_at = now;
        idx.active_id = Some(id.clone());
        let out = existing.clone();
        save_index(&idx)?;
        info!("account_profiles: updated snapshot id={id}");
        return Ok(out);
    }

    let id = Uuid::new_v4().to_string();
    let dest = profile_auth_path(&id);
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o600));
    }

    let saved = SavedAccount {
        id: id.clone(),
        email: profile.email.clone(),
        display_name: profile.display_name.clone(),
        label: label
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| label_from_profile(&profile)),
        updated_at: now,
    };
    idx.profiles.push(saved.clone());
    idx.active_id = Some(id);
    save_index(&idx)?;
    info!("account_profiles: saved new snapshot id={}", saved.id);
    Ok(saved)
}

/// Activate a saved account: write auth into CLI home + agent-home.
pub fn switch_account(id: &str) -> Result<AccountProfile, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("empty account id".into());
    }
    let src = profile_auth_path(id);
    if !src.is_file() {
        return Err(format!("account snapshot not found: {id}"));
    }

    // Write to both default CLI auth and agent-home
    let cli_auth = crate::process_util::user_home().join(".grok").join("auth.json");
    if let Some(parent) = cli_auth.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(&src, &cli_auth).map_err(|e| format!("write ~/.grok/auth.json: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&cli_auth, fs::Permissions::from_mode(0o600));
    }

    if let Err(e) = account::sync_cli_auth_to_agent_home() {
        warn!("account_profiles: agent-home sync after switch: {e}");
    }

    let mut idx = load_index();
    idx.active_id = Some(id.to_string());
    let _ = save_index(&idx);

    let profile = account::read_auth_profile();
    if !profile.signed_in {
        return Err("Switched auth file but profile still unsigned-in. Snapshot may be corrupt.".into());
    }
    info!("account_profiles: switched to id={id}");
    Ok(profile)
}

pub fn remove_account(id: &str) -> Result<(), String> {
    let id = id.trim();
    let mut idx = load_index();
    let before = idx.profiles.len();
    idx.profiles.retain(|p| p.id != id);
    if idx.profiles.len() == before {
        return Err(format!("account not found: {id}"));
    }
    if idx.active_id.as_deref() == Some(id) {
        idx.active_id = idx.profiles.first().map(|p| p.id.clone());
    }
    save_index(&idx)?;
    let dir = accounts_root().join(id);
    if dir.is_dir() {
        let _ = fs::remove_dir_all(&dir);
    }
    Ok(())
}

pub fn rename_account(id: &str, label: &str) -> Result<SavedAccount, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("label empty".into());
    }
    let mut idx = load_index();
    let slot = idx
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("account not found: {id}"))?;
    slot.label = label.to_string();
    slot.updated_at = Utc::now().to_rfc3339();
    let out = slot.clone();
    save_index(&idx)?;
    Ok(out)
}

fn current_auth_source() -> Result<PathBuf, String> {
    let primary = crate::process_util::user_home().join(".grok").join("auth.json");
    if primary.is_file() {
        return Ok(primary);
    }
    let agent = paths::agent_home_dir().join("auth.json");
    if agent.is_file() {
        return Ok(agent);
    }
    Err("No auth.json found. Sign in first.".into())
}

/// After a successful login, auto-snapshot so multi-account list stays current.
pub fn auto_snapshot_after_login() {
    match save_current_account(None) {
        Ok(s) => info!("account_profiles: auto-snapshot after login id={}", s.id),
        Err(e) => warn!("account_profiles: auto-snapshot skipped: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_fallback() {
        let p = AccountProfile {
            signed_in: true,
            auth_mode: None,
            email: Some("a@b.com".into()),
            display_name: Some("A".into()),
            user_id: None,
            team_id: None,
            principal_type: None,
            expires_at: None,
            expired: false,
            has_refresh: false,
            oidc_issuer: None,
        };
        assert_eq!(label_from_profile(&p), "a@b.com");
    }
}
