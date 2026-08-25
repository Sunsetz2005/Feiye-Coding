//! Versioned Memory export and privacy-clear contracts.
//!
//! Memory candidate JSON remains the only long-term fact source. The SQLite
//! session-search database is deliberately excluded: it is a rebuildable cache
//! of visible journals, not Memory. Exports contain approved candidates with
//! live user-message provenance plus bounded injection audit metadata. They
//! never contain recall queries, generated prompt fragments, FTS snippets, or
//! raw journal text.
//!
//! Clears are two-step operations. A read-only preview returns a deterministic
//! plan and hash. Confirmation re-locks every affected JSON store, rebuilds the
//! plan, and rejects any candidate hash, ledger revision, or scope drift before
//! atomically replacing individual stores. A best-effort rollback protects the
//! multi-file operation from ordinary write failures.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::memory_candidates::{
    MemoryCandidateOwnershipV1, MemoryCandidateStatusV1, MemoryCandidateTypeV1, MemoryCandidateV1,
};
use crate::memory_injection::{
    MemoryInjectionFailureCodeV1, MemoryInjectionFeedbackV1, MemoryInjectionRecordV1,
    MemoryInjectionStatusV1,
};

pub const MEMORY_PORTABILITY_VERSION: u8 = 1;
pub const MEMORY_PORTABILITY_SCHEMA: &str = "sunsetz.memory-portability.v1";
pub const MEMORY_CLEAR_SCHEMA: &str = "sunsetz.memory-clear.v1";
pub const MAX_MEMORY_EXPORT_CANDIDATES: usize = 256;
pub const MAX_MEMORY_EXPORT_INJECTIONS: usize = 2_048;
pub const MAX_MEMORY_PORTABILITY_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_MEMORY_CLEAR_SCOPES: usize = 256;

const MEMORY_CANDIDATE_STORE_VERSION: u8 = 1;
const MEMORY_INJECTION_LEDGER_VERSION: u8 = 1;
const MAX_MEMORY_LEDGER_FILES: usize = 1_024;
const MAX_MEMORY_LEDGER_ENTRIES: usize = 128;
const MAX_MEMORY_CONTENT_CHARS: usize = 2_000;
const MAX_INJECTION_SELECTIONS: usize = 8;
const MAX_SOURCE_ID_CHARS: usize = 256;
const HASH_HEX_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPortableCategoryV1 {
    UserPreference,
    ProjectFact,
    WorkflowHint,
}

