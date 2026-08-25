//! Bounded, read-only Git summaries for indexed project roots.
//!
//! A direct `.git` directory or bounded worktree gitfile is required before Git
//! is spawned, so a project nested inside another repository is never resolved
//! by searching upward. The command uses argument arrays, no shell, no file
//! content reads, and no stderr is returned or persisted.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PROJECT_GIT_SUMMARY_VERSION: u8 = 1;

const GIT_TIMEOUT: Duration = Duration::from_millis(600);
const CACHE_TTL: Duration = Duration::from_secs(2);
const MAX_CACHE_ENTRIES: usize = 128;
const MAX_GIT_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_GITFILE_BYTES: u64 = 4 * 1024;
const MAX_BRANCH_BYTES: usize = 256;
const MAX_DIRTY_FILES: u32 = 2_000;
const MAX_DIVERGENCE: u32 = 1_000_000;
const MAX_PROJECT_ID_BYTES: usize = 128;

const STATUS_ARGS: &[&str] = &[
    "status",
    "--porcelain=v2",
    "--branch",
    "--untracked-files=normal",
    "--no-renames",
    "-z",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectGitSummaryRequestV1 {
    pub version: u8,
    pub project_id: String,
    pub project_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectGitSummarySourceV1 {
    FilesystemMarker,
    GitStatusPorcelainV2,
    Cache,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectGitUnavailableReasonV1 {
    ProjectMissing,
    InvalidGitMetadata,
    GitUnavailable,
    Timeout,
    OutputLimit,
    GitFailed,
    ParseFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectGitSummaryV1 {
    pub version: u8,
    pub project_id: String,
    pub available: bool,
    pub is_repo: bool,
    pub branch: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub dirty: u32,
    pub conflicts: u32,
    pub counts_capped: bool,
    pub head: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub source: ProjectGitSummarySourceV1,
    pub unavailable_reason: Option<ProjectGitUnavailableReasonV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MetadataStamp {
    len: u64,
    modified_nanos: Option<u128>,
    kind: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RepoCacheKey {
    canonical_root: PathBuf,
    canonical_git_dir: PathBuf,
    git_marker: MetadataStamp,
    git_dir: MetadataStamp,
    head: Option<MetadataStamp>,
    index: Option<MetadataStamp>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    stored_at: Instant,
    summary: ProjectGitSummaryV1,
}

#[derive(Debug, Default)]
struct SummaryCache {
    entries: HashMap<RepoCacheKey, CacheEntry>,
}

impl SummaryCache {
    fn get(&mut self, key: &RepoCacheKey, now: Instant) -> Option<ProjectGitSummaryV1> {
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.stored_at) <= CACHE_TTL);
        let mut summary = self.entries.get(key)?.summary.clone();
        summary.source = ProjectGitSummarySourceV1::Cache;
        Some(summary)
    }

    fn insert(&mut self, key: RepoCacheKey, summary: ProjectGitSummaryV1, now: Instant) {
        self.entries.retain(|existing, _| {
            existing.canonical_root != key.canonical_root || existing == &key
        });
        self.entries.insert(
            key,
            CacheEntry {
                stored_at: now,
                summary,
            },
        );
        while self.entries.len() > MAX_CACHE_ENTRIES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.stored_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }
}

static SUMMARY_CACHE: OnceLock<Mutex<SummaryCache>> = OnceLock::new();

fn summary_cache() -> &'static Mutex<SummaryCache> {
    SUMMARY_CACHE.get_or_init(|| Mutex::new(SummaryCache::default()))
}

enum AuthorizedProject {
    Existing {
        project_id: String,
        canonical_root: PathBuf,
    },
    Missing {
        project_id: String,
    },
}

enum RepoDiscovery {
    NotRepository,
    Repository(RepoCacheKey),
    Unavailable(ProjectGitUnavailableReasonV1),
}

enum GitRunOutcome {
    Completed { success: bool, stdout: Vec<u8> },
    Unavailable(ProjectGitUnavailableReasonV1),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedStatus {
    branch: Option<String>,
    ahead: Option<u32>,
    behind: Option<u32>,
    dirty: u32,
    conflicts: u32,
    counts_capped: bool,
    head: Option<String>,
}

/// Resolve an indexed project and lazily return a bounded Git summary.
/// Authorization failures are hard errors; Git availability failures are DTOs.
pub fn project_git_summary_v1(
    request: ProjectGitSummaryRequestV1,
) -> Result<ProjectGitSummaryV1, String> {
    summarize_at(
        &crate::store::load_projects(),
        request,
        summary_cache(),
        Instant::now(),
        Utc::now(),
        &run_git_status,
    )
}

fn summarize_at(
    projects: &[crate::store::Project],
    request: ProjectGitSummaryRequestV1,
    cache: &Mutex<SummaryCache>,
    cache_now: Instant,
    observed_at: DateTime<Utc>,
    runner: &impl Fn(&Path, &[&str], Duration, usize) -> GitRunOutcome,
) -> Result<ProjectGitSummaryV1, String> {
    require_version(request.version)?;
    let authorized = authorize_indexed_project(projects, &request)?;
    let (project_id, canonical_root) = match authorized {
        AuthorizedProject::Existing {
            project_id,
            canonical_root,
        } => (project_id, canonical_root),
        AuthorizedProject::Missing { project_id } => {
            return Ok(unavailable_summary(
                project_id,
                false,
                ProjectGitUnavailableReasonV1::ProjectMissing,
                observed_at,
            ));
        }
    };

    let cache_key = match discover_direct_repository(&canonical_root) {
        RepoDiscovery::NotRepository => {
            return Ok(ProjectGitSummaryV1 {
                version: PROJECT_GIT_SUMMARY_VERSION,
                project_id,
                available: true,
                is_repo: false,
                branch: None,
                ahead: None,
                behind: None,
                dirty: 0,
                conflicts: 0,
                counts_capped: false,
                head: None,
                observed_at,
                source: ProjectGitSummarySourceV1::FilesystemMarker,
                unavailable_reason: None,
            });
        }
        RepoDiscovery::Unavailable(reason) => {
            return Ok(unavailable_summary(project_id, true, reason, observed_at));
        }
        RepoDiscovery::Repository(key) => key,
    };

    if let Some(cached) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&cache_key, cache_now)
    {
        return Ok(cached);
    }

    let summary = match runner(
        &canonical_root,
        STATUS_ARGS,
        GIT_TIMEOUT,
        MAX_GIT_OUTPUT_BYTES,
    ) {
        GitRunOutcome::Unavailable(reason) => {
            unavailable_summary(project_id, true, reason, observed_at)
        }
        GitRunOutcome::Completed { success: false, .. } => unavailable_summary(
            project_id,
            true,
            ProjectGitUnavailableReasonV1::GitFailed,
            observed_at,
        ),
        GitRunOutcome::Completed {
            success: true,
            stdout,
        } if stdout.len() > MAX_GIT_OUTPUT_BYTES => unavailable_summary(
            project_id,
            true,
            ProjectGitUnavailableReasonV1::OutputLimit,
            observed_at,
        ),
        GitRunOutcome::Completed {
            success: true,
            stdout,
        } => match parse_porcelain_v2(&stdout) {
            Ok(parsed) => ProjectGitSummaryV1 {
                version: PROJECT_GIT_SUMMARY_VERSION,
                project_id,
                available: true,
                is_repo: true,
                branch: parsed.branch,
                ahead: parsed.ahead,
                behind: parsed.behind,
                dirty: parsed.dirty,
                conflicts: parsed.conflicts,
                counts_capped: parsed.counts_capped,
                head: parsed.head,
                observed_at,
                source: ProjectGitSummarySourceV1::GitStatusPorcelainV2,
                unavailable_reason: None,
            },
            Err(()) => unavailable_summary(
                project_id,
                true,
                ProjectGitUnavailableReasonV1::ParseFailed,
                observed_at,
            ),
        },
    };

    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(cache_key, summary.clone(), cache_now);
    Ok(summary)
}

fn authorize_indexed_project(
    projects: &[crate::store::Project],
    request: &ProjectGitSummaryRequestV1,
) -> Result<AuthorizedProject, String> {
    let project_id = request.project_id.trim();
    if project_id.is_empty()
        || project_id.len() > MAX_PROJECT_ID_BYTES
        || project_id.chars().any(char::is_control)
    {
        return Err("PROJECT_GIT_DENIED: invalid project id".into());
    }
    let matching = projects
        .iter()
        .filter(|project| project.id == project_id)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err("PROJECT_GIT_DENIED: project is not uniquely indexed".into());
    }
    let project = matching[0];
    let requested_raw = request.project_path.trim();
    if requested_raw.is_empty()
        || requested_raw.contains('\0')
        || requested_raw.chars().any(char::is_control)
    {
        return Err("PROJECT_GIT_DENIED: invalid project path".into());
    }
    let requested = PathBuf::from(requested_raw);
    if !requested.is_absolute()
        || requested
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err("PROJECT_GIT_DENIED: project path must be an exact absolute root".into());
    }
    let indexed = PathBuf::from(project.path.trim());
    let canonical_indexed = indexed.canonicalize();
    let canonical_requested = requested.canonicalize();
    match (canonical_indexed, canonical_requested) {
        (Ok(indexed), Ok(requested)) if indexed == requested && requested.is_dir() => {
            Ok(AuthorizedProject::Existing {
                project_id: project_id.to_string(),
                canonical_root: requested,
            })
        }
        (Err(_), Err(_)) if PathBuf::from(project.path.trim()) == requested => {
            Ok(AuthorizedProject::Missing {
                project_id: project_id.to_string(),
            })
        }
        (Ok(indexed), Err(_))
            if PathBuf::from(project.path.trim()) == requested && !indexed.exists() =>
        {
            Ok(AuthorizedProject::Missing {
                project_id: project_id.to_string(),
            })
        }
        _ => Err("PROJECT_GIT_DENIED: path is not the indexed canonical project root".into()),
    }
}

fn discover_direct_repository(root: &Path) -> RepoDiscovery {
    let marker = root.join(".git");
    let marker_metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RepoDiscovery::NotRepository;
        }
        Err(_) => {
            return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
        }
    };
    if marker_metadata.file_type().is_symlink() {
        return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
    }
    let git_dir = if marker_metadata.is_dir() {
        match marker.canonicalize() {
            Ok(path) => path,
            Err(_) => {
                return RepoDiscovery::Unavailable(
                    ProjectGitUnavailableReasonV1::InvalidGitMetadata,
                );
            }
        }
    } else if marker_metadata.is_file() {
        match resolve_gitfile(&marker, &marker_metadata) {
            Some(path) => path,
            None => {
                return RepoDiscovery::Unavailable(
                    ProjectGitUnavailableReasonV1::InvalidGitMetadata,
                );
            }
        }
    } else {
        return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
    };
    let Some(marker_stamp) = metadata_stamp(&marker_metadata) else {
        return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
    };
    let git_dir_metadata = match fs::metadata(&git_dir) {
        Ok(metadata) if metadata.is_dir() => metadata,
        _ => {
            return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
        }
    };
    let Some(git_dir_stamp) = metadata_stamp(&git_dir_metadata) else {
        return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
    };
    let head = match optional_path_stamp(&git_dir.join("HEAD")) {
        Ok(stamp) => stamp,
        Err(()) => {
            return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
        }
    };
    let index = match optional_path_stamp(&git_dir.join("index")) {
        Ok(stamp) => stamp,
        Err(()) => {
            return RepoDiscovery::Unavailable(ProjectGitUnavailableReasonV1::InvalidGitMetadata);
        }
    };
    RepoDiscovery::Repository(RepoCacheKey {
        canonical_root: root.to_path_buf(),
        canonical_git_dir: git_dir,
        git_marker: marker_stamp,
        git_dir: git_dir_stamp,
        head,
        index,
    })
}

