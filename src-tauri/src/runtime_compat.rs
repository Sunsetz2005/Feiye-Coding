//! Private compatibility boundary for the upstream runtime.
//!
//! Product code uses Sunsetz-owned names. The values in this module preserve
//! compatibility with the installed upstream CLI and its existing data.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const PRODUCT_HOME_ENV: &str = "SUNSETZ_HOME";
pub const LEGACY_PRODUCT_HOME_ENV: &str = "GROK_APP_HOME";
pub const PRODUCT_ACP_ENV: &str = "SUNSETZ_ACP";
pub const LEGACY_PRODUCT_ACP_ENV: &str = "GROK_APP_ACP";
/// Explicit product kernel selector. `grok_acp` keeps the legacy ACP adapter.
pub const PRODUCT_RUNTIME_BACKEND_ENV: &str = "SUNSETZ_RUNTIME_BACKEND";
pub const ACP_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxProfileV1 {
    Off,
    WorkspaceWrite,
    ReadOnly,
}

impl Default for SandboxProfileV1 {
    fn default() -> Self {
        Self::Off
    }
}

impl SandboxProfileV1 {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "workspace_write" | "workspace-write" => Self::WorkspaceWrite,
            "read_only" | "read-only" => Self::ReadOnly,
            _ => Self::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::WorkspaceWrite => "workspace_write",
            Self::ReadOnly => "read_only",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SandboxApplicationV1 {
    pub requested: String,
    pub applied: String,
    pub verified: bool,
    pub state: String,
    pub platform: String,
    pub reason: Option<String>,
}

impl SandboxApplicationV1 {
    pub fn off() -> Self {
        Self {
            requested: "off".into(),
            applied: "off".into(),
            verified: true,
            state: "off".into(),
            platform: platform_name().into(),
            reason: None,
        }
    }
}

pub struct SandboxLaunchPlan {
    pub executable: PathBuf,
    pub prefix_args: Vec<OsString>,
    pub application: SandboxApplicationV1,
}

pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

pub fn sandbox_support(profile: SandboxProfileV1) -> SandboxApplicationV1 {
    if profile == SandboxProfileV1::Off {
        return SandboxApplicationV1::off();
    }
    #[cfg(target_os = "linux")]
    {
        return match which::which("bwrap") {
            Ok(_) => SandboxApplicationV1 {
                requested: profile.as_str().into(),
                applied: "off".into(),
                verified: false,
                state: "available".into(),
                platform: platform_name().into(),
                reason: Some("bubblewrap will be applied on the next Runtime spawn".into()),
            },
            Err(_) => SandboxApplicationV1 {
                requested: profile.as_str().into(),
                applied: "off".into(),
                verified: false,
                state: "needs_install".into(),
                platform: platform_name().into(),
                reason: Some("bubblewrap (bwrap) is not installed".into()),
            },
        };
    }
    #[cfg(not(target_os = "linux"))]
    {
        SandboxApplicationV1 {
            requested: profile.as_str().into(),
            applied: "off".into(),
            verified: false,
            state: "unsupported_platform".into(),
            platform: platform_name().into(),
            reason: Some(
                if cfg!(target_os = "macos") {
                    "macOS has no supported Runtime subprocess sandbox adapter"
                } else if cfg!(target_os = "windows") {
                    "Windows has no supported Runtime subprocess sandbox adapter"
                } else {
                    "this platform has no supported Runtime subprocess sandbox adapter"
                }
                .into(),
            ),
        }
    }
}

pub fn sandbox_launch_plan(
    cli_path: &Path,
    cwd: &Path,
    runtime_home: &Path,
    profile: SandboxProfileV1,
) -> Result<SandboxLaunchPlan, String> {
    if profile == SandboxProfileV1::Off {
        return Ok(SandboxLaunchPlan {
            executable: cli_path.to_path_buf(),
            prefix_args: Vec::new(),
            application: SandboxApplicationV1::off(),
        });
    }

    #[cfg(target_os = "linux")]
    {
        let bwrap = which::which("bwrap")
            .map_err(|_| "SANDBOX_UNAVAILABLE: bubblewrap (bwrap) is not installed".to_string())?;
        let mut args = vec![
            OsString::from("--die-with-parent"),
            OsString::from("--new-session"),
            OsString::from("--unshare-all"),
            OsString::from("--share-net"),
            OsString::from("--proc"),
            OsString::from("/proc"),
            OsString::from("--dev"),
            OsString::from("/dev"),
            OsString::from("--ro-bind"),
            OsString::from("/"),
            OsString::from("/"),
            OsString::from("--tmpfs"),
            OsString::from("/tmp"),
            OsString::from("--bind"),
            runtime_home.as_os_str().to_os_string(),
            runtime_home.as_os_str().to_os_string(),
        ];
        if profile == SandboxProfileV1::WorkspaceWrite {
            args.extend([
                OsString::from("--bind"),
                cwd.as_os_str().to_os_string(),
                cwd.as_os_str().to_os_string(),
            ]);
        }
        args.extend([OsString::from("--"), cli_path.as_os_str().to_os_string()]);
        return Ok(SandboxLaunchPlan {
            executable: bwrap,
            prefix_args: args,
            application: SandboxApplicationV1 {
                requested: profile.as_str().into(),
                applied: profile.as_str().into(),
                verified: true,
                state: "applied".into(),
                platform: platform_name().into(),
                reason: Some("Runtime process launched through bubblewrap".into()),
            },
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (cwd, runtime_home);
        let support = sandbox_support(profile);
        Err(format!(
            "SANDBOX_UNAVAILABLE: {}",
            support
                .reason
                .unwrap_or_else(|| "unsupported platform".into())
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFeatureV1 {
    pub state: String,
    pub source: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilitiesV1 {
    pub version: u8,
    pub runtime_version: Option<String>,
    pub protocol_version: u32,
    pub client_version: String,
    pub platform: String,
    pub sandbox: SandboxApplicationV1,
    pub memory: RuntimeFeatureV1,
    pub plugin_catalog: RuntimeFeatureV1,
    pub hooks_inventory: RuntimeFeatureV1,
    pub mcp: RuntimeFeatureV1,
}

pub fn runtime_capabilities(active_sandbox: Option<SandboxApplicationV1>) -> RuntimeCapabilitiesV1 {
    let settings = crate::store::load_settings();
    let probe = crate::cli_probe::probe_cli(settings.manual_cli_path.as_deref());
    let requested = SandboxProfileV1::parse(&settings.sandbox_profile);
    let sandbox = active_sandbox.unwrap_or_else(|| sandbox_support(requested));
    let runtime_state = if probe.found {
        "available"
    } else {
        "needs_install"
    };
    let runtime_reason = (!probe.found).then(|| "Sunsetz Runtime is not installed".into());
    RuntimeCapabilitiesV1 {
        version: 1,
        runtime_version: probe.version,
        protocol_version: ACP_PROTOCOL_VERSION,
        client_version: env!("CARGO_PKG_VERSION").into(),
        platform: platform_name().into(),
        sandbox,
        memory: RuntimeFeatureV1 {
            state: "unavailable".into(),
            source: "host".into(),
            reason: Some("No machine-readable Runtime memory bridge is registered".into()),
        },
        plugin_catalog: RuntimeFeatureV1 {
            state: runtime_state.into(),
            source: "runtime_cli".into(),
            reason: runtime_reason.clone(),
        },
        hooks_inventory: RuntimeFeatureV1 {
            state: runtime_state.into(),
            source: "runtime_cli".into(),
            reason: runtime_reason.clone(),
        },
        mcp: RuntimeFeatureV1 {
            state: runtime_state.into(),
            source: "runtime_acp".into(),
            reason: runtime_reason,
        },
    }
}

pub fn product_home_override() -> Option<PathBuf> {
    std::env::var(PRODUCT_HOME_ENV)
        .or_else(|_| std::env::var(LEGACY_PRODUCT_HOME_ENV))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

/// Serialize tests that mutate process environment (`SUNSETZ_HOME`,
/// `GROK_APP_HOME`, connector URL overrides). Per-module mutexes still race.
#[cfg(test)]
pub fn lock_test_process_env() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

pub fn use_mock_runtime() -> bool {
    std::env::var(PRODUCT_ACP_ENV)
        .or_else(|_| std::env::var(LEGACY_PRODUCT_ACP_ENV))
        .map(|value| value.eq_ignore_ascii_case("mock"))
        .unwrap_or(false)
}

pub fn public_model_label(label: &str) -> String {
    label.replace("Grok", "Sunsetz").replace("grok", "Sunsetz")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_profiles_are_strict_and_default_off() {
        assert_eq!(
            SandboxProfileV1::parse("workspace_write"),
            SandboxProfileV1::WorkspaceWrite
        );
        assert_eq!(
            SandboxProfileV1::parse("read-only"),
            SandboxProfileV1::ReadOnly
        );
        assert_eq!(SandboxProfileV1::parse("unknown"), SandboxProfileV1::Off);
    }

    #[test]
    fn off_is_always_verified_without_adapter() {
        let status = sandbox_support(SandboxProfileV1::Off);
        assert_eq!(status.requested, "off");
        assert_eq!(status.applied, "off");
        assert!(status.verified);
    }
}