impl From<MemoryCandidateTypeV1> for MemoryPortableCategoryV1 {
    fn from(value: MemoryCandidateTypeV1) -> Self {
        match value {
            MemoryCandidateTypeV1::UserPreference => Self::UserPreference,
            MemoryCandidateTypeV1::ProjectFact => Self::ProjectFact,
            MemoryCandidateTypeV1::WorkflowHint => Self::WorkflowHint,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPortableProvenanceKindV1 {
    PersistedUserMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPortableProvenanceV1 {
    pub kind: MemoryPortableProvenanceKindV1,
    pub session_id: String,
    pub message_id: String,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPortableCandidateV1 {
    pub candidate_id: String,
    pub content_hash: String,
    pub category: MemoryPortableCategoryV1,
    pub content: String,
    pub provenance: MemoryPortableProvenanceV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPortableInjectionSelectionV1 {
    pub candidate_id: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPortableInjectionAuditV1 {
    pub session_id: String,
    pub injection_id: String,
    pub context_hash: String,
    pub status: MemoryInjectionStatusV1,
    pub revision: u64,
    pub attempt: u32,
    pub failure_code: Option<MemoryInjectionFailureCodeV1>,
    pub feedback: Option<MemoryInjectionFeedbackV1>,
    pub selections: Vec<MemoryPortableInjectionSelectionV1>,
    pub created_at: DateTime<Utc>,
    pub prepared_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub applied_at: Option<DateTime<Utc>>,
    pub removed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryExportRequestV1 {
    pub version: u8,
    pub include_injection_audit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPortableExportV1 {
    pub schema: String,
    pub version: u8,
    pub content_hash: String,
    pub candidates: Vec<MemoryPortableCandidateV1>,
    pub injection_audit: Vec<MemoryPortableInjectionAuditV1>,
    pub excludes_session_search_index: bool,
    pub excludes_queries_and_prompt_fragments: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryExportHashDomainV1<'a> {
    schema: &'a str,
    version: u8,
    candidates: &'a [MemoryPortableCandidateV1],
    injection_audit: &'a [MemoryPortableInjectionAuditV1],
    excludes_session_search_index: bool,
    excludes_queries_and_prompt_fragments: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearCandidateSelectionV1 {
    pub candidate_id: String,
    pub expected_content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearScopeV1 {
    #[serde(default)]
    pub candidate_selections: Vec<MemoryClearCandidateSelectionV1>,
    #[serde(default)]
    pub source_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearPreviewRequestV1 {
    pub version: u8,
    pub scope: MemoryClearScopeV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryClearSkippedKindV1 {
    Candidate,
    SourceSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryClearSkippedReasonV1 {
    CandidateNotFound,
    NoCandidatesForProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearSkippedV1 {
    pub kind: MemoryClearSkippedKindV1,
    pub id: String,
    pub reason: MemoryClearSkippedReasonV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearCandidatePlanV1 {
    pub candidate_id: String,
    pub expected_content_hash: String,
    pub category: MemoryPortableCategoryV1,
    pub provenance: MemoryPortableProvenanceV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearInjectionPlanV1 {
    pub session_id: String,
    pub injection_id: String,
    pub expected_context_hash: String,
    pub expected_revision: u64,
    pub matched_candidate_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearPlanV1 {
    pub schema: String,
    pub version: u8,
    pub dry_run: bool,
    pub requires_confirmation: bool,
    pub candidate_store_hash: String,
    pub sessions_index_hash: String,
    pub scope: MemoryClearScopeV1,
    pub candidates: Vec<MemoryClearCandidatePlanV1>,
    pub injections: Vec<MemoryClearInjectionPlanV1>,
    pub skipped: Vec<MemoryClearSkippedV1>,
    pub session_search_index_is_rebuildable_cache: bool,
    pub plan_hash: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryClearPlanHashDomainV1<'a> {
    schema: &'a str,
    version: u8,
    dry_run: bool,
    requires_confirmation: bool,
    candidate_store_hash: &'a str,
    sessions_index_hash: &'a str,
    scope: &'a MemoryClearScopeV1,
    candidates: &'a [MemoryClearCandidatePlanV1],
    injections: &'a [MemoryClearInjectionPlanV1],
    skipped: &'a [MemoryClearSkippedV1],
    session_search_index_is_rebuildable_cache: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearConfirmRequestV1 {
    pub version: u8,
    pub plan: MemoryClearPlanV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearedCandidateV1 {
    pub candidate_id: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearedInjectionV1 {
    pub session_id: String,
    pub injection_id: String,
    pub context_hash: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryClearResultV1 {
    pub version: u8,
    pub plan_hash: String,
    pub before_candidate_store_hash: String,
    pub after_candidate_store_hash: String,
    pub deleted_candidates: Vec<MemoryClearedCandidateV1>,
    pub deleted_injections: Vec<MemoryClearedInjectionV1>,
    pub skipped: Vec<MemoryClearSkippedV1>,
    pub session_search_index_mutated: bool,
    pub session_search_index_is_rebuildable_cache: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryCandidateStoreSnapshotV1 {
    version: u8,
    candidates: Vec<MemoryCandidateV1>,
}

impl Default for MemoryCandidateStoreSnapshotV1 {
    fn default() -> Self {
        Self {
            version: MEMORY_CANDIDATE_STORE_VERSION,
            candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryInjectionLedgerSnapshotV1 {
    version: u8,
    entries: Vec<MemoryInjectionRecordV1>,
}

impl Default for MemoryInjectionLedgerSnapshotV1 {
    fn default() -> Self {
        Self {
            version: MEMORY_INJECTION_LEDGER_VERSION,
            entries: Vec::new(),
        }
    }
}

struct LockedMemorySnapshot {
    _locks: Vec<crate::store_lock::ExclusiveLock>,
    candidate_store_path: PathBuf,
    candidate_store: MemoryCandidateStoreSnapshotV1,
    sessions: Vec<crate::store::SessionMeta>,
    messages: HashMap<String, Vec<crate::store::ChatMessageStored>>,
    ledgers: BTreeMap<String, (PathBuf, MemoryInjectionLedgerSnapshotV1)>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn canonical_hash<T: Serialize>(value: &T, label: &str) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| format!("serialize {label}: {error}"))?;
    Ok(sha256_hex(&bytes))
}

fn ensure_serialized_bound<T: Serialize>(
    value: &T,
    limit: usize,
    label: &str,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| format!("serialize {label}: {error}"))?;
    if bytes.len() > limit {
        return Err(format!("{label} exceeds {limit} bytes"));
    }
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    value.len() == HASH_HEX_CHARS && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_version(version: u8) -> Result<(), String> {
    if version != MEMORY_PORTABILITY_VERSION {
        return Err(format!("unsupported memory portability version: {version}"));
    }
    Ok(())
}

fn validate_source_id(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > MAX_SOURCE_ID_CHARS
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("invalid memory portability {label}"));
    }
    Ok(())
}

fn candidate_store_path(root: &Path) -> PathBuf {
    root.join("memory-candidates.v1.json")
}

fn sessions_index_path(root: &Path) -> PathBuf {
    root.join("sessions_index.json")
}

fn messages_path(root: &Path, session_id: &str) -> PathBuf {
    root.join("sessions").join(session_id).join("messages.json")
}

fn validate_session_directory_boundary(root: &Path, session_id: &str) -> Result<(), String> {
    validate_source_id("session path id", session_id)?;
    let sessions_root = root.join("sessions");
    if fs::symlink_metadata(&sessions_root).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("memory portability sessions root may not be a symlink".into());
    }
    let session_directory = sessions_root.join(session_id);
    if fs::symlink_metadata(&session_directory)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("memory portability session directory may not be a symlink".into());
    }
    Ok(())
}

fn ledger_path(root: &Path, session_id: &str) -> PathBuf {
    root.join("sessions")
        .join(session_id)
        .join("memory-injections.v1.json")
}

fn read_json_or_default<T: for<'de> Deserialize<'de>>(
    path: &Path,
    default: impl FnOnce() -> T,
) -> Result<T, String> {
    match fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(default()),
        Ok(raw) => {
            serde_json::from_str(&raw).map_err(|error| format!("parse {}: {error}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default()),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

fn read_sessions_index(path: &Path) -> Result<Vec<crate::store::SessionMeta>, String> {
    read_json_or_default(path, Vec::new)
}

fn read_messages(path: &Path) -> Result<Vec<crate::store::ChatMessageStored>, String> {
    read_json_or_default(path, Vec::new)
}

fn suspicious_secret(text: &str) -> Option<&'static str> {
    const DIRECT_PATTERNS: &[(&str, &str)] = &[
        ("-----begin private key-----", "private key"),
        ("-----begin rsa private key-----", "private key"),
        ("-----begin ec private key-----", "private key"),
        ("-----begin openssh private key-----", "private key"),
        ("-----begin pgp private key block-----", "private key"),
        ("authorization: bearer ", "bearer token"),
        ("xoxb-", "Slack token"),
        ("xoxp-", "Slack token"),
        ("ghp_", "GitHub token"),
        ("github_pat_", "GitHub token"),
        ("glpat-", "GitLab token"),
        ("sk-proj-", "API key"),
        ("sk-ant-", "API key"),
        ("xai-", "API key"),
    ];
    let lower = text.to_ascii_lowercase();
    if lower.contains("-----begin ") && lower.contains("private key-----") {
        return Some("private key");
    }
    for (pattern, label) in DIRECT_PATTERNS {
        if lower.contains(pattern) {
            return Some(label);
        }
    }

    for scheme in [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "rediss://",
    ] {
        for (start, _) in lower.match_indices(scheme) {
            let rest = &text[start + scheme.len()..];
            let authority = rest
                .split(|character: char| {
                    character.is_whitespace() || matches!(character, '/' | '?' | '#')
                })
                .next()
                .unwrap_or_default();
            if authority.rsplit_once('@').is_some_and(|(userinfo, _)| {
                userinfo
                    .split_once(':')
                    .is_some_and(|(_, password)| !password.is_empty())
            }) {
                return Some("credentialed database URL");
            }
        }
    }

    for token in text.split_whitespace() {
        let token = token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_' | '.')
        });
        let segments = token.split('.').collect::<Vec<_>>();
        if segments.len() == 3
            && segments[0].starts_with("eyJ")
            && segments.iter().all(|segment| {
                segment.len() >= 8
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        {
            return Some("JWT");
        }
    }

    for line in lower.lines() {
        let compact: String = line
            .chars()
            .filter(|character| {
                !character.is_whitespace() && !matches!(character, '"' | '\'' | '`')
            })
            .collect();
        if compact.contains("authorization:bearer") {
            return Some("bearer token");
        }
        for key in [
            "api_key",
            "apikey",
            "api-token",
            "access_token",
            "refresh_token",
            "auth_token",
            "token",
            "client_secret",
            "secret_key",
            "secret",
            "private_key",
            "password",
        ] {
            for separator in ['=', ':'] {
                let pattern = format!("{key}{separator}");
                for (start, _) in compact.match_indices(&pattern) {
                    let value = compact[start + pattern.len()..]
                        .split([',', '}', ';'])
                        .next()
                        .unwrap_or_default();
                    if value.len() >= 8
                        && !value.starts_with('<')
                        && !value.starts_with("${")
                        && !value.starts_with("{{")
                        && !value.starts_with("[redacted]")
                    {
                        return Some("credential assignment");
                    }
                }
            }
        }
    }
    None
}

fn validate_candidate(candidate: &MemoryCandidateV1) -> Result<(), String> {
    uuid::Uuid::parse_str(&candidate.id)
        .map_err(|_| "memory portability candidate id is invalid".to_string())?;
    validate_source_id("source session id", &candidate.source.session_id)?;
    validate_source_id("source message id", &candidate.source.message_id)?;
    let normalized = candidate.content.trim();
    if normalized.is_empty()
        || normalized != candidate.content
        || normalized.chars().count() > MAX_MEMORY_CONTENT_CHARS
        || normalized
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("memory portability candidate content is invalid".into());
    }
    if !valid_hash(&candidate.content_hash)
        || sha256_hex(candidate.content.as_bytes()) != candidate.content_hash
    {
        return Err("memory portability candidate content hash is invalid".into());
    }
    if candidate.ownership != MemoryCandidateOwnershipV1::HostCandidate {
        return Err("memory portability candidate ownership is invalid".into());
    }
    Ok(())
}

fn validate_candidate_store(store: &MemoryCandidateStoreSnapshotV1) -> Result<(), String> {
    if store.version != MEMORY_CANDIDATE_STORE_VERSION {
        return Err(format!(
            "unsupported memory candidate store version: {}",
            store.version
        ));
    }
    if store.candidates.len() > MAX_MEMORY_EXPORT_CANDIDATES {
        return Err(format!(
            "memory candidate store exceeds {MAX_MEMORY_EXPORT_CANDIDATES} entries"
        ));
    }
    let mut ids = HashSet::with_capacity(store.candidates.len());
    for candidate in &store.candidates {
        validate_candidate(candidate)?;
        if !ids.insert(candidate.id.as_str()) {
            return Err("memory candidate store contains duplicate ids".into());
        }
    }
    Ok(())
}

fn injection_id_for(session_id: &str, context_hash: &str) -> String {
    let digest =
        sha256_hex(format!("sunsetz-memory-injection-v1\0{session_id}\0{context_hash}").as_bytes());
    format!("memory-injection-v1-{digest}")
}

fn validate_injection_record(
    record: &MemoryInjectionRecordV1,
    session_id: &str,
) -> Result<(), String> {
    if record.version != MEMORY_INJECTION_LEDGER_VERSION
        || record.session_id != session_id
        || !valid_hash(&record.context_hash)
        || record.injection_id != injection_id_for(session_id, &record.context_hash)
        || record.revision == 0
        || record.attempt == 0
        || record.selections.is_empty()
        || record.selections.len() > MAX_INJECTION_SELECTIONS
        || record.updated_at < record.created_at
        || record.prepared_at < record.created_at
    {
        return Err("invalid memory portability injection record".into());
    }
    let mut ids = HashSet::with_capacity(record.selections.len());
    for selection in &record.selections {
        uuid::Uuid::parse_str(&selection.id)
            .map_err(|_| "invalid memory portability injection selection id".to_string())?;
        if !valid_hash(&selection.expected_content_hash) || !ids.insert(selection.id.as_str()) {
            return Err("invalid memory portability injection selection".into());
        }
    }
    match record.status {
        MemoryInjectionStatusV1::Prepared | MemoryInjectionStatusV1::Dispatching => {
            if record.failure_code.is_some()
                || record.applied_at.is_some()
                || record.removed_at.is_some()
                || record.feedback.is_some()
            {
                return Err("invalid prepared memory portability injection".into());
            }
        }
        MemoryInjectionStatusV1::Applied => {
            if record.failure_code.is_some()
                || record.applied_at.is_none()
                || record.removed_at.is_some()
            {
                return Err("invalid applied memory portability injection".into());
            }
        }
        MemoryInjectionStatusV1::Failed => {
            if record.failure_code.is_none()
                || record.applied_at.is_some()
                || record.removed_at.is_some()
                || record.feedback.is_some()
            {
                return Err("invalid failed memory portability injection".into());
            }
        }
        MemoryInjectionStatusV1::Removed => {
            if record.removed_at.is_none() {
                return Err("invalid removed memory portability injection".into());
            }
        }
    }
    Ok(())
}

fn validate_ledger(
    ledger: &MemoryInjectionLedgerSnapshotV1,
    session_id: &str,
) -> Result<(), String> {
    if ledger.version != MEMORY_INJECTION_LEDGER_VERSION {
        return Err(format!(
            "unsupported memory injection ledger version: {}",
            ledger.version
        ));
    }
    if ledger.entries.len() > MAX_MEMORY_LEDGER_ENTRIES {
        return Err(format!(
            "memory injection ledger exceeds {MAX_MEMORY_LEDGER_ENTRIES} entries"
        ));
    }
    let mut ids = HashSet::with_capacity(ledger.entries.len());
    for record in &ledger.entries {
        validate_injection_record(record, session_id)?;
        if !ids.insert(record.injection_id.as_str()) {
            return Err("memory injection ledger contains duplicate ids".into());
        }
    }
    Ok(())
}

fn validate_sessions(sessions: &[crate::store::SessionMeta]) -> Result<(), String> {
    let mut ids = HashSet::with_capacity(sessions.len());
    for session in sessions {
        validate_source_id("session index id", &session.id)?;
        if !ids.insert(session.id.as_str()) {
            return Err("memory portability sessions index contains duplicate ids".into());
        }
    }
    Ok(())
}

fn discover_ledger_sessions(root: &Path) -> Result<Vec<String>, String> {
    let sessions_root = root.join("sessions");
    let entries = match fs::read_dir(&sessions_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "read memory portability sessions directory {}: {error}",
                sessions_root.display()
            ))
        }
    };
    let mut session_ids = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("read memory session entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect memory session entry: {error}"))?;
        if !file_type.is_dir() {
            continue;
        }
        let Some(session_id) = entry.file_name().to_str().map(str::to_string) else {
            if entry.path().join("memory-injections.v1.json").exists() {
                return Err("memory injection ledger has a non-UTF-8 session directory".into());
            }
            continue;
        };
        if !entry.path().join("memory-injections.v1.json").is_file() {
            continue;
        }
        validate_source_id("ledger session id", &session_id)?;
        session_ids.insert(session_id);
        if session_ids.len() > MAX_MEMORY_LEDGER_FILES {
            return Err(format!(
                "memory portability exceeds {MAX_MEMORY_LEDGER_FILES} ledger files"
            ));
        }
    }
    Ok(session_ids.into_iter().collect())
}

fn acquire_snapshot(root: &Path) -> Result<LockedMemorySnapshot, String> {
    let candidate_store_path = candidate_store_path(root);
    let sessions_index_path = sessions_index_path(root);
    let mut locks = Vec::new();

    // Match the established Memory injection lock order: candidates, session
    // index, source journals, then per-target injection ledgers.
    locks.push(crate::store_lock::lock_exclusive(&candidate_store_path)?);
    locks.push(crate::store_lock::lock_exclusive(&sessions_index_path)?);

    let candidate_store = read_json_or_default(
        &candidate_store_path,
        MemoryCandidateStoreSnapshotV1::default,
    )?;
    validate_candidate_store(&candidate_store)?;
    let sessions = read_sessions_index(&sessions_index_path)?;
    validate_sessions(&sessions)?;
    let live_session_ids = sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect::<HashSet<_>>();

    let source_session_ids = candidate_store
        .candidates
        .iter()
        .map(|candidate| candidate.source.session_id.as_str())
        .filter(|session_id| live_session_ids.contains(*session_id))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    for session_id in &source_session_ids {
        validate_session_directory_boundary(root, session_id)?;
        locks.push(crate::store_lock::lock_exclusive(&messages_path(
            root, session_id,
        ))?);
    }
    let mut messages = HashMap::new();
    for session_id in source_session_ids {
        let path = messages_path(root, &session_id);
        if path.is_file() {
            messages.insert(session_id, read_messages(&path)?);
        }
    }

    let ledger_session_ids = discover_ledger_sessions(root)?;
    for session_id in &ledger_session_ids {
        locks.push(crate::store_lock::lock_exclusive(&ledger_path(
            root, session_id,
        ))?);
    }
    let mut ledgers = BTreeMap::new();
    let mut total_entries = 0_usize;
    for session_id in ledger_session_ids {
        let path = ledger_path(root, &session_id);
        let ledger = read_json_or_default(&path, MemoryInjectionLedgerSnapshotV1::default)?;
        validate_ledger(&ledger, &session_id)?;
        total_entries = total_entries
            .checked_add(ledger.entries.len())
            .ok_or_else(|| "memory portability injection count overflow".to_string())?;
        if total_entries > MAX_MEMORY_EXPORT_INJECTIONS {
            return Err(format!(
                "memory portability exceeds {MAX_MEMORY_EXPORT_INJECTIONS} injection records"
            ));
        }
        ledgers.insert(session_id, (path, ledger));
    }

    Ok(LockedMemorySnapshot {
        _locks: locks,
        candidate_store_path,
        candidate_store,
        sessions,
        messages,
        ledgers,
    })
}

fn live_provenance(snapshot: &LockedMemorySnapshot, candidate: &MemoryCandidateV1) -> bool {
    snapshot
        .sessions
        .iter()
        .any(|session| session.id == candidate.source.session_id)
        && snapshot
            .messages
            .get(&candidate.source.session_id)
            .is_some_and(|messages| {
                messages.iter().any(|message| {
                    message.id == candidate.source.message_id && message.role == "user"
                })
            })
}

fn portable_provenance(candidate: &MemoryCandidateV1, live: bool) -> MemoryPortableProvenanceV1 {
    MemoryPortableProvenanceV1 {
        kind: MemoryPortableProvenanceKindV1::PersistedUserMessage,
        session_id: candidate.source.session_id.clone(),
        message_id: candidate.source.message_id.clone(),
        live,
    }
}

fn build_export_from_snapshot(
    snapshot: &LockedMemorySnapshot,
    include_injection_audit: bool,
) -> Result<MemoryPortableExportV1, String> {
    let mut candidates = Vec::new();
    for candidate in &snapshot.candidate_store.candidates {
        if candidate.status != MemoryCandidateStatusV1::Approved
            || !live_provenance(snapshot, candidate)
        {
            continue;
        }
        if let Some(kind) = suspicious_secret(&candidate.content) {
            return Err(format!(
                "MEMORY_EXPORT_SENSITIVE_CONTENT: candidate {} contains {kind}",
                candidate.id
            ));
        }
        if crate::store::redact_text(&candidate.content).contains("[REDACTED]") {
            return Err(format!(
                "MEMORY_EXPORT_SENSITIVE_CONTENT: candidate {} matches a configured secret",
                candidate.id
            ));
        }
        candidates.push(MemoryPortableCandidateV1 {
            candidate_id: candidate.id.clone(),
            content_hash: candidate.content_hash.clone(),
            category: candidate.candidate_type.into(),
            content: candidate.content.clone(),
            provenance: portable_provenance(candidate, true),
            created_at: candidate.created_at,
            updated_at: candidate.updated_at,
        });
    }
    candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    if candidates.len() > MAX_MEMORY_EXPORT_CANDIDATES {
        return Err(format!(
            "memory export exceeds {MAX_MEMORY_EXPORT_CANDIDATES} candidates"
        ));
    }

    let exported_ids = candidates
        .iter()
        .map(|candidate| candidate.candidate_id.as_str())
        .collect::<HashSet<_>>();
    let mut injection_audit = Vec::new();
    if include_injection_audit {
        for (session_id, (_, ledger)) in &snapshot.ledgers {
            for record in &ledger.entries {
                if !record
                    .selections
                    .iter()
                    .any(|selection| exported_ids.contains(selection.id.as_str()))
                {
                    continue;
                }
                let mut selections = record
                    .selections
                    .iter()
                    .map(|selection| MemoryPortableInjectionSelectionV1 {
                        candidate_id: selection.id.clone(),
                        content_hash: selection.expected_content_hash.clone(),
                    })
                    .collect::<Vec<_>>();
                selections.sort_by(|left, right| {
                    left.candidate_id
                        .cmp(&right.candidate_id)
                        .then_with(|| left.content_hash.cmp(&right.content_hash))
                });
                injection_audit.push(MemoryPortableInjectionAuditV1 {
                    session_id: session_id.clone(),
                    injection_id: record.injection_id.clone(),
                    context_hash: record.context_hash.clone(),
                    status: record.status,
                    revision: record.revision,
                    attempt: record.attempt,
                    failure_code: record.failure_code,
                    feedback: record.feedback,
                    selections,
                    created_at: record.created_at,
                    prepared_at: record.prepared_at,
                    updated_at: record.updated_at,
                    applied_at: record.applied_at,
                    removed_at: record.removed_at,
                });
            }
        }
        injection_audit.sort_by(|left, right| {
            left.session_id
                .cmp(&right.session_id)
                .then_with(|| left.injection_id.cmp(&right.injection_id))
        });
        if injection_audit.len() > MAX_MEMORY_EXPORT_INJECTIONS {
            return Err(format!(
                "memory export exceeds {MAX_MEMORY_EXPORT_INJECTIONS} injection records"
            ));
        }
    }

    let hash_domain = MemoryExportHashDomainV1 {
        schema: MEMORY_PORTABILITY_SCHEMA,
        version: MEMORY_PORTABILITY_VERSION,
        candidates: &candidates,
        injection_audit: &injection_audit,
        excludes_session_search_index: true,
        excludes_queries_and_prompt_fragments: true,
    };
    let content_hash = canonical_hash(&hash_domain, "memory portability export")?;
    let export = MemoryPortableExportV1 {
        schema: MEMORY_PORTABILITY_SCHEMA.into(),
        version: MEMORY_PORTABILITY_VERSION,
        content_hash,
        candidates,
        injection_audit,
        excludes_session_search_index: true,
        excludes_queries_and_prompt_fragments: true,
    };
    let bytes = serde_json::to_vec(&export)
        .map_err(|error| format!("serialize bounded memory export: {error}"))?;
    if bytes.len() > MAX_MEMORY_PORTABILITY_BYTES {
        return Err(format!(
            "memory export exceeds {MAX_MEMORY_PORTABILITY_BYTES} bytes"
        ));
    }
    Ok(export)
}

fn export_memory_at(
    root: &Path,
    request: MemoryExportRequestV1,
) -> Result<MemoryPortableExportV1, String> {
    validate_version(request.version)?;
    let snapshot = acquire_snapshot(root)?;
    build_export_from_snapshot(&snapshot, request.include_injection_audit)
}

pub fn export_memory_v1(request: MemoryExportRequestV1) -> Result<MemoryPortableExportV1, String> {
    export_memory_at(&crate::paths::app_data_root(), request)
}

fn validate_clear_scope(scope: &MemoryClearScopeV1) -> Result<MemoryClearScopeV1, String> {
    let total = scope
        .candidate_selections
        .len()
        .checked_add(scope.source_session_ids.len())
        .ok_or_else(|| "memory clear scope size overflow".to_string())?;
    if total == 0 {
        return Err("memory clear requires a candidate or source-session scope".into());
    }
    if total > MAX_MEMORY_CLEAR_SCOPES {
        return Err(format!(
            "memory clear exceeds {MAX_MEMORY_CLEAR_SCOPES} scope entries"
        ));
    }

    let mut candidate_ids = HashSet::new();
    let mut candidate_selections = Vec::with_capacity(scope.candidate_selections.len());
    for selection in &scope.candidate_selections {
        uuid::Uuid::parse_str(&selection.candidate_id)
            .map_err(|_| "memory clear candidate id is invalid".to_string())?;
        if !valid_hash(&selection.expected_content_hash) {
            return Err("memory clear expected candidate content hash is invalid".into());
        }
        if !candidate_ids.insert(selection.candidate_id.as_str()) {
            return Err("memory clear contains duplicate candidate ids".into());
        }
        candidate_selections.push(selection.clone());
    }
    candidate_selections.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));

    let mut source_ids = HashSet::new();
    let mut source_session_ids = Vec::with_capacity(scope.source_session_ids.len());
    for session_id in &scope.source_session_ids {
        validate_source_id("source session id", session_id)?;
        if !source_ids.insert(session_id.as_str()) {
            return Err("memory clear contains duplicate source session ids".into());
        }
        source_session_ids.push(session_id.clone());
    }
    source_session_ids.sort();

    Ok(MemoryClearScopeV1 {
        candidate_selections,
        source_session_ids,
    })
}

fn candidate_store_hash(store: &MemoryCandidateStoreSnapshotV1) -> Result<String, String> {
    canonical_hash(store, "memory candidate store CAS")
}

fn sessions_index_hash(sessions: &[crate::store::SessionMeta]) -> Result<String, String> {
    let ids = sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect::<BTreeSet<_>>();
    canonical_hash(&ids, "memory sessions index CAS")
}

fn plan_hash(plan: &MemoryClearPlanV1) -> Result<String, String> {
    canonical_hash(
        &MemoryClearPlanHashDomainV1 {
            schema: &plan.schema,
            version: plan.version,
            dry_run: plan.dry_run,
            requires_confirmation: plan.requires_confirmation,
            candidate_store_hash: &plan.candidate_store_hash,
            sessions_index_hash: &plan.sessions_index_hash,
            scope: &plan.scope,
            candidates: &plan.candidates,
            injections: &plan.injections,
            skipped: &plan.skipped,
            session_search_index_is_rebuildable_cache: plan
                .session_search_index_is_rebuildable_cache,
        },
        "memory clear plan",
    )
}

fn build_clear_plan_from_snapshot(
    snapshot: &LockedMemorySnapshot,
    scope: MemoryClearScopeV1,
) -> Result<MemoryClearPlanV1, String> {
    let scope = validate_clear_scope(&scope)?;
    let candidates_by_id = snapshot
        .candidate_store
        .candidates
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let source_scope = scope
        .source_session_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut selected_ids = BTreeSet::new();
    let mut skipped = Vec::new();

    for selection in &scope.candidate_selections {
        let Some(candidate) = candidates_by_id.get(selection.candidate_id.as_str()) else {
            skipped.push(MemoryClearSkippedV1 {
                kind: MemoryClearSkippedKindV1::Candidate,
                id: selection.candidate_id.clone(),
                reason: MemoryClearSkippedReasonV1::CandidateNotFound,
            });
            continue;
        };
        if candidate.content_hash != selection.expected_content_hash {
            return Err(format!(
                "STALE_MEMORY_CANDIDATE: content hash mismatch for {}",
                selection.candidate_id
            ));
        }
        selected_ids.insert(selection.candidate_id.clone());
    }

    let mut matched_sources = HashSet::new();
    for candidate in &snapshot.candidate_store.candidates {
        if source_scope.contains(candidate.source.session_id.as_str()) {
            selected_ids.insert(candidate.id.clone());
            matched_sources.insert(candidate.source.session_id.as_str());
        }
    }
    for session_id in &scope.source_session_ids {
        if !matched_sources.contains(session_id.as_str()) {
            skipped.push(MemoryClearSkippedV1 {
                kind: MemoryClearSkippedKindV1::SourceSession,
                id: session_id.clone(),
                reason: MemoryClearSkippedReasonV1::NoCandidatesForProvenance,
            });
        }
    }
    skipped.sort_by(|left, right| {
        let left_kind = match left.kind {
            MemoryClearSkippedKindV1::Candidate => 0_u8,
            MemoryClearSkippedKindV1::SourceSession => 1_u8,
        };
        let right_kind = match right.kind {
            MemoryClearSkippedKindV1::Candidate => 0_u8,
            MemoryClearSkippedKindV1::SourceSession => 1_u8,
        };
        left_kind
            .cmp(&right_kind)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut candidates = selected_ids
        .iter()
        .filter_map(|candidate_id| candidates_by_id.get(candidate_id.as_str()))
        .map(|candidate| MemoryClearCandidatePlanV1 {
            candidate_id: candidate.id.clone(),
            expected_content_hash: candidate.content_hash.clone(),
            category: candidate.candidate_type.into(),
            provenance: portable_provenance(candidate, live_provenance(snapshot, candidate)),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));

    let mut injections = Vec::new();
    for (session_id, (_, ledger)) in &snapshot.ledgers {
        for record in &ledger.entries {
            let mut matched_candidate_ids = record
                .selections
                .iter()
                .filter(|selection| selected_ids.contains(&selection.id))
                .map(|selection| selection.id.clone())
                .collect::<Vec<_>>();
            if matched_candidate_ids.is_empty() {
                continue;
            }
            matched_candidate_ids.sort();
            injections.push(MemoryClearInjectionPlanV1 {
                session_id: session_id.clone(),
                injection_id: record.injection_id.clone(),
                expected_context_hash: record.context_hash.clone(),
                expected_revision: record.revision,
                matched_candidate_ids,
            });
        }
    }
    injections.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then_with(|| left.injection_id.cmp(&right.injection_id))
    });
    if injections.len() > MAX_MEMORY_EXPORT_INJECTIONS {
        return Err(format!(
            "memory clear exceeds {MAX_MEMORY_EXPORT_INJECTIONS} injection records"
        ));
    }

    let mut plan = MemoryClearPlanV1 {
        schema: MEMORY_CLEAR_SCHEMA.into(),
        version: MEMORY_PORTABILITY_VERSION,
        dry_run: true,
        requires_confirmation: true,
        candidate_store_hash: candidate_store_hash(&snapshot.candidate_store)?,
        sessions_index_hash: sessions_index_hash(&snapshot.sessions)?,
        scope,
        candidates,
        injections,
        skipped,
        session_search_index_is_rebuildable_cache: true,
        plan_hash: String::new(),
    };
    plan.plan_hash = plan_hash(&plan)?;
    ensure_serialized_bound(&plan, MAX_MEMORY_PORTABILITY_BYTES, "memory clear plan")?;
    Ok(plan)
}

fn validate_clear_plan(plan: &MemoryClearPlanV1) -> Result<(), String> {
    validate_version(plan.version)?;
    if plan.schema != MEMORY_CLEAR_SCHEMA
        || !plan.dry_run
        || !plan.requires_confirmation
        || !plan.session_search_index_is_rebuildable_cache
        || !valid_hash(&plan.candidate_store_hash)
        || !valid_hash(&plan.sessions_index_hash)
        || !valid_hash(&plan.plan_hash)
    {
        return Err("invalid memory clear plan envelope".into());
    }
    let normalized_scope = validate_clear_scope(&plan.scope)?;
    if normalized_scope != plan.scope {
        return Err("memory clear plan scope is not normalized".into());
    }
    if plan.candidates.len() > MAX_MEMORY_EXPORT_CANDIDATES
        || plan.injections.len() > MAX_MEMORY_EXPORT_INJECTIONS
        || plan.skipped.len() > MAX_MEMORY_CLEAR_SCOPES
    {
        return Err("memory clear plan exceeds bounds".into());
    }
    let mut candidate_ids = HashSet::new();
    let mut previous_candidate_id: Option<&str> = None;
    for candidate in &plan.candidates {
        uuid::Uuid::parse_str(&candidate.candidate_id)
            .map_err(|_| "memory clear plan candidate id is invalid".to_string())?;
        if !valid_hash(&candidate.expected_content_hash)
            || !candidate_ids.insert(candidate.candidate_id.as_str())
            || previous_candidate_id
                .is_some_and(|previous| previous >= candidate.candidate_id.as_str())
        {
            return Err("memory clear plan candidates are invalid or unsorted".into());
        }
        validate_source_id(
            "plan provenance session id",
            &candidate.provenance.session_id,
        )?;
        validate_source_id(
            "plan provenance message id",
            &candidate.provenance.message_id,
        )?;
        if candidate.provenance.kind != MemoryPortableProvenanceKindV1::PersistedUserMessage {
            return Err("memory clear plan provenance kind is invalid".into());
        }
        previous_candidate_id = Some(&candidate.candidate_id);
    }
    let mut injection_keys = HashSet::new();
    let mut previous_injection_key: Option<(&str, &str)> = None;
    for injection in &plan.injections {
        validate_source_id("plan injection session id", &injection.session_id)?;
        if injection.injection_id
            != injection_id_for(&injection.session_id, &injection.expected_context_hash)
            || !valid_hash(&injection.expected_context_hash)
            || injection.expected_revision == 0
            || injection.matched_candidate_ids.is_empty()
            || injection.matched_candidate_ids.len() > MAX_INJECTION_SELECTIONS
            || !injection_keys.insert((
                injection.session_id.as_str(),
                injection.injection_id.as_str(),
            ))
        {
            return Err("memory clear plan injection CAS is invalid".into());
        }
        let current_key = (
            injection.session_id.as_str(),
            injection.injection_id.as_str(),
        );
        if previous_injection_key.is_some_and(|previous| previous >= current_key) {
            return Err("memory clear plan injections are unsorted".into());
        }
        let mut previous_id: Option<&str> = None;
        for candidate_id in &injection.matched_candidate_ids {
            if !candidate_ids.contains(candidate_id.as_str())
                || previous_id.is_some_and(|previous| previous >= candidate_id.as_str())
            {
                return Err("memory clear plan injection matches are invalid".into());
            }
            previous_id = Some(candidate_id);
        }
        previous_injection_key = Some(current_key);
    }
    let mut previous_skipped_key: Option<(u8, &str)> = None;
    for skipped in &plan.skipped {
        let (kind, valid_reason) = match skipped.kind {
            MemoryClearSkippedKindV1::Candidate => (
                0_u8,
                skipped.reason == MemoryClearSkippedReasonV1::CandidateNotFound,
            ),
            MemoryClearSkippedKindV1::SourceSession => (
                1_u8,
                skipped.reason == MemoryClearSkippedReasonV1::NoCandidatesForProvenance,
            ),
        };
        if !valid_reason {
            return Err("memory clear plan skipped reason is invalid".into());
        }
        match skipped.kind {
            MemoryClearSkippedKindV1::Candidate => {
                uuid::Uuid::parse_str(&skipped.id)
                    .map_err(|_| "memory clear skipped candidate id is invalid".to_string())?;
            }
            MemoryClearSkippedKindV1::SourceSession => {
                validate_source_id("skipped source session id", &skipped.id)?;
            }
        }
        let key = (kind, skipped.id.as_str());
        if previous_skipped_key.is_some_and(|previous| previous >= key) {
            return Err("memory clear plan skipped entries are unsorted".into());
        }
        previous_skipped_key = Some(key);
    }
    if plan_hash(plan)? != plan.plan_hash {
        return Err("memory clear plan hash is invalid".into());
    }
    ensure_serialized_bound(plan, MAX_MEMORY_PORTABILITY_BYTES, "memory clear plan")?;
    Ok(())
}

fn preview_memory_clear_at(
    root: &Path,
    request: MemoryClearPreviewRequestV1,
) -> Result<MemoryClearPlanV1, String> {
    validate_version(request.version)?;
    let scope = validate_clear_scope(&request.scope)?;
    let snapshot = acquire_snapshot(root)?;
    build_clear_plan_from_snapshot(&snapshot, scope)
}

pub fn preview_memory_clear_v1(
    request: MemoryClearPreviewRequestV1,
) -> Result<MemoryClearPlanV1, String> {
    preview_memory_clear_at(&crate::paths::app_data_root(), request)
}

#[cfg(not(windows))]
fn replace_file_atomic(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|error| format!("rename into place: {error}"))
}

#[cfg(windows)]
fn replace_file_atomic(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| format!("replace into place: {error}"))
}

fn write_bytes_unlocked_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("memory clear store dir: {error}"))?;
    }
    let temporary = {
        let mut value = path.as_os_str().to_os_string();
        value.push(format!(".tmp.{}", uuid::Uuid::new_v4()));
        PathBuf::from(value)
    };
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("create memory clear temp: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("write memory clear temp: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("sync memory clear temp: {error}"))?;
        drop(file);
        replace_file_atomic(&temporary, path)?;
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| format!("sync memory clear parent directory: {error}"))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

struct PendingStoreWrite {
    path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}

fn commit_store_writes(writes: &[PendingStoreWrite]) -> Result<(), String> {
    let mut committed = 0_usize;
    for write in writes {
        if let Err(error) = write_bytes_unlocked_atomic(&write.path, &write.after) {
            let mut rollback_errors = Vec::new();
            for rollback in writes[..committed].iter().rev() {
                if let Err(rollback_error) =
                    write_bytes_unlocked_atomic(&rollback.path, &rollback.before)
                {
                    rollback_errors.push(format!("{}: {rollback_error}", rollback.path.display()));
                }
            }
            if rollback_errors.is_empty() {
                return Err(format!(
                    "MEMORY_CLEAR_WRITE_FAILED: {}: {error}; prior stores rolled back",
                    write.path.display()
                ));
            }
            return Err(format!(
                "MEMORY_CLEAR_ROLLBACK_FAILED: {}: {error}; rollback errors: {}",
                write.path.display(),
                rollback_errors.join(" | ")
            ));
        }
        committed += 1;
    }
    Ok(())
}

fn confirm_memory_clear_at(
    root: &Path,
    request: MemoryClearConfirmRequestV1,
) -> Result<MemoryClearResultV1, String> {
    validate_version(request.version)?;
    validate_clear_plan(&request.plan)?;
    let mut snapshot = acquire_snapshot(root)?;
    let current_plan = build_clear_plan_from_snapshot(&snapshot, request.plan.scope.clone())?;
    if current_plan != request.plan {
        return Err("STALE_MEMORY_CLEAR_PLAN: Memory facts or audit revisions changed".into());
    }

    let candidate_ids = current_plan
        .candidates
        .iter()
        .map(|candidate| candidate.candidate_id.as_str())
        .collect::<HashSet<_>>();
    let injection_keys = current_plan
        .injections
        .iter()
        .map(|injection| {
            (
                injection.session_id.as_str(),
                injection.injection_id.as_str(),
            )
        })
        .collect::<HashSet<_>>();

    let before_candidate_store_hash = candidate_store_hash(&snapshot.candidate_store)?;
    let before_candidate_store = serde_json::to_vec_pretty(&snapshot.candidate_store)
        .map_err(|error| format!("serialize original memory candidate store: {error}"))?;
    snapshot
        .candidate_store
        .candidates
        .retain(|candidate| !candidate_ids.contains(candidate.id.as_str()));
    validate_candidate_store(&snapshot.candidate_store)?;
    let after_candidate_store_hash = candidate_store_hash(&snapshot.candidate_store)?;
    let after_candidate_store = serde_json::to_vec_pretty(&snapshot.candidate_store)
        .map_err(|error| format!("serialize cleared memory candidate store: {error}"))?;

    let mut writes = Vec::new();
    let mut deleted_injections = Vec::new();
    for (session_id, (path, ledger)) in &mut snapshot.ledgers {
        let before_len = ledger.entries.len();
        let before = serde_json::to_vec_pretty(ledger)
            .map_err(|error| format!("serialize original memory injection ledger: {error}"))?;
        let mut removed = Vec::new();
        ledger.entries.retain(|record| {
            if injection_keys.contains(&(session_id.as_str(), record.injection_id.as_str())) {
                removed.push(MemoryClearedInjectionV1 {
                    session_id: session_id.clone(),
                    injection_id: record.injection_id.clone(),
                    context_hash: record.context_hash.clone(),
                    revision: record.revision,
                });
                false
            } else {
                true
            }
        });
        if ledger.entries.len() == before_len {
            continue;
        }
        validate_ledger(ledger, session_id)?;
        let after = serde_json::to_vec_pretty(ledger)
            .map_err(|error| format!("serialize cleared memory injection ledger: {error}"))?;
        writes.push(PendingStoreWrite {
            path: path.clone(),
            before,
            after,
        });
        deleted_injections.extend(removed);
    }
    deleted_injections.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then_with(|| left.injection_id.cmp(&right.injection_id))
    });

    if before_candidate_store != after_candidate_store {
        // Candidate deletion is last. If it fails, all prior ledger writes can
        // be rolled back while their locks remain held.
        writes.push(PendingStoreWrite {
            path: snapshot.candidate_store_path.clone(),
            before: before_candidate_store,
            after: after_candidate_store,
        });
    }
    commit_store_writes(&writes)?;

    let deleted_candidates = current_plan
        .candidates
        .iter()
        .map(|candidate| MemoryClearedCandidateV1 {
            candidate_id: candidate.candidate_id.clone(),
            content_hash: candidate.expected_content_hash.clone(),
        })
        .collect();
    Ok(MemoryClearResultV1 {
        version: MEMORY_PORTABILITY_VERSION,
        plan_hash: current_plan.plan_hash,
        before_candidate_store_hash,
        after_candidate_store_hash,
        deleted_candidates,
        deleted_injections,
        skipped: current_plan.skipped,
        session_search_index_mutated: false,
        session_search_index_is_rebuildable_cache: true,
    })
}

pub fn confirm_memory_clear_v1(
    request: MemoryClearConfirmRequestV1,
) -> Result<MemoryClearResultV1, String> {
    confirm_memory_clear_at(&crate::paths::app_data_root(), request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_candidates::{MemoryCandidateSourceV1, MemoryCandidateTypeV1};
    use crate::memory_injection::MemoryInjectionFeedbackV1;
    use serde_json::json;
    use std::sync::{Arc, Barrier};
    use std::thread;

    struct TestRoot {
        root: PathBuf,
    }

    impl TestRoot {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "sunsetz-memory-portability-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(root.join("sessions")).unwrap();
            Self { root }
        }

        fn write_sessions(&self, session_ids: &[&str]) {
            let now = Utc::now();
            let sessions = session_ids
                .iter()
                .map(|session_id| session(session_id, now))
                .collect::<Vec<_>>();
            fs::write(
                sessions_index_path(&self.root),
                serde_json::to_vec_pretty(&sessions).unwrap(),
            )
            .unwrap();
        }

        fn write_user_message(&self, session_id: &str, message_id: &str, content: &str) {
            let directory = self.root.join("sessions").join(session_id);
            fs::create_dir_all(&directory).unwrap();
            let messages = vec![crate::store::ChatMessageStored {
                id: message_id.into(),
                role: "user".into(),
                content: content.into(),
                thought: None,
                created_at: Utc::now(),
                is_error: false,
                attachments: None,
                marker: None,
            }];
            fs::write(
                directory.join("messages.json"),
                serde_json::to_vec_pretty(&messages).unwrap(),
            )
            .unwrap();
        }

        fn write_candidates(&self, candidates: &[MemoryCandidateV1]) {
            let store = MemoryCandidateStoreSnapshotV1 {
                version: MEMORY_CANDIDATE_STORE_VERSION,
                candidates: candidates.to_vec(),
            };
            fs::write(
                candidate_store_path(&self.root),
                serde_json::to_vec_pretty(&store).unwrap(),
            )
            .unwrap();
        }

        fn write_ledger(&self, session_id: &str, records: &[MemoryInjectionRecordV1]) {
            let path = ledger_path(&self.root, session_id);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let ledger = MemoryInjectionLedgerSnapshotV1 {
                version: MEMORY_INJECTION_LEDGER_VERSION,
                entries: records.to_vec(),
            };
            fs::write(path, serde_json::to_vec_pretty(&ledger).unwrap()).unwrap();
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn session(id: &str, now: DateTime<Utc>) -> crate::store::SessionMeta {
        crate::store::SessionMeta {
            id: id.into(),
            project_id: None,
            title: format!("Session {id}"),
            agent_session_id: None,
            created_at: now,
            updated_at: now,
            model_id: None,
            archived: false,
            effort: None,
            mode: None,
            permission_policy: None,
            scheduled: false,
            context_usage: None,
        }
    }

    fn candidate(
        content: &str,
        status: MemoryCandidateStatusV1,
        candidate_type: MemoryCandidateTypeV1,
        session_id: &str,
        message_id: &str,
    ) -> MemoryCandidateV1 {
        let now = Utc::now();
        MemoryCandidateV1 {
            id: uuid::Uuid::new_v4().to_string(),
            status,
            candidate_type,
            content: content.into(),
            content_hash: sha256_hex(content.as_bytes()),
            source: MemoryCandidateSourceV1 {
                session_id: session_id.into(),
                message_id: message_id.into(),
            },
            ownership: MemoryCandidateOwnershipV1::HostCandidate,
            created_at: now,
            updated_at: now,
        }
    }

    fn applied_record(
        target_session_id: &str,
        candidates: &[&MemoryCandidateV1],
    ) -> MemoryInjectionRecordV1 {
        let now = Utc::now();
        let context_hash = sha256_hex(
            candidates
                .iter()
                .flat_map(|candidate| candidate.content_hash.as_bytes())
                .copied()
                .collect::<Vec<_>>()
                .as_slice(),
        );
        MemoryInjectionRecordV1 {
            version: MEMORY_INJECTION_LEDGER_VERSION,
            injection_id: injection_id_for(target_session_id, &context_hash),
            session_id: target_session_id.into(),
            context_hash,
            selections: candidates
                .iter()
                .map(
                    |candidate| crate::memory_candidates::MemoryCandidateMutationRequestV1 {
                        id: candidate.id.clone(),
                        expected_content_hash: candidate.content_hash.clone(),
                    },
                )
                .collect(),
            status: MemoryInjectionStatusV1::Applied,
            revision: 2,
            attempt: 1,
            failure_code: None,
            feedback: None,
            created_at: now,
            prepared_at: now,
            updated_at: now,
            applied_at: Some(now),
            removed_at: None,
        }
    }

    fn explicit_scope(candidate: &MemoryCandidateV1) -> MemoryClearScopeV1 {
        MemoryClearScopeV1 {
            candidate_selections: vec![MemoryClearCandidateSelectionV1 {
                candidate_id: candidate.id.clone(),
                expected_content_hash: candidate.content_hash.clone(),
            }],
            source_session_ids: Vec::new(),
        }
    }

    fn preview(root: &TestRoot, scope: MemoryClearScopeV1) -> MemoryClearPlanV1 {
        preview_memory_clear_at(
            &root.root,
            MemoryClearPreviewRequestV1 {
                version: MEMORY_PORTABILITY_VERSION,
                scope,
            },
        )
        .unwrap()
    }

    #[test]
    fn export_is_deterministic_approved_live_and_metadata_only() {
        let root = TestRoot::new("export-deterministic");
        root.write_sessions(&["source", "target"]);
        root.write_user_message("source", "user-1", "journal-secret-do-not-export");
        let first = candidate(
            "Prefer concise verification notes.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::UserPreference,
            "source",
            "user-1",
        );
        let second = candidate(
            "The project uses Rust.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::ProjectFact,
            "source",
            "user-1",
        );
        let pending = candidate(
            "Pending content must not export.",
            MemoryCandidateStatusV1::Pending,
            MemoryCandidateTypeV1::WorkflowHint,
            "source",
            "user-1",
        );
        root.write_candidates(&[second.clone(), pending, first.clone()]);
        let one = applied_record("target", &[&second]);
        let two = applied_record("target", &[&first]);
        root.write_ledger("target", &[two.clone(), one.clone()]);

        let request = MemoryExportRequestV1 {
            version: MEMORY_PORTABILITY_VERSION,
            include_injection_audit: true,
        };
        let export = export_memory_at(&root.root, request.clone()).unwrap();
        assert_eq!(export.candidates.len(), 2);
        assert_eq!(export.injection_audit.len(), 2);
        assert!(export
            .candidates
            .windows(2)
            .all(|pair| pair[0].candidate_id < pair[1].candidate_id));
        assert!(export
            .injection_audit
            .windows(2)
            .all(|pair| pair[0].injection_id < pair[1].injection_id));
        assert!(export.candidates.iter().all(|item| item.provenance.live));
        assert!(export.excludes_session_search_index);
        assert!(export.excludes_queries_and_prompt_fragments);

        // Semantic order in source stores does not affect the portable form.
        root.write_candidates(&[first.clone(), second.clone()]);
        root.write_ledger("target", &[one, two]);
        let repeated = export_memory_at(&root.root, request).unwrap();
        assert_eq!(repeated, export);

        let json = serde_json::to_string(&export).unwrap();
        for forbidden in [
            "journal-secret-do-not-export",
            "promptFragment",
            "prompt_fragment",
            "query",
            "snippet",
            "messages_fts",
        ] {
            assert!(!json.contains(forbidden), "export leaked {forbidden}");
        }
        let hash_domain = MemoryExportHashDomainV1 {
            schema: &export.schema,
            version: export.version,
            candidates: &export.candidates,
            injection_audit: &export.injection_audit,
            excludes_session_search_index: export.excludes_session_search_index,
            excludes_queries_and_prompt_fragments: export.excludes_queries_and_prompt_fragments,
        };
        assert_eq!(
            export.content_hash,
            canonical_hash(&hash_domain, "test export").unwrap()
        );
    }

    #[test]
    fn export_rejects_secrets_and_omits_deleted_provenance() {
        let root = TestRoot::new("export-secret");
        root.write_sessions(&["live"]);
        root.write_user_message("live", "user-live", "visible source");
        let live = candidate(
            "api_key=abcdefgh12345678",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::UserPreference,
            "live",
            "user-live",
        );
        root.write_candidates(&[live]);
        let error = export_memory_at(
            &root.root,
            MemoryExportRequestV1 {
                version: 1,
                include_injection_audit: false,
            },
        )
        .unwrap_err();
        assert!(error.contains("MEMORY_EXPORT_SENSITIVE_CONTENT"));

        let safe = candidate(
            "Prefer concise answers.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::UserPreference,
            "live",
            "user-live",
        );
        let deleted = candidate(
            "A fact from a deleted session.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::ProjectFact,
            "deleted",
            "user-deleted",
        );
        root.write_candidates(&[deleted, safe.clone()]);
        let export = export_memory_at(
            &root.root,
            MemoryExportRequestV1 {
                version: 1,
                include_injection_audit: false,
            },
        )
        .unwrap();
        assert_eq!(export.candidates.len(), 1);
        assert_eq!(export.candidates[0].candidate_id, safe.id);
    }

    #[test]
    fn deleted_session_provenance_can_be_previewed_and_cleared() {
        let root = TestRoot::new("deleted-provenance-clear");
        root.write_sessions(&["target"]);
        root.write_user_message("target", "kept-message", "kept source");
        let orphan = candidate(
            "Orphaned project fact.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::ProjectFact,
            "deleted-source",
            "deleted-message",
        );
        let kept = candidate(
            "Keep this preference.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::UserPreference,
            "target",
            "kept-message",
        );
        root.write_candidates(&[orphan.clone(), kept.clone()]);
        // A context hash binds the whole selection. Clearing one selected
        // candidate removes the entire audit row but keeps unrelated facts.
        let record = applied_record("target", &[&orphan, &kept]);
        root.write_ledger("target", &[record]);
        let fts_path = root.root.join("session-search.v1.sqlite3");
        fs::write(&fts_path, b"rebuildable-index-sentinel").unwrap();

        let plan = preview(
            &root,
            MemoryClearScopeV1 {
                candidate_selections: Vec::new(),
                source_session_ids: vec!["deleted-source".into()],
            },
        );
        assert_eq!(plan.candidates.len(), 1);
        assert!(!plan.candidates[0].provenance.live);
        assert_eq!(plan.injections.len(), 1);
        let result =
            confirm_memory_clear_at(&root.root, MemoryClearConfirmRequestV1 { version: 1, plan })
                .unwrap();
        assert_eq!(result.deleted_candidates.len(), 1);
        assert_eq!(result.deleted_injections.len(), 1);
        assert!(!result.session_search_index_mutated);
        assert_eq!(fs::read(&fts_path).unwrap(), b"rebuildable-index-sentinel");
        let store: MemoryCandidateStoreSnapshotV1 = read_json_or_default(
            &candidate_store_path(&root.root),
            MemoryCandidateStoreSnapshotV1::default,
        )
        .unwrap();
        assert_eq!(store.candidates.len(), 1);
        assert_eq!(store.candidates[0].id, kept.id);
        let ledger: MemoryInjectionLedgerSnapshotV1 = read_json_or_default(
            &ledger_path(&root.root, "target"),
            MemoryInjectionLedgerSnapshotV1::default,
        )
        .unwrap();
        assert!(ledger.entries.is_empty());
    }

    #[test]
    fn missing_targets_are_reported_as_skipped_without_creating_fact_stores() {
        let root = TestRoot::new("skipped-noop");
        root.write_sessions(&[]);
        let missing_id = uuid::Uuid::new_v4().to_string();
        let plan = preview(
            &root,
            MemoryClearScopeV1 {
                candidate_selections: vec![MemoryClearCandidateSelectionV1 {
                    candidate_id: missing_id.clone(),
                    expected_content_hash: "0".repeat(64),
                }],
                source_session_ids: vec!["deleted-source".into()],
            },
        );
        assert!(plan.candidates.is_empty());
        assert!(plan.injections.is_empty());
        assert_eq!(plan.skipped.len(), 2);
        assert_eq!(plan.skipped[0].id, missing_id);
        assert_eq!(
            plan.skipped[0].reason,
            MemoryClearSkippedReasonV1::CandidateNotFound
        );
        assert_eq!(
            plan.skipped[1].reason,
            MemoryClearSkippedReasonV1::NoCandidatesForProvenance
        );

        let result =
            confirm_memory_clear_at(&root.root, MemoryClearConfirmRequestV1 { version: 1, plan })
                .unwrap();
        assert!(result.deleted_candidates.is_empty());
        assert!(result.deleted_injections.is_empty());
        assert_eq!(result.skipped.len(), 2);
        assert!(!candidate_store_path(&root.root).exists());
    }

    #[test]
    fn stale_candidate_hash_and_stale_injection_revision_fail_closed() {
        let root = TestRoot::new("stale-cas");
        root.write_sessions(&["source", "target"]);
        root.write_user_message("source", "user", "source");
        let item = candidate(
            "Prefer Rust.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::UserPreference,
            "source",
            "user",
        );
        root.write_candidates(&[item.clone()]);
        let record = applied_record("target", &[&item]);
        root.write_ledger("target", &[record.clone()]);

        let mut stale_scope = explicit_scope(&item);
        stale_scope.candidate_selections[0].expected_content_hash = "0".repeat(64);
        let error = preview_memory_clear_at(
            &root.root,
            MemoryClearPreviewRequestV1 {
                version: 1,
                scope: stale_scope,
            },
        )
        .unwrap_err();
        assert!(error.contains("STALE_MEMORY_CANDIDATE"));

        let plan = preview(&root, explicit_scope(&item));
        let mut changed_record = record;
        changed_record.revision += 1;
        changed_record.feedback = Some(MemoryInjectionFeedbackV1::Helpful);
        changed_record.updated_at = Utc::now().max(changed_record.updated_at);
        root.write_ledger("target", &[changed_record]);
        let before = fs::read(candidate_store_path(&root.root)).unwrap();
        let error =
            confirm_memory_clear_at(&root.root, MemoryClearConfirmRequestV1 { version: 1, plan })
                .unwrap_err();
        assert!(error.contains("STALE_MEMORY_CLEAR_PLAN"));
        assert_eq!(fs::read(candidate_store_path(&root.root)).unwrap(), before);
    }

    #[test]
    fn concurrent_confirmation_allows_exactly_one_commit() {
        let root = Arc::new(TestRoot::new("concurrent-confirm"));
        root.write_sessions(&["source", "target"]);
        root.write_user_message("source", "user", "source");
        let item = candidate(
            "Prefer deterministic verification.",
            MemoryCandidateStatusV1::Approved,
            MemoryCandidateTypeV1::UserPreference,
            "source",
            "user",
        );
        root.write_candidates(&[item.clone()]);
        root.write_ledger("target", &[applied_record("target", &[&item])]);
        let plan = preview(&root, explicit_scope(&item));
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            let plan = plan.clone();
            workers.push(thread::spawn(move || {
                barrier.wait();
                confirm_memory_clear_at(
                    &root.root,
                    MemoryClearConfirmRequestV1 { version: 1, plan },
                )
            }));
        }
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert!(results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .all(|error| error.contains("STALE_MEMORY_CLEAR_PLAN")));
    }

    #[test]
    fn strict_contracts_traversal_and_bounds_fail_closed() {
        assert!(serde_json::from_value::<MemoryExportRequestV1>(json!({
            "version": 1,
            "includeInjectionAudit": true,
            "unknown": true,
        }))
        .is_err());
        assert!(
            serde_json::from_value::<MemoryClearPreviewRequestV1>(json!({
                "version": 1,
                "scope": {
                    "candidateSelections": [],
                    "sourceSessionIds": ["source"],
                    "unknown": true,
                }
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<MemoryPortableExportV1>(json!({
            "schema": MEMORY_PORTABILITY_SCHEMA,
            "version": 1,
            "contentHash": "0".repeat(64),
            "candidates": [],
            "injectionAudit": [],
            "excludesSessionSearchIndex": true,
            "excludesQueriesAndPromptFragments": true,
            "unknown": true,
        }))
        .is_err());

        let traversal = MemoryClearScopeV1 {
            candidate_selections: Vec::new(),
            source_session_ids: vec!["../sessions".into()],
        };
        assert!(validate_clear_scope(&traversal).is_err());
        let oversized = MemoryClearScopeV1 {
            candidate_selections: Vec::new(),
            source_session_ids: (0..=MAX_MEMORY_CLEAR_SCOPES)
                .map(|index| format!("session-{index}"))
                .collect(),
        };
        assert!(validate_clear_scope(&oversized).is_err());

        let root = TestRoot::new("store-bound");
        root.write_sessions(&[]);
        let candidates = (0..=MAX_MEMORY_EXPORT_CANDIDATES)
            .map(|index| {
                candidate(
                    &format!("Bounded fact {index}"),
                    MemoryCandidateStatusV1::Pending,
                    MemoryCandidateTypeV1::ProjectFact,
                    "source",
                    &format!("message-{index}"),
                )
            })
            .collect::<Vec<_>>();
        root.write_candidates(&candidates);
        assert!(export_memory_at(
            &root.root,
            MemoryExportRequestV1 {
                version: 1,
                include_injection_audit: false,
            },
        )
        .unwrap_err()
        .contains("exceeds"));
    }
}