fn resolve_gitfile(marker: &Path, metadata: &fs::Metadata) -> Option<PathBuf> {
    if metadata.len() == 0 || metadata.len() > MAX_GITFILE_BYTES {
        return None;
    }
    let mut file = fs::File::open(marker).ok()?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_GITFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_GITFILE_BYTES {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let line = text.trim_end_matches(['\r', '\n']);
    if line.contains(['\r', '\n', '\0']) {
        return None;
    }
    let raw = line.strip_prefix("gitdir:")?.trim();
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return None;
    }
    let pointed = PathBuf::from(raw);
    let candidate = if pointed.is_absolute() {
        pointed
    } else {
        marker.parent()?.join(pointed)
    };
    let canonical = candidate.canonicalize().ok()?;
    canonical.is_dir().then_some(canonical)
}

fn metadata_stamp(metadata: &fs::Metadata) -> Option<MetadataStamp> {
    let kind = if metadata.is_dir() {
        1
    } else if metadata.is_file() {
        2
    } else {
        return None;
    };
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    Some(MetadataStamp {
        len: metadata.len(),
        modified_nanos,
        kind,
    })
}

fn optional_path_stamp(path: &Path) -> Result<Option<MetadataStamp>, ()> {
    match fs::metadata(path) {
        Ok(metadata) => metadata_stamp(&metadata).map(Some).ok_or(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

fn run_git_status(
    root: &Path,
    args: &[&str],
    timeout: Duration,
    output_limit: usize,
) -> GitRunOutcome {
    let mut command = build_git_command(root, args);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::GitUnavailable);
        }
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::GitFailed);
    };
    let exceeded = Arc::new(AtomicBool::new(false));
    let reader_exceeded = Arc::clone(&exceeded);
    let (reader_tx, reader_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::with_capacity(output_limit.min(8 * 1024));
        let mut buffer = [0_u8; 8 * 1024];
        let result = loop {
            let read = match stdout.read(&mut buffer) {
                Ok(read) => read,
                Err(_) => break Err(()),
            };
            if read == 0 {
                break Ok(output);
            }
            let remaining = output_limit.saturating_sub(output.len());
            if read > remaining {
                output.extend_from_slice(&buffer[..remaining]);
                reader_exceeded.store(true, Ordering::Release);
                break Ok(output);
            }
            output.extend_from_slice(&buffer[..read]);
        };
        let _ = reader_tx.send(result);
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        if exceeded.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::OutputLimit);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::Timeout);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::GitFailed);
            }
        }
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    let stdout = match reader_rx.recv_timeout(remaining) {
        Ok(Ok(output)) => output,
        Ok(Err(())) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::GitFailed);
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::Timeout);
        }
    };
    if exceeded.load(Ordering::Acquire) {
        return GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::OutputLimit);
    }
    GitRunOutcome::Completed {
        success: status.success(),
        stdout,
    }
}

