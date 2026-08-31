//! Host-owned isolation for Sunsetz `run_command`.
//!
//! The default kernel applies `sandboxProfile` to each command child. Linux uses
//! bubblewrap; macOS uses `sandbox-exec`. Windows uses an AppContainer plus a
//! Job Object. Other platforms fail closed when a non-off profile is requested.
//! This is separate from the legacy ACP Runtime process wrapper in
//! `runtime_compat::sandbox_launch_plan`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::runtime_compat::{platform_name, SandboxApplicationV1, SandboxProfileV1};

#[cfg(target_os = "windows")]
#[path = "command_sandbox_windows.rs"]
mod windows_isolation;
#[cfg(target_os = "windows")]
pub use windows_isolation::{run_windows_isolated, run_windows_isolated_sync};

#[derive(Debug, Clone)]
pub struct CommandSandboxPlan {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub application: SandboxApplicationV1,
    #[allow(dead_code)]
    pub project_root: PathBuf,
    #[allow(dead_code)]
    pub profile: SandboxProfileV1,
}

pub fn support(profile: SandboxProfileV1) -> SandboxApplicationV1 {
    if profile == SandboxProfileV1::Off {
        return SandboxApplicationV1::off();
    }
    #[cfg(target_os = "linux")]
    {
        return match which::which("bwrap") {
            Ok(_) => applied_status(
                profile,
                "run_command uses bubblewrap for the built-in kernel",
            ),
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
    #[cfg(target_os = "macos")]
    {
        return match sandbox_exec_path() {
            Ok(_) => applied_status(
                profile,
                "run_command uses macOS sandbox-exec for the built-in kernel",
            ),
            Err(reason) => SandboxApplicationV1 {
                requested: profile.as_str().into(),
                applied: "off".into(),
                verified: false,
                state: "needs_install".into(),
                platform: platform_name().into(),
                reason: Some(reason),
            },
        };
    }
    #[cfg(target_os = "windows")]
    {
        return applied_status(
            profile,
            "run_command uses a Windows AppContainer for the built-in kernel",
        );
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        SandboxApplicationV1 {
            requested: profile.as_str().into(),
            applied: "off".into(),
            verified: false,
            state: "unsupported_platform".into(),
            platform: platform_name().into(),
            reason: Some("this platform has no supported run_command sandbox adapter".into()),
        }
    }
}

pub fn plan_run_command(
    command: &str,
    cwd: &Path,
    project_root: &Path,
    profile: SandboxProfileV1,
) -> Result<CommandSandboxPlan, String> {
    if command.trim().is_empty() {
        return Err("empty command".into());
    }
    if command.contains('\0') {
        return Err("invalid command".into());
    }
    if profile == SandboxProfileV1::Off {
        return Ok(unsandboxed_plan(command, cwd));
    }

    let support = support(profile);
    if support.state != "applied" {
        return Err(format!(
            "SANDBOX_UNAVAILABLE: {}",
            support
                .reason
                .unwrap_or_else(|| "unsupported platform".into())
        ));
    }

    #[cfg(target_os = "linux")]
    {
        return linux_bwrap_plan(command, cwd, project_root, profile, support);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_seatbelt_plan(command, cwd, project_root, profile, support);
    }
    #[cfg(target_os = "windows")]
    {
        return windows_appcontainer_plan(command, cwd, project_root, profile, support);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (cwd, project_root);
        Err(format!(
            "SANDBOX_UNAVAILABLE: {}",
            support
                .reason
                .unwrap_or_else(|| "unsupported platform".into())
        ))
    }
}

fn applied_status(profile: SandboxProfileV1, reason: &str) -> SandboxApplicationV1 {
    SandboxApplicationV1 {
        requested: profile.as_str().into(),
        applied: profile.as_str().into(),
        verified: true,
        state: "applied".into(),
        platform: platform_name().into(),
        reason: Some(reason.into()),
    }
}

fn unsandboxed_plan(command: &str, cwd: &Path) -> CommandSandboxPlan {
    if cfg!(windows) {
        CommandSandboxPlan {
            executable: PathBuf::from("cmd.exe"),
            args: vec![OsString::from("/C"), OsString::from(command)],
            current_dir: cwd.to_path_buf(),
            application: SandboxApplicationV1::off(),
            project_root: cwd.to_path_buf(),
            profile: SandboxProfileV1::Off,
        }
    } else {
        CommandSandboxPlan {
            executable: PathBuf::from("/bin/sh"),
            args: vec![OsString::from("-lc"), OsString::from(command)],
            current_dir: cwd.to_path_buf(),
            application: SandboxApplicationV1::off(),
            project_root: cwd.to_path_buf(),
            profile: SandboxProfileV1::Off,
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_bwrap_plan(
    command: &str,
    cwd: &Path,
    project_root: &Path,
    profile: SandboxProfileV1,
    application: SandboxApplicationV1,
) -> Result<CommandSandboxPlan, String> {
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
    ];
    if profile == SandboxProfileV1::WorkspaceWrite {
        args.extend([
            OsString::from("--bind"),
            project_root.as_os_str().to_os_string(),
            project_root.as_os_str().to_os_string(),
        ]);
    }
    args.extend([
        OsString::from("--chdir"),
        cwd.as_os_str().to_os_string(),
        OsString::from("--"),
        OsString::from("/bin/sh"),
        OsString::from("-lc"),
        OsString::from(command),
    ]);
    Ok(CommandSandboxPlan {
        executable: bwrap,
        args,
        current_dir: cwd.to_path_buf(),
        application,
        project_root: project_root.to_path_buf(),
        profile,
    })
}

#[cfg(target_os = "macos")]
fn macos_seatbelt_plan(
    command: &str,
    cwd: &Path,
    project_root: &Path,
    profile: SandboxProfileV1,
    application: SandboxApplicationV1,
) -> Result<CommandSandboxPlan, String> {
    let sandbox_exec = sandbox_exec_path()?;
    let policy = macos_seatbelt_profile(project_root, profile)?;
    Ok(CommandSandboxPlan {
        executable: sandbox_exec,
        args: vec![
            OsString::from("-p"),
            OsString::from(policy),
            OsString::from("/bin/sh"),
            OsString::from("-lc"),
            OsString::from(command),
        ],
        current_dir: cwd.to_path_buf(),
        application,
        project_root: project_root.to_path_buf(),
        profile,
    })
}

#[cfg(target_os = "windows")]
fn windows_appcontainer_plan(
    command: &str,
    cwd: &Path,
    project_root: &Path,
    profile: SandboxProfileV1,
    application: SandboxApplicationV1,
) -> Result<CommandSandboxPlan, String> {
    Ok(CommandSandboxPlan {
        executable: PathBuf::from("cmd.exe"),
        args: vec![OsString::from("/C"), OsString::from(command)],
        current_dir: cwd.to_path_buf(),
        application,
        project_root: project_root.to_path_buf(),
        profile,
    })
}

#[cfg(target_os = "macos")]
fn sandbox_exec_path() -> Result<PathBuf, String> {
    let bundled = PathBuf::from("/usr/bin/sandbox-exec");
    if bundled.is_file() {
        return Ok(bundled);
    }
    which::which("sandbox-exec")
        .map_err(|_| "SANDBOX_UNAVAILABLE: macOS sandbox-exec is not installed".to_string())
}

#[cfg(target_os = "macos")]
pub fn macos_seatbelt_profile(
    project_root: &Path,
    profile: SandboxProfileV1,
) -> Result<String, String> {
    let project = seatbelt_subpath(project_root)?;
    let mut policy = String::from(
        "(version 1)\n\
         (deny default)\n\
         (allow process-exec)\n\
         (allow process-fork)\n\
         (allow process-info*)\n\
         (allow signal)\n\
         (allow sysctl-read)\n\
         (allow mach-lookup)\n\
         (allow mach-register)\n\
         (allow system-socket)\n\
         (allow ipc-posix-shm)\n\
         (allow ipc-posix-sem)\n\
         (allow file-read*)\n\
         (allow file-read-metadata)\n\
         (allow file-write-data (literal \"/dev/null\"))\n\
         (allow file-write-data (literal \"/dev/dtracehelper\"))\n\
         (allow file-ioctl (literal \"/dev/null\"))\n\
         (allow file-ioctl (literal \"/dev/dtracehelper\"))\n\
         (allow file-write* (subpath \"/tmp\"))\n\
         (allow file-write* (subpath \"/private/tmp\"))\n\
         (allow file-write* (subpath \"/private/var/tmp\"))\n\
         (allow file-write* (subpath \"/var/tmp\"))\n\
         (allow file-write* (subpath \"/private/var/folders\"))\n\
         (allow file-write* (subpath \"/var/folders\"))\n\
         (allow network-outbound)\n\
         (allow network-inbound)\n\
         (allow network-bind)\n",
    );
    if profile == SandboxProfileV1::WorkspaceWrite {
        policy.push_str("(allow file-write* (subpath \"");
        policy.push_str(&project);
        policy.push_str("\"))\n");
    }
    Ok(policy)
}

#[cfg(target_os = "macos")]
fn seatbelt_subpath(path: &Path) -> Result<String, String> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let raw = canonical
        .to_str()
        .ok_or_else(|| "SANDBOX_UNAVAILABLE: sandbox path is not UTF-8".to_string())?;
    if raw
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '"' | '(' | ')'))
    {
        return Err("SANDBOX_UNAVAILABLE: sandbox path contains unsupported characters".into());
    }
    Ok(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Command, Stdio};

    fn temp_root(label: &str) -> PathBuf {
        // macOS seatbelt allows /tmp and /var/folders, so isolation tests must
        // use a HOME-backed directory outside those prefixes.
        let home =
            std::env::var("HOME").unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        let path = PathBuf::from(home)
            .join("Library/Caches/sunsetz-command-sandbox-tests")
            .join(format!("{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn run_plan(plan: &CommandSandboxPlan) -> std::process::Output {
        Command::new(&plan.executable)
            .args(&plan.args)
            .current_dir(&plan.current_dir)
            .stdin(Stdio::null())
            .output()
            .expect("spawn sandboxed command")
    }

    #[test]
    fn off_plan_uses_host_shell() {
        let root = temp_root("off");
        let plan =
            plan_run_command("echo sunsetz-off", &root, &root, SandboxProfileV1::Off).unwrap();
        assert_eq!(plan.application.applied, "off");
        if cfg!(windows) {
            assert!(plan.executable.ends_with("cmd.exe"));
            assert_eq!(plan.args[0], "/C");
        } else {
            assert_eq!(plan.executable, PathBuf::from("/bin/sh"));
            assert_eq!(plan.args[0], "-lc");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn non_off_never_silently_drops_to_host_shell() {
        let root = temp_root("no-silent");
        match plan_run_command("echo leak", &root, &root, SandboxProfileV1::WorkspaceWrite) {
            Ok(plan) => {
                assert_eq!(plan.application.applied, "workspace_write");
                if cfg!(windows) {
                    assert_eq!(plan.application.state, "applied");
                    assert!(plan.executable.ends_with("cmd.exe"));
                } else {
                    assert_ne!(plan.executable, PathBuf::from("/bin/sh"));
                    assert_ne!(plan.executable, PathBuf::from("cmd.exe"));
                }
            }
            Err(error) => {
                assert!(error.contains("SANDBOX_UNAVAILABLE"), "{error}");
            }
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_workspace_write_allows_project_and_blocks_escape() {
        let root = temp_root("write");
        let outside = temp_root("outside");
        let marker = root.join("inside.txt");
        let escaped = outside.join("escaped.txt");
        let write_inside = plan_run_command(
            "printf inside > inside.txt",
            &root,
            &root,
            SandboxProfileV1::WorkspaceWrite,
        )
        .unwrap();
        let inside_out = run_plan(&write_inside);
        assert!(
            inside_out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&inside_out.stderr)
        );
        assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "inside");

        let escape = format!("printf leaked > '{}'", escaped.display());
        let write_outside =
            plan_run_command(&escape, &root, &root, SandboxProfileV1::WorkspaceWrite).unwrap();
        let outside_out = run_plan(&write_outside);
        assert!(
            !outside_out.status.success() || !escaped.exists(),
            "escape write succeeded: status={} stderr={}",
            outside_out.status,
            String::from_utf8_lossy(&outside_out.stderr)
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_read_only_blocks_project_write_and_allows_tmp() {
        let root = temp_root("ro");
        let blocked = plan_run_command(
            "printf blocked > inside.txt",
            &root,
            &root,
            SandboxProfileV1::ReadOnly,
        )
        .unwrap();
        let blocked_out = run_plan(&blocked);
        assert!(
            !blocked_out.status.success(),
            "read_only project write succeeded: stderr={}",
            String::from_utf8_lossy(&blocked_out.stderr)
        );
        assert!(!root.join("inside.txt").exists());

        let tmp_path =
            std::env::temp_dir().join(format!("sunsetz-sandbox-tmp-{}.txt", uuid::Uuid::new_v4()));
        let tmp_cmd = format!("printf tmpok > '{}'", tmp_path.display());
        let tmp_plan =
            plan_run_command(&tmp_cmd, &root, &root, SandboxProfileV1::ReadOnly).unwrap();
        let tmp_out = run_plan(&tmp_plan);
        assert!(
            tmp_out.status.success(),
            "tmp write failed: stderr={}",
            String::from_utf8_lossy(&tmp_out.stderr)
        );
        assert_eq!(fs::read_to_string(&tmp_path).unwrap().trim(), "tmpok");
        let _ = fs::remove_file(&tmp_path);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_seatbelt_profile_mentions_project_only_for_workspace_write() {
        let root = temp_root("policy");
        let write = macos_seatbelt_profile(&root, SandboxProfileV1::WorkspaceWrite).unwrap();
        let read = macos_seatbelt_profile(&root, SandboxProfileV1::ReadOnly).unwrap();
        let canonical = root.canonicalize().unwrap();
        let shown = canonical.to_str().unwrap();
        assert!(write.contains(shown), "{write}");
        assert!(!read.contains(&format!("(subpath \"{shown}\")")), "{read}");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_plan_uses_bwrap_or_fails_closed() {
        let root = temp_root("linux");
        match plan_run_command("echo hi", &root, &root, SandboxProfileV1::ReadOnly) {
            Ok(plan) => {
                assert!(plan.executable.ends_with("bwrap"));
                let joined: Vec<String> = plan
                    .args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                assert!(joined.contains(&"--ro-bind".into()));
                assert!(!joined.windows(3).any(|window| {
                    window[0] == "--bind"
                        && window[1] == root.to_string_lossy()
                        && window[2] == root.to_string_lossy()
                }));
            }
            Err(error) => assert!(error.contains("SANDBOX_UNAVAILABLE"), "{error}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_non_off_uses_appcontainer_plan() {
        let root = temp_root("win");
        let plan = plan_run_command("echo hi", &root, &root, SandboxProfileV1::WorkspaceWrite)
            .unwrap();
        assert_eq!(plan.application.applied, "workspace_write");
        assert_eq!(plan.application.state, "applied");
        assert_eq!(plan.profile, SandboxProfileV1::WorkspaceWrite);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_workspace_write_allows_project_and_blocks_escape() {
        let root = temp_root("write");
        let outside = temp_root("outside");
        let marker = root.join("inside.txt");
        let escaped = outside.join("escaped.txt");
        let write_inside = plan_run_command(
            "echo inside> inside.txt",
            &root,
            &root,
            SandboxProfileV1::WorkspaceWrite,
        )
        .unwrap();
        match run_windows_isolated_sync(&write_inside) {
            Ok(output) => {
                assert!(
                    output.status.success(),
                    "stderr={}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "inside");
                let escape = format!("echo leaked> \"{}\"", escaped.display());
                let write_outside = plan_run_command(
                    &escape,
                    &root,
                    &root,
                    SandboxProfileV1::WorkspaceWrite,
                )
                .unwrap();
                let outside_out = run_windows_isolated_sync(&write_outside).unwrap();
                assert!(
                    !outside_out.status.success() || !escaped.exists(),
                    "escape write succeeded"
                );
            }
            Err(error) => assert!(error.contains("SANDBOX_UNAVAILABLE"), "{error}"),
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_read_only_blocks_project_write() {
        let root = temp_root("ro");
        let blocked = plan_run_command(
            "echo blocked> inside.txt",
            &root,
            &root,
            SandboxProfileV1::ReadOnly,
        )
        .unwrap();
        match run_windows_isolated_sync(&blocked) {
            Ok(output) => {
                assert!(
                    !output.status.success() || !root.join("inside.txt").exists(),
                    "read_only project write succeeded"
                );
            }
            Err(error) => assert!(error.contains("SANDBOX_UNAVAILABLE"), "{error}"),
        }
        let _ = fs::remove_dir_all(&root);
    }
}
