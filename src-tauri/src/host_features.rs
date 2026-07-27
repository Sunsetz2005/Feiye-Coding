//! Small host capability surface for controls that must not be decorative.

use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Available,
    Unavailable,
    NeedsPermission,
    NeedsInstall,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostCapability {
    pub state: CapabilityState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilities {
    pub version: u32,
    pub platform: String,
    pub finder_selection: bool,
    pub speech_recognition: bool,
    pub skill_draft_save: bool,
    pub capabilities: BTreeMap<String, HostCapability>,
}

pub fn capabilities() -> HostCapabilities {
    let finder_selection = cfg!(target_os = "macos");
    let mut capability_map = BTreeMap::new();
    capability_map.insert(
        "finderSelection".to_string(),
        HostCapability {
            state: if finder_selection {
                CapabilityState::Available
            } else {
                CapabilityState::UnsupportedPlatform
            },
            reason: None,
            version: Some("1".to_string()),
        },
    );
    capability_map.insert(
        "speechRecognition".to_string(),
        HostCapability {
            state: CapabilityState::Unavailable,
            reason: Some("No native speech adapter is registered".to_string()),
            version: None,
        },
    );
    capability_map.insert(
        "skillDraftSave".to_string(),
        HostCapability {
            state: CapabilityState::Available,
            reason: None,
            version: Some("1".to_string()),
        },
    );
    HostCapabilities {
        version: 1,
        platform: if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            "other"
        }
        .to_string(),
        // Finder selection is implemented below with a static AppleScript.
        finder_selection,
        // Do not expose a microphone until a native recognizer is registered.
        speech_recognition: false,
        skill_draft_save: true,
        capabilities: capability_map,
    }
}

fn normalize_selected_paths<I>(lines: I) -> Result<Vec<String>, String>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in lines {
        let candidate = raw.as_ref().trim().trim_end_matches('\r');
        if candidate.is_empty() {
            continue;
        }
        let path = PathBuf::from(candidate);
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("Finder selection path is unavailable: {error}"))?;
        let normalized = canonical.to_string_lossy().into_owned();
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    Ok(out)
}

#[cfg(target_os = "macos")]
pub fn finder_selected_paths() -> Result<Vec<String>, String> {
    use std::process::Command;

    // Script is static: no path or user text is interpolated into the shell.
    const SCRIPT: &str = r#"
tell application "Finder"
  set selectedItems to selection
end tell
set outputText to ""
repeat with selectedItem in selectedItems
  try
    set outputText to outputText & POSIX path of (selectedItem as alias) & linefeed
  end try
end repeat
return outputText
"#;

    let output = Command::new("/usr/bin/osascript")
        .args(["-e", SCRIPT])
        .output()
        .map_err(|error| format!("Unable to read Finder selection: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "Unable to read Finder selection".to_string()
        } else {
            detail
        });
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "Finder returned a non-UTF-8 path".to_string())?;
    normalize_selected_paths(stdout.lines())
}

#[cfg(not(target_os = "macos"))]
pub fn finder_selected_paths() -> Result<Vec<String>, String> {
    Err("Finder selection is only available on macOS".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sunsetz-host-features-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn capabilities_never_advertise_unimplemented_speech() {
        let caps = capabilities();
        assert_eq!(caps.version, 1);
        assert!(!caps.speech_recognition);
        assert!(caps.skill_draft_save);
        assert_eq!(
            caps.capabilities["speechRecognition"].state,
            CapabilityState::Unavailable
        );
    }

    #[test]
    fn finder_paths_are_canonical_and_deduplicated() {
        let root = temp_dir("finder");
        let file = root.join("a.txt");
        fs::write(&file, "a").unwrap();
        let raw = vec![
            format!("  {}  ", file.display()),
            file.display().to_string(),
            "".to_string(),
        ];
        let normalized = normalize_selected_paths(raw).unwrap();
        assert_eq!(normalized.len(), 1);
        assert_eq!(
            PathBuf::from(&normalized[0]),
            file.canonicalize().unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finder_paths_reject_missing_items() {
        let missing = std::env::temp_dir()
            .join(format!("sunsetz-missing-{}", uuid::Uuid::new_v4()));
        assert!(normalize_selected_paths([missing.to_string_lossy()]).is_err());
    }
}
