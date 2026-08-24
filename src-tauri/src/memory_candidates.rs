//! Versioned, review-only candidates for limited long-term memory.
//!
//! This store is deliberately independent from session search and FTS. JSON is
//! the source of truth, and every mutation uses the App's locked atomic writer.
//! Creating a candidate only records a pending proposal; no generation or
//! approval is performed here.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MEMORY_CANDIDATE_STORE_VERSION: u8 = 1;
pub const MAX_MEMORY_CANDIDATES: usize = 256;
pub const MAX_MEMORY_CONTENT_CHARS: usize = 2_000;

const MAX_SOURCE_ID_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateStatusV1 {
    Pending,
    Approved,
    Rejected,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateTypeV1 {
    UserPreference,
    ProjectFact,
    WorkflowHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateOwnershipV1 {
    HostCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCandidateSourceV1 {
    pub session_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCandidateV1 {
    pub id: String,
    pub status: MemoryCandidateStatusV1,
    #[serde(rename = "type")]
    pub candidate_type: MemoryCandidateTypeV1,
    pub content: String,
    pub content_hash: String,
    pub source: MemoryCandidateSourceV1,
    pub ownership: MemoryCandidateOwnershipV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCandidateCreateRequestV1 {
    #[serde(rename = "type")]
    pub candidate_type: MemoryCandidateTypeV1,
    pub content: String,
    pub source: MemoryCandidateSourceV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCandidateMutationRequestV1 {
    pub id: String,
    pub expected_content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryCandidateStoreV1 {
    version: u8,
    candidates: Vec<MemoryCandidateV1>,
}

impl Default for MemoryCandidateStoreV1 {
    fn default() -> Self {
        Self {
            version: MEMORY_CANDIDATE_STORE_VERSION,
            candidates: Vec::new(),
        }
    }
}

fn store_path() -> PathBuf {
    crate::paths::app_data_root().join("memory-candidates.v1.json")
}

fn content_hash(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_source_id(label: &str, value: &str) -> Result<String, String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(format!("memory candidate {label} is required"));
    }
    if normalized.chars().count() > MAX_SOURCE_ID_CHARS {
        return Err(format!("memory candidate {label} is too long"));
    }
    if normalized.chars().any(char::is_control) {
        return Err(format!(
            "memory candidate {label} contains control characters"
        ));
    }
    if !normalized
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("memory candidate {label} is invalid"));
    }
    Ok(normalized.to_string())
}

fn verify_persisted_source(
    source: &MemoryCandidateSourceV1,
    messages: &[crate::store::ChatMessageStored],
) -> Result<MemoryCandidateSourceV1, String> {
    let source = MemoryCandidateSourceV1 {
        session_id: validate_source_id("session id", &source.session_id)?,
        message_id: validate_source_id("message id", &source.message_id)?,
    };
    if !messages
        .iter()
        .any(|message| message.id == source.message_id && message.role == "user")
    {
        return Err("MEMORY_SOURCE_NOT_FOUND: expected a persisted user message".into());
    }
    Ok(source)
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

fn normalize_content(content: &str, redact: &impl Fn(&str) -> String) -> Result<String, String> {
    let normalized = content.trim();
    if normalized.is_empty() {
        return Err("memory candidate content is required".into());
    }
    if normalized.chars().count() > MAX_MEMORY_CONTENT_CHARS {
        return Err(format!(
            "memory candidate content exceeds {MAX_MEMORY_CONTENT_CHARS} characters"
        ));
    }
    if normalized
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("memory candidate content contains control characters".into());
    }
    if let Some(kind) = suspicious_secret(normalized) {
        return Err(format!(
            "memory candidate content contains sensitive material ({kind})"
        ));
    }
    if redact(normalized).contains("[REDACTED]") {
        return Err("memory candidate content contains redacted sensitive material".into());
    }
    Ok(normalized.to_string())
}

fn validate_candidate(
    candidate: &MemoryCandidateV1,
    redact: &impl Fn(&str) -> String,
) -> Result<(), String> {
    uuid::Uuid::parse_str(&candidate.id)
        .map_err(|_| "memory candidate has an invalid id".to_string())?;
    validate_source_id("session id", &candidate.source.session_id)?;
    validate_source_id("message id", &candidate.source.message_id)?;
    let normalized = normalize_content(&candidate.content, redact)?;
    if normalized != candidate.content {
        return Err("memory candidate content is not normalized".into());
    }
    if !valid_hash(&candidate.content_hash)
        || content_hash(&candidate.content) != candidate.content_hash
    {
        return Err("memory candidate content hash is invalid".into());
    }
    if candidate.ownership != MemoryCandidateOwnershipV1::HostCandidate {
        return Err("memory candidate ownership is invalid".into());
    }
    Ok(())
}

fn validate_store(
    store: &MemoryCandidateStoreV1,
    redact: &impl Fn(&str) -> String,
) -> Result<(), String> {
    if store.version != MEMORY_CANDIDATE_STORE_VERSION {
        return Err(format!(
            "unsupported memory candidate store version: {}",
            store.version
        ));
    }
    if store.candidates.len() > MAX_MEMORY_CANDIDATES {
        return Err(format!(
            "memory candidate store exceeds {MAX_MEMORY_CANDIDATES} entries"
        ));
    }
    let mut ids = HashSet::with_capacity(store.candidates.len());
    for candidate in &store.candidates {
        validate_candidate(candidate, redact)?;
        if !ids.insert(candidate.id.as_str()) {
            return Err("memory candidate store contains duplicate ids".into());
        }
    }
    Ok(())
}

fn read_store_at(
    path: &Path,
    redact: &impl Fn(&str) -> String,
) -> Result<MemoryCandidateStoreV1, String> {
    let store = match fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => MemoryCandidateStoreV1::default(),
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| format!("parse {}: {error}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            MemoryCandidateStoreV1::default()
        }
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    validate_store(&store, redact)?;
    Ok(store)
}

fn update_store_at<R>(
    path: &Path,
    redact: &impl Fn(&str) -> String,
    update: impl FnOnce(&mut MemoryCandidateStoreV1) -> Result<R, String>,
) -> Result<R, String> {
    crate::store_lock::update_json_locked(path, MemoryCandidateStoreV1::default, |store| {
        validate_store(store, redact)?;
        let result = update(store)?;
        validate_store(store, redact)?;
        Ok(result)
    })
}

pub fn create_pending(
    request: MemoryCandidateCreateRequestV1,
) -> Result<MemoryCandidateV1, String> {
    let session_id = validate_source_id("session id", &request.source.session_id)?;
    if !crate::store::load_sessions_index()
        .iter()
        .any(|session| session.id == session_id)
    {
        return Err("MEMORY_SOURCE_NOT_FOUND: session does not exist".into());
    }
    let source =
        verify_persisted_source(&request.source, &crate::store::load_messages(&session_id))?;
    create_pending_at(
        &store_path(),
        MemoryCandidateCreateRequestV1 { source, ..request },
        &crate::store::redact_text,
    )
}

fn create_pending_at(
    path: &Path,
    request: MemoryCandidateCreateRequestV1,
    redact: &impl Fn(&str) -> String,
) -> Result<MemoryCandidateV1, String> {
    let content = normalize_content(&request.content, redact)?;
    let source = MemoryCandidateSourceV1 {
        session_id: validate_source_id("session id", &request.source.session_id)?,
        message_id: validate_source_id("message id", &request.source.message_id)?,
    };
    let now = Utc::now();
    let candidate = MemoryCandidateV1 {
        id: uuid::Uuid::new_v4().to_string(),
        status: MemoryCandidateStatusV1::Pending,
        candidate_type: request.candidate_type,
        content_hash: content_hash(&content),
        content,
        source,
        ownership: MemoryCandidateOwnershipV1::HostCandidate,
        created_at: now,
        updated_at: now,
    };

    update_store_at(path, redact, |store| {
        if store.candidates.len() >= MAX_MEMORY_CANDIDATES {
            return Err(format!(
                "memory candidate store is full ({MAX_MEMORY_CANDIDATES} entries)"
            ));
        }
        store.candidates.push(candidate.clone());
        Ok(candidate)
    })
}

pub fn list() -> Result<Vec<MemoryCandidateV1>, String> {
    list_at(&store_path(), &crate::store::redact_text)
}

fn list_at(
    path: &Path,
    redact: &impl Fn(&str) -> String,
) -> Result<Vec<MemoryCandidateV1>, String> {
    let mut candidates = read_store_at(path, redact)?.candidates;
    candidates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(candidates)
}

fn validate_mutation_request(request: &MemoryCandidateMutationRequestV1) -> Result<(), String> {
    uuid::Uuid::parse_str(request.id.trim())
        .map_err(|_| "memory candidate id is invalid".to_string())?;
    if !valid_hash(request.expected_content_hash.trim()) {
        return Err("memory candidate expected content hash is invalid".into());
    }
    Ok(())
}

fn find_and_compare<'a>(
    store: &'a mut MemoryCandidateStoreV1,
    request: &MemoryCandidateMutationRequestV1,
) -> Result<&'a mut MemoryCandidateV1, String> {
    let candidate = store
        .candidates
        .iter_mut()
        .find(|candidate| candidate.id == request.id.trim())
        .ok_or_else(|| "memory candidate not found".to_string())?;
    if candidate.content_hash != request.expected_content_hash.trim() {
        return Err("STALE_MEMORY_CANDIDATE: content hash mismatch".into());
    }
    Ok(candidate)
}

fn transition_pending_at(
    path: &Path,
    request: MemoryCandidateMutationRequestV1,
    status: MemoryCandidateStatusV1,
    redact: &impl Fn(&str) -> String,
) -> Result<MemoryCandidateV1, String> {
    validate_mutation_request(&request)?;
    update_store_at(path, redact, |store| {
        let candidate = find_and_compare(store, &request)?;
        if candidate.status != MemoryCandidateStatusV1::Pending {
            return Err("memory candidate is no longer pending".into());
        }
        candidate.status = status;
        candidate.updated_at = Utc::now().max(candidate.created_at);
        Ok(candidate.clone())
    })
}

pub fn approve(request: MemoryCandidateMutationRequestV1) -> Result<MemoryCandidateV1, String> {
    transition_pending_at(
        &store_path(),
        request,
        MemoryCandidateStatusV1::Approved,
        &crate::store::redact_text,
    )
}

pub fn reject(request: MemoryCandidateMutationRequestV1) -> Result<MemoryCandidateV1, String> {
    transition_pending_at(
        &store_path(),
        request,
        MemoryCandidateStatusV1::Rejected,
        &crate::store::redact_text,
    )
}

pub fn supersede(request: MemoryCandidateMutationRequestV1) -> Result<MemoryCandidateV1, String> {
    supersede_at(&store_path(), request, &crate::store::redact_text)
}

fn supersede_at(
    path: &Path,
    request: MemoryCandidateMutationRequestV1,
    redact: &impl Fn(&str) -> String,
) -> Result<MemoryCandidateV1, String> {
    validate_mutation_request(&request)?;
    update_store_at(path, redact, |store| {
        let candidate = find_and_compare(store, &request)?;
        if !matches!(
            candidate.status,
            MemoryCandidateStatusV1::Pending | MemoryCandidateStatusV1::Approved
        ) {
            return Err("memory candidate cannot be superseded from its current status".into());
        }
        candidate.status = MemoryCandidateStatusV1::Superseded;
        candidate.updated_at = Utc::now().max(candidate.created_at);
        Ok(candidate.clone())
    })
}

pub fn delete(request: MemoryCandidateMutationRequestV1) -> Result<MemoryCandidateV1, String> {
    delete_at(&store_path(), request, &crate::store::redact_text)
}

fn delete_at(
    path: &Path,
    request: MemoryCandidateMutationRequestV1,
    redact: &impl Fn(&str) -> String,
) -> Result<MemoryCandidateV1, String> {
    validate_mutation_request(&request)?;
    update_store_at(path, redact, |store| {
        let index = store
            .candidates
            .iter()
            .position(|candidate| candidate.id == request.id.trim())
            .ok_or_else(|| "memory candidate not found".to_string())?;
        if store.candidates[index].content_hash != request.expected_content_hash.trim() {
            return Err("STALE_MEMORY_CANDIDATE: content hash mismatch".into());
        }
        Ok(store.candidates.remove(index))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    struct TestStore {
        root: PathBuf,
        path: PathBuf,
    }

    impl TestStore {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "sunsetz-memory-candidates-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&root).unwrap();
            let path = root.join("memory-candidates.v1.json");
            Self { root, path }
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn identity_redact(text: &str) -> String {
        text.to_string()
    }

    fn redacts_everything(_: &str) -> String {
        "[REDACTED]".into()
    }

    fn request(index: usize) -> MemoryCandidateCreateRequestV1 {
        MemoryCandidateCreateRequestV1 {
            candidate_type: MemoryCandidateTypeV1::UserPreference,
            content: format!("Prefer concise verification notes for workflow {index}."),
            source: MemoryCandidateSourceV1 {
                session_id: "session-1".into(),
                message_id: format!("message-{index}"),
            },
        }
    }

    fn mutation(candidate: &MemoryCandidateV1) -> MemoryCandidateMutationRequestV1 {
        MemoryCandidateMutationRequestV1 {
            id: candidate.id.clone(),
            expected_content_hash: candidate.content_hash.clone(),
        }
    }

    fn stored_candidate(index: usize) -> MemoryCandidateV1 {
        let content = format!("Safe bounded project fact {index}");
        let now = Utc::now();
        MemoryCandidateV1 {
            id: uuid::Uuid::new_v4().to_string(),
            status: MemoryCandidateStatusV1::Pending,
            candidate_type: MemoryCandidateTypeV1::ProjectFact,
            content_hash: content_hash(&content),
            content,
            source: MemoryCandidateSourceV1 {
                session_id: "session-seed".into(),
                message_id: format!("message-{index}"),
            },
            ownership: MemoryCandidateOwnershipV1::HostCandidate,
            created_at: now,
            updated_at: now,
        }
    }

    fn stored_message(id: &str, role: &str) -> crate::store::ChatMessageStored {
        crate::store::ChatMessageStored {
            id: id.into(),
            role: role.into(),
            content: "visible".into(),
            thought: None,
            created_at: Utc::now(),
            is_error: false,
            attachments: None,
            marker: None,
        }
    }

    #[test]
    fn create_is_pending_versioned_and_json_backed() {
        let store = TestStore::new("create");
        let candidate =
            create_pending_at(&store.path, request(1), &identity_redact).expect("create");

        assert_eq!(candidate.status, MemoryCandidateStatusV1::Pending);
        assert_eq!(
            candidate.ownership,
            MemoryCandidateOwnershipV1::HostCandidate
        );
        assert_eq!(candidate.content_hash, content_hash(&candidate.content));

        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(json["version"], MEMORY_CANDIDATE_STORE_VERSION);
        assert_eq!(json["candidates"][0]["type"], "user_preference");
        assert_eq!(json["candidates"][0]["ownership"], "host_candidate");
    }

    #[test]
    fn stale_approve_and_reject_fail_closed_without_mutation() {
        let store = TestStore::new("stale");
        let candidate =
            create_pending_at(&store.path, request(1), &identity_redact).expect("create");
        let stale = MemoryCandidateMutationRequestV1 {
            id: candidate.id.clone(),
            expected_content_hash: "0".repeat(64),
        };

        let approve_error = transition_pending_at(
            &store.path,
            stale.clone(),
            MemoryCandidateStatusV1::Approved,
            &identity_redact,
        )
        .unwrap_err();
        let reject_error = transition_pending_at(
            &store.path,
            stale,
            MemoryCandidateStatusV1::Rejected,
            &identity_redact,
        )
        .unwrap_err();

        assert!(approve_error.contains("STALE_MEMORY_CANDIDATE"));
        assert!(reject_error.contains("STALE_MEMORY_CANDIDATE"));
        assert_eq!(
            list_at(&store.path, &identity_redact).unwrap()[0].status,
            MemoryCandidateStatusV1::Pending
        );
    }

    #[test]
    fn approved_candidate_can_still_be_deleted() {
        let store = TestStore::new("delete-approved");
        let candidate =
            create_pending_at(&store.path, request(1), &identity_redact).expect("create");
        let approved = transition_pending_at(
            &store.path,
            mutation(&candidate),
            MemoryCandidateStatusV1::Approved,
            &identity_redact,
        )
        .expect("approve");
        assert_eq!(approved.status, MemoryCandidateStatusV1::Approved);

        let deleted =
            delete_at(&store.path, mutation(&approved), &identity_redact).expect("delete approved");
        assert_eq!(deleted.id, approved.id);
        assert!(list_at(&store.path, &identity_redact).unwrap().is_empty());
    }

    #[test]
    fn supersede_is_explicit_and_hash_guarded() {
        let store = TestStore::new("supersede");
        let candidate =
            create_pending_at(&store.path, request(1), &identity_redact).expect("create");
        let superseded =
            supersede_at(&store.path, mutation(&candidate), &identity_redact).expect("supersede");
        assert_eq!(superseded.status, MemoryCandidateStatusV1::Superseded);
        assert!(supersede_at(&store.path, mutation(&superseded), &identity_redact).is_err());
    }

    #[test]
    fn secrets_and_overlong_content_are_rejected_before_write() {
        let store = TestStore::new("secret");
        let mut secret = request(1);
        secret.content = "Authorization: Bearer live-secret-token".into();
        assert!(create_pending_at(&store.path, secret, &identity_redact).is_err());

        let mut private_key = request(2);
        private_key.content = "-----BEGIN OPENSSH PRIVATE KEY-----\nsecret".into();
        assert!(create_pending_at(&store.path, private_key, &identity_redact).is_err());

        let mut generic_token = request(3);
        generic_token.content = "token=live-secret-token".into();
        assert!(create_pending_at(&store.path, generic_token, &identity_redact).is_err());

        let mut known_secret = request(4);
        known_secret.content = "A value known by the App secret backend".into();
        assert!(create_pending_at(&store.path, known_secret, &redacts_everything).is_err());

        let mut overlong = request(5);
        overlong.content = "x".repeat(MAX_MEMORY_CONTENT_CHARS + 1);
        assert!(create_pending_at(&store.path, overlong, &identity_redact).is_err());

        for (index, content) in [
            "OPENAI_API_KEY=abcdefghijklmnopqrstuvwxyz123456",
            "SERVICE_TOKEN=abcdefghijklmnopqrstuvwxyz123456",
            "postgresql://user:password123@db.example.test/app",
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nsecret",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJlX3ZhbHVl",
        ]
        .into_iter()
        .enumerate()
        {
            let mut leaked = request(index + 10);
            leaked.content = content.into();
            assert!(
                create_pending_at(&store.path, leaked, &identity_redact).is_err(),
                "accepted sensitive case: {content}"
            );
        }
        assert!(!store.path.exists());
    }

    #[test]
    fn source_must_match_a_persisted_user_message() {
        let source = MemoryCandidateSourceV1 {
            session_id: "session-1".into(),
            message_id: "message-1".into(),
        };
        let messages = vec![
            stored_message("message-1", "user"),
            stored_message("assistant-1", "assistant"),
        ];
        assert_eq!(verify_persisted_source(&source, &messages).unwrap(), source);

        let unknown = MemoryCandidateSourceV1 {
            message_id: "missing".into(),
            ..source.clone()
        };
        assert!(verify_persisted_source(&unknown, &messages)
            .unwrap_err()
            .contains("MEMORY_SOURCE_NOT_FOUND"));
        let assistant = MemoryCandidateSourceV1 {
            message_id: "assistant-1".into(),
            ..source.clone()
        };
        assert!(verify_persisted_source(&assistant, &messages).is_err());
        let traversal = MemoryCandidateSourceV1 {
            session_id: "../sessions".into(),
            ..source
        };
        assert!(verify_persisted_source(&traversal, &messages).is_err());
    }

    #[test]
    fn store_cap_is_total_and_deletion_frees_capacity() {
        let store = TestStore::new("cap");
        let full = MemoryCandidateStoreV1 {
            version: MEMORY_CANDIDATE_STORE_VERSION,
            candidates: (0..MAX_MEMORY_CANDIDATES).map(stored_candidate).collect(),
        };
        let bytes = serde_json::to_vec_pretty(&full).unwrap();
        crate::store_lock::write_bytes_atomic(&store.path, &bytes).unwrap();

        let error = create_pending_at(&store.path, request(999), &identity_redact).unwrap_err();
        assert!(error.contains("store is full"));

        let first = full.candidates[0].clone();
        delete_at(&store.path, mutation(&first), &identity_redact).unwrap();
        create_pending_at(&store.path, request(1_000), &identity_redact).unwrap();
        assert_eq!(
            list_at(&store.path, &identity_redact).unwrap().len(),
            MAX_MEMORY_CANDIDATES
        );
    }

    #[test]
    fn concurrent_creates_do_not_lose_entries() {
        let store = TestStore::new("concurrent");
        let path = Arc::new(store.path.clone());
        let barrier = Arc::new(Barrier::new(9));
        let mut workers = Vec::new();

        for index in 0..8 {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                create_pending_at(&path, request(index), &identity_redact).unwrap();
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }

        assert_eq!(list_at(&path, &identity_redact).unwrap().len(), 8);
    }

    #[test]
    fn unsupported_store_version_is_not_overwritten() {
        let store = TestStore::new("version");
        crate::store_lock::write_bytes_atomic(&store.path, br#"{"version":2,"candidates":[]}"#)
            .unwrap();
        let before = fs::read(&store.path).unwrap();

        assert!(create_pending_at(&store.path, request(1), &identity_redact).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), before);
    }
}
