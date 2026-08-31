//! Optional OS registration so scheduled tasks can start after Quit or reboot.
//!
//! This is not KeepAlive. Login plus a 5-minute interval relaunch `--background`.
//! The in-process 30s scheduler remains the execution loop.

use std::path::{Path, PathBuf};

pub const LABEL: &str = "dev.sunsetz.desktop.scheduler";
const START_INTERVAL_SECS: u32 = 300;

/// `Ok(true)` registered, `Ok(false)` not registered, `Err` unsupported.
pub fn registration_state() -> Result<bool, String> {
    current_registration()
}

pub fn apply(enabled: bool) -> Result<(), String> {
    let exe = current_executable()?;
    if enabled {
        register(&exe)
    } else {
        unregister()
    }
}

fn current_executable() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("current executable: {error}"))
}

pub fn launch_agent_plist(exe: &Path) -> String {
    let exe = xml_escape(&exe.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>--background</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StartInterval</key>
    <integer>{START_INTERVAL_SECS}</integer>
</dict>
</plist>
"#
    )
}

pub fn windows_schtasks_command(exe: &Path) -> String {
    format!(
        r#"schtasks /Create /TN SunsetzScheduledTasks /TR "\"{}\" --background" /SC ONLOGON /RI 5 /DU 24:00 /F"#,
        exe.display()
    )
}

pub fn systemd_user_unit(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=Sunsetz scheduled tasks\n\n[Service]\nType=simple\nExecStart={} --background\n\n[Install]\nWantedBy=default.target\n",
        exe.display()
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn macos_plist_path() -> Result<PathBuf, String> {
    let home = crate::process_util::user_home();
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

#[allow(dead_code)]
fn linux_unit_path() -> Result<PathBuf, String> {
    let home = crate::process_util::user_home();
    Ok(home
        .join(".config/systemd/user")
        .join("sunsetz-scheduler.service"))
}

fn current_registration() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(macos_plist_path()?.is_file());
    }
    #[cfg(target_os = "windows")]
    {
        return windows_task_registered();
    }
    #[cfg(target_os = "linux")]
    {
        return Ok(linux_unit_path()?.is_file());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("persistent scheduler is unsupported on this platform".into())
    }
}

fn register(exe: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return register_macos(exe);
    }
    #[cfg(target_os = "windows")]
    {
        return register_windows(exe);
    }
    #[cfg(target_os = "linux")]
    {
        return register_linux(exe);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = exe;
        Err("persistent scheduler is unsupported on this platform".into())
    }
}

fn unregister() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return unregister_macos();
    }
    #[cfg(target_os = "windows")]
    {
        return unregister_windows();
    }
    #[cfg(target_os = "linux")]
    {
        return unregister_linux();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("persistent scheduler is unsupported on this platform".into())
    }
}

#[cfg(target_os = "macos")]
fn register_macos(exe: &Path) -> Result<(), String> {
    let path = macos_plist_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, launch_agent_plist(exe)).map_err(|error| error.to_string())?;
    let user_id = macos_user_id();
    let target = format!("gui/{user_id}/{LABEL}");
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &target])
        .status();
    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", &format!("gui/{user_id}"), path.to_string_lossy().as_ref()])
        .status()
        .map_err(|error| format!("launchctl bootstrap: {error}"))?;
    if !status.success() {
        // load -w is the older path; still counts as registered if the plist exists.
        let _ = std::process::Command::new("launchctl")
            .args(["load", "-w", path.to_string_lossy().as_ref()])
            .status();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn unregister_macos() -> Result<(), String> {
    let user_id = macos_user_id();
    let target = format!("gui/{user_id}/{LABEL}");
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &target])
        .status();
    if let Ok(path) = macos_plist_path() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w", path.to_string_lossy().as_ref()])
            .status();
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_user_id() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(target_os = "windows")]
fn register_windows(exe: &Path) -> Result<(), String> {
    let status = std::process::Command::new("schtasks")
        .args([
            "/Create",
            "/TN",
            "SunsetzScheduledTasks",
            "/TR",
            &format!("\"{}\" --background", exe.display()),
            "/SC",
            "ONLOGON",
            "/RI",
            "5",
            "/DU",
            "24:00",
            "/F",
        ])
        .status()
        .map_err(|error| format!("schtasks: {error}"))?;
    if !status.success() {
        return Err("schtasks failed to register Sunsetz scheduled tasks".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn unregister_windows() -> Result<(), String> {
    let _ = std::process::Command::new("schtasks")
        .args(["/Delete", "/TN", "SunsetzScheduledTasks", "/F"])
        .status();
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_task_registered() -> Result<bool, String> {
    let output = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", "SunsetzScheduledTasks"])
        .output()
        .map_err(|error| format!("schtasks: {error}"))?;
    Ok(output.status.success())
}

#[cfg(target_os = "linux")]
fn register_linux(exe: &Path) -> Result<(), String> {
    let path = linux_unit_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, systemd_user_unit(exe)).map_err(|error| error.to_string())?;
    let timer = path.with_extension("timer");
    std::fs::write(
        &timer,
        "[Unit]\nDescription=Sunsetz scheduled tasks timer\n\n[Timer]\nOnBootSec=1min\nOnUnitActiveSec=5min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n",
    )
    .map_err(|error| error.to_string())?;
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "sunsetz-scheduler.timer"])
        .status();
    Ok(())
}

#[cfg(target_os = "linux")]
fn unregister_linux() -> Result<(), String> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", "sunsetz-scheduler.timer"])
        .status();
    if let Ok(path) = linux_unit_path() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("timer"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn launch_agent_plist_starts_background_without_keepalive() {
        let plist = launch_agent_plist(Path::new("/Applications/Sunsetz.app/Contents/MacOS/Sunsetz"));
        assert!(plist.contains(LABEL));
        assert!(plist.contains("--background"));
        assert!(plist.contains("StartInterval"));
        assert!(!plist.to_ascii_lowercase().contains("keepalive"));
        assert!(plist.contains("/Applications/Sunsetz.app/Contents/MacOS/Sunsetz"));
    }

    #[test]
    fn windows_command_uses_logon_and_repeat() {
        let cmd = windows_schtasks_command(Path::new(r"C:\Program Files\Sunsetz\Sunsetz.exe"));
        assert!(cmd.contains("--background"));
        assert!(cmd.contains("ONLOGON"));
        assert!(cmd.contains("/RI 5"));
        assert!(!cmd.to_ascii_lowercase().contains("keepalive"));
    }

    #[test]
    fn systemd_unit_runs_background_binary() {
        let unit = systemd_user_unit(&PathBuf::from("/usr/bin/sunsetz"));
        assert!(unit.contains("/usr/bin/sunsetz --background"));
        assert!(unit.contains("WantedBy=default.target"));
    }
}