fn build_git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(["-c", "core.fsmonitor=false"])
        .args(["-c", "core.untrackedCache=false"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    crate::process_util::apply_no_window_std(&mut command);
    command
}

fn parse_porcelain_v2(raw: &[u8]) -> Result<ParsedStatus, ()> {
    let mut branch = None;
    let mut ahead = None;
    let mut behind = None;
    let mut head = None;
    let mut dirty = 0_u32;
    let mut conflicts = 0_u32;
    let mut counts_capped = false;
    let mut saw_oid = false;

    for field in raw
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
    {
        if let Some(value) = field.strip_prefix(b"# branch.oid ") {
            saw_oid = true;
            if value != b"(initial)" {
                if !matches!(value.len(), 40 | 64) || !value.iter().all(u8::is_ascii_hexdigit) {
                    return Err(());
                }
                head = Some(String::from_utf8_lossy(&value[..12]).to_ascii_lowercase());
            }
            continue;
        }
        if let Some(value) = field.strip_prefix(b"# branch.head ") {
            if value != b"(detached)" {
                if value.is_empty() || value.len() > MAX_BRANCH_BYTES {
                    return Err(());
                }
                let value = std::str::from_utf8(value).map_err(|_| ())?;
                if value.chars().any(char::is_control) {
                    return Err(());
                }
                branch = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = field.strip_prefix(b"# branch.ab ") {
            let text = std::str::from_utf8(value).map_err(|_| ())?;
            let mut parts = text.split_whitespace();
            let raw_ahead = parts.next().and_then(|value| value.strip_prefix('+'));
            let raw_behind = parts.next().and_then(|value| value.strip_prefix('-'));
            if parts.next().is_some() || raw_ahead.is_none() || raw_behind.is_none() {
                return Err(());
            }
            let parsed_ahead = raw_ahead.unwrap().parse::<u64>().map_err(|_| ())?;
            let parsed_behind = raw_behind.unwrap().parse::<u64>().map_err(|_| ())?;
            if parsed_ahead > u64::from(MAX_DIVERGENCE) || parsed_behind > u64::from(MAX_DIVERGENCE)
            {
                counts_capped = true;
            }
            ahead = Some(parsed_ahead.min(u64::from(MAX_DIVERGENCE)) as u32);
            behind = Some(parsed_behind.min(u64::from(MAX_DIVERGENCE)) as u32);
            continue;
        }
        let status_kind = field.first().copied();
        if matches!(status_kind, Some(b'1' | b'2' | b'u' | b'?')) && field.get(1) == Some(&b' ') {
            if dirty < MAX_DIRTY_FILES {
                dirty += 1;
            } else {
                counts_capped = true;
            }
            if status_kind == Some(b'u') {
                if conflicts < MAX_DIRTY_FILES {
                    conflicts += 1;
                } else {
                    counts_capped = true;
                }
            }
        }
    }
    if !saw_oid {
        return Err(());
    }
    Ok(ParsedStatus {
        branch,
        ahead,
        behind,
        dirty,
        conflicts,
        counts_capped,
        head,
    })
}

fn unavailable_summary(
    project_id: String,
    is_repo: bool,
    reason: ProjectGitUnavailableReasonV1,
    observed_at: DateTime<Utc>,
) -> ProjectGitSummaryV1 {
    ProjectGitSummaryV1 {
        version: PROJECT_GIT_SUMMARY_VERSION,
        project_id,
        available: false,
        is_repo,
        branch: None,
        ahead: None,
        behind: None,
        dirty: 0,
        conflicts: 0,
        counts_capped: false,
        head: None,
        observed_at,
        source: ProjectGitSummarySourceV1::Unavailable,
        unavailable_reason: Some(reason),
    }
}

fn require_version(version: u8) -> Result<(), String> {
    if version == PROJECT_GIT_SUMMARY_VERSION {
        Ok(())
    } else {
        Err("PROJECT_GIT_UNSUPPORTED_VERSION: expected schema v1".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use uuid::Uuid;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("sunsetz-project-git-{label}-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-24T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn project(id: &str, path: &Path, trusted: bool) -> crate::store::Project {
        crate::store::Project {
            id: id.into(),
            name: "Project".into(),
            path: path.to_string_lossy().into_owned(),
            trusted,
            last_opened_at: fixed_now(),
            path_ok: path.is_dir(),
            pinned: false,
            model_id: None,
            effort: None,
            mode: None,
            permission_policy: None,
        }
    }

    fn request(id: &str, path: &Path) -> ProjectGitSummaryRequestV1 {
        ProjectGitSummaryRequestV1 {
            version: PROJECT_GIT_SUMMARY_VERSION,
            project_id: id.into(),
            project_path: path.to_string_lossy().into_owned(),
        }
    }

    fn init_git_metadata(root: &Path) -> PathBuf {
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        fs::write(git_dir.join("index"), b"index-v1").unwrap();
        git_dir
    }

    fn status_output(branch: &str, oid: char) -> Vec<u8> {
        format!(
            "# branch.oid {}\0# branch.head {branch}\0# branch.upstream origin/{branch}\0# branch.ab +3 -2\0",
            oid.to_string().repeat(40)
        )
        .into_bytes()
    }

    fn success_runner(
        _root: &Path,
        _args: &[&str],
        _timeout: Duration,
        _limit: usize,
    ) -> GitRunOutcome {
        GitRunOutcome::Completed {
            success: true,
            stdout: status_output("main", 'a'),
        }
    }

    #[test]
    fn strict_request_and_index_authorization_reject_outside_and_traversal_paths() {
        let indexed = TestDir::new("indexed");
        let outside = TestDir::new("outside");
        let child = indexed.path.join("child");
        fs::create_dir_all(&child).unwrap();
        let projects = vec![project("project-1", &indexed.path, false)];
        let cache = Mutex::new(SummaryCache::default());
        let now = Instant::now();

        let outside_error = summarize_at(
            &projects,
            request("project-1", &outside.path),
            &cache,
            now,
            fixed_now(),
            &success_runner,
        )
        .unwrap_err();
        assert!(outside_error.contains("PROJECT_GIT_DENIED"));

        let traversal = child.join("..");
        let traversal_error = summarize_at(
            &projects,
            request("project-1", &traversal),
            &cache,
            now,
            fixed_now(),
            &success_runner,
        )
        .unwrap_err();
        assert!(traversal_error.contains("exact absolute root"));

        let unknown_error = summarize_at(
            &projects,
            request("not-indexed", &indexed.path),
            &cache,
            now,
            fixed_now(),
            &success_runner,
        )
        .unwrap_err();
        assert!(unknown_error.contains("not uniquely indexed"));

        let unknown_field = serde_json::json!({
            "version": 1,
            "projectId": "project-1",
            "projectPath": indexed.path.to_string_lossy(),
            "searchParents": true
        });
        assert!(serde_json::from_value::<ProjectGitSummaryRequestV1>(unknown_field).is_err());
    }

    #[test]
    fn indexed_non_repo_is_known_without_running_git_or_requiring_trust() {
        let root = TestDir::new("non-repo");
        let projects = vec![project("project-1", &root.path, false)];
        let cache = Mutex::new(SummaryCache::default());
        let summary = summarize_at(
            &projects,
            request("project-1", &root.path),
            &cache,
            Instant::now(),
            fixed_now(),
            &|_, _, _, _| panic!("Git must not search a parent repository"),
        )
        .unwrap();
        assert!(summary.available);
        assert!(!summary.is_repo);
        assert_eq!(summary.source, ProjectGitSummarySourceV1::FilesystemMarker);
        assert!(summary.unavailable_reason.is_none());
    }

    #[test]
    fn injected_timeout_and_output_limit_are_explicitly_unavailable() {
        let root = TestDir::new("unavailable");
        init_git_metadata(&root.path);
        let projects = vec![project("project-1", &root.path, true)];
        let timeout = summarize_at(
            &projects,
            request("project-1", &root.path),
            &Mutex::new(SummaryCache::default()),
            Instant::now(),
            fixed_now(),
            &|_, _, _, _| GitRunOutcome::Unavailable(ProjectGitUnavailableReasonV1::Timeout),
        )
        .unwrap();
        assert!(!timeout.available);
        assert!(timeout.is_repo);
        assert_eq!(
            timeout.unavailable_reason,
            Some(ProjectGitUnavailableReasonV1::Timeout)
        );

        let output_limit = summarize_at(
            &projects,
            request("project-1", &root.path),
            &Mutex::new(SummaryCache::default()),
            Instant::now(),
            fixed_now(),
            &|_, _, _, _| GitRunOutcome::Completed {
                success: true,
                stdout: vec![b'x'; MAX_GIT_OUTPUT_BYTES + 1],
            },
        )
        .unwrap();
        assert!(!output_limit.available);
        assert_eq!(
            output_limit.unavailable_reason,
            Some(ProjectGitUnavailableReasonV1::OutputLimit)
        );
    }

    #[test]
    fn porcelain_v2_parse_is_deterministic_and_counts_are_capped() {
        let mut raw = status_output("feature/cache", 'b');
        raw.extend_from_slice(b"1 .M N... metadata only\0");
        raw.extend_from_slice(b"u UU N... conflict\0");
        raw.extend_from_slice(b"? untracked\0");
        let first = parse_porcelain_v2(&raw).unwrap();
        let second = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.branch.as_deref(), Some("feature/cache"));
        assert_eq!(first.ahead, Some(3));
        assert_eq!(first.behind, Some(2));
        assert_eq!(first.dirty, 3);
        assert_eq!(first.conflicts, 1);
        assert_eq!(first.head.as_deref(), Some("bbbbbbbbbbbb"));
        assert!(!first.counts_capped);

        let mut capped = status_output("main", 'c');
        for _ in 0..MAX_DIRTY_FILES + 7 {
            capped.extend_from_slice(b"? path\0");
        }
        let capped = parse_porcelain_v2(&capped).unwrap();
        assert_eq!(capped.dirty, MAX_DIRTY_FILES);
        assert!(capped.counts_capped);
    }

    #[test]
    fn worktree_gitfile_is_bounded_and_resolves_explicit_git_dir() {
        let root = TestDir::new("worktree-root");
        let control = TestDir::new("worktree-control");
        fs::write(control.path.join("HEAD"), b"ref: refs/heads/worktree\n").unwrap();
        fs::write(control.path.join("index"), b"worktree-index").unwrap();
        fs::write(
            root.path.join(".git"),
            format!("gitdir: {}\n", control.path.display()),
        )
        .unwrap();

        let discovery = discover_direct_repository(&root.path);
        let RepoDiscovery::Repository(key) = discovery else {
            panic!("valid worktree gitfile must resolve");
        };
        assert_eq!(key.canonical_git_dir, control.path.canonicalize().unwrap());

        let projects = vec![project("project-1", &root.path, true)];
        let summary = summarize_at(
            &projects,
            request("project-1", &root.path),
            &Mutex::new(SummaryCache::default()),
            Instant::now(),
            fixed_now(),
            &|seen_root, args, timeout, limit| {
                assert_eq!(seen_root, root.path.canonicalize().unwrap());
                assert_eq!(args, STATUS_ARGS);
                assert_eq!(timeout, GIT_TIMEOUT);
                assert_eq!(limit, MAX_GIT_OUTPUT_BYTES);
                GitRunOutcome::Completed {
                    success: true,
                    stdout: status_output("worktree", 'd'),
                }
            },
        )
        .unwrap();
        assert!(summary.available);
        assert!(summary.is_repo);
        assert_eq!(summary.branch.as_deref(), Some("worktree"));

        let command = build_git_command(&root.path, STATUS_ARGS);
        assert_eq!(command.get_program(), "git");
        let args = command.get_args().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(args[0], "-C");
        assert_eq!(args[1], root.path.as_os_str());
        assert!(args.iter().any(|arg| arg == "status"));
    }

    #[test]
    fn cache_invalidates_on_index_change_ttl_and_project_move() {
        let first_root = TestDir::new("cache-first");
        let first_git_dir = init_git_metadata(&first_root.path);
        let projects = vec![project("project-1", &first_root.path, true)];
        let cache = Mutex::new(SummaryCache::default());
        let calls = AtomicUsize::new(0);
        let runner = |_: &Path, _: &[&str], _: Duration, _: usize| {
            calls.fetch_add(1, AtomicOrdering::SeqCst);
            GitRunOutcome::Completed {
                success: true,
                stdout: status_output("main", 'e'),
            }
        };
        let cache_now = Instant::now();
        let first = summarize_at(
            &projects,
            request("project-1", &first_root.path),
            &cache,
            cache_now,
            fixed_now(),
            &runner,
        )
        .unwrap();
        assert_eq!(
            first.source,
            ProjectGitSummarySourceV1::GitStatusPorcelainV2
        );
        let cached = summarize_at(
            &projects,
            request("project-1", &first_root.path),
            &cache,
            cache_now + Duration::from_millis(500),
            fixed_now() + ChronoDuration::milliseconds(500),
            &runner,
        )
        .unwrap();
        assert_eq!(cached.source, ProjectGitSummarySourceV1::Cache);
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);

        fs::write(first_git_dir.join("index"), b"index-v2-with-new-length").unwrap();
        let after_index = summarize_at(
            &projects,
            request("project-1", &first_root.path),
            &cache,
            cache_now + Duration::from_millis(700),
            fixed_now() + ChronoDuration::milliseconds(700),
            &runner,
        )
        .unwrap();
        assert_eq!(
            after_index.source,
            ProjectGitSummarySourceV1::GitStatusPorcelainV2
        );
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);

        let after_ttl = summarize_at(
            &projects,
            request("project-1", &first_root.path),
            &cache,
            cache_now + CACHE_TTL + Duration::from_millis(701),
            fixed_now() + ChronoDuration::seconds(3),
            &runner,
        )
        .unwrap();
        assert_eq!(
            after_ttl.source,
            ProjectGitSummarySourceV1::GitStatusPorcelainV2
        );
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 3);

        let moved_root = TestDir::new("cache-moved");
        init_git_metadata(&moved_root.path);
        let moved_projects = vec![project("project-1", &moved_root.path, true)];
        let moved = summarize_at(
            &moved_projects,
            request("project-1", &moved_root.path),
            &cache,
            cache_now + CACHE_TTL + Duration::from_millis(702),
            fixed_now() + ChronoDuration::seconds(3),
            &runner,
        )
        .unwrap();
        assert_eq!(
            moved.source,
            ProjectGitSummarySourceV1::GitStatusPorcelainV2
        );
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 4);
    }
}
