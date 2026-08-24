//! Review-only self-learning Skill candidates.
//!
//! Completing a tool-using task may create an App-owned pending draft. Nothing
//! under user, project, plugin, or external Skill roots is touched until the
//! user approves the candidate through the existing atomic Skill writer.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::skill_draft::{
    SkillDraftReference, SkillDraftSaveRequest, SkillDraftSaveResult, SkillDraftScope,
};

const MAX_PENDING_CANDIDATES: usize = 256;
const MAX_USER_CHARS: usize = 2_000;
const MAX_ASSISTANT_CHARS: usize = 8_000;
const MAX_AUDIT_EVENTS: usize = 32;
const MAX_OWNED_SKILL_FILES: usize = 64;
const MAX_OWNED_SKILL_BYTES: usize = 2 * 1024 * 1024;
const HOST_CANDIDATE_OWNER: &str = "host_candidate";
const LEGACY_HOST_CANDIDATE_OWNER: &str = "host_generated";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCandidateStatusV1 {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateDraftV1 {
    pub name: String,
    pub description: String,
    pub skill_md: String,
    #[serde(default)]
    pub references: Vec<SkillDraftReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateSourceV1 {
    pub session_id: String,
    pub session_title: String,
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateOwnerV1 {
    pub kind: String,
    pub namespace: String,
    pub may_overwrite_external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateAuditEventV2 {
    pub version: u8,
    pub candidate_id: String,
    pub status: SkillCandidateStatusV1,
    pub content_hash: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateV1 {
    pub version: u8,
    pub id: String,
    pub status: SkillCandidateStatusV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_content_hash: Option<String>,
    pub source: SkillCandidateSourceV1,
    pub owner: SkillCandidateOwnerV1,
    pub draft: SkillCandidateDraftV1,
    pub approved_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit_events: Vec<SkillCandidateAuditEventV2>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateApproveRequestV1 {
    pub id: String,
    pub scope: SkillDraftScope,
    pub project_path: Option<String>,
    pub draft: Option<SkillCandidateDraftV1>,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub user_confirmed_overwrite: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateApproveRequestV2 {
    pub id: String,
    pub expected_content_hash: String,
    #[serde(default)]
    pub final_content_hash: Option<String>,
    pub scope: SkillDraftScope,
    pub project_path: Option<String>,
    pub draft: Option<SkillCandidateDraftV1>,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub user_confirmed_overwrite: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateDecisionRequestV2 {
    pub id: String,
    pub expected_content_hash: String,
}

fn root() -> PathBuf {
    crate::paths::app_data_root()
        .join("skill-candidates")
        .join("v1")
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn candidate_path_at(candidates_root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("invalid skill candidate id".into());
    }
    Ok(candidates_root.join(format!("{id}.json")))
}

fn sha256(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

fn draft_content_hash(draft: &SkillCandidateDraftV1) -> Result<String, String> {
    let bytes =
        serde_json::to_vec(draft).map_err(|error| format!("hash skill candidate: {error}"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn normalize_review_draft(draft: &SkillCandidateDraftV1) -> Result<SkillCandidateDraftV1, String> {
    let references = draft
        .references
        .iter()
        .map(|reference| {
            Ok(SkillDraftReference {
                path: crate::skill_draft::normalized_reference_path(&reference.path)?,
                content: reference.content.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(SkillCandidateDraftV1 {
        name: draft.name.trim().to_string(),
        description: draft.description.trim().to_string(),
        skill_md: draft.skill_md.clone(),
        references,
    })
}

fn final_draft_content_hash(
    draft: &SkillCandidateDraftV1,
) -> Result<(SkillCandidateDraftV1, String), String> {
    let normalized = normalize_review_draft(draft)?;
    let hash = draft_content_hash(&normalized)?;
    Ok((normalized, hash))
}

fn valid_content_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn host_candidate_owner(owner: &SkillCandidateOwnerV1) -> bool {
    matches!(
        owner.kind.as_str(),
        HOST_CANDIDATE_OWNER | LEGACY_HOST_CANDIDATE_OWNER
    ) && owner.namespace == "sunsetz"
        && !owner.may_overwrite_external
}

fn require_host_candidate(candidate: &SkillCandidateV1) -> Result<(), String> {
    if host_candidate_owner(&candidate.owner) {
        Ok(())
    } else {
        Err("skill candidate ownership is not host_candidate".into())
    }
}

fn append_audit_event(
    candidate: &mut SkillCandidateV1,
    status: SkillCandidateStatusV1,
    content_hash: &str,
    occurred_at: DateTime<Utc>,
) {
    if candidate.audit_events.len() >= MAX_AUDIT_EVENTS {
        let remove = candidate.audit_events.len() + 1 - MAX_AUDIT_EVENTS;
        candidate.audit_events.drain(..remove);
    }
    candidate.audit_events.push(SkillCandidateAuditEventV2 {
        version: 2,
        candidate_id: candidate.id.clone(),
        status,
        content_hash: content_hash.to_string(),
        occurred_at,
    });
}

fn require_pending(candidate: &SkillCandidateV1) -> Result<(), String> {
    require_host_candidate(candidate)?;
    if !matches!(candidate.status, SkillCandidateStatusV1::Pending) {
        return Err("skill candidate is no longer pending".into());
    }
    let actual_hash = draft_content_hash(&candidate.draft)?;
    if candidate
        .review_content_hash
        .as_deref()
        .is_some_and(|stored| stored != actual_hash)
    {
        return Err("skill candidate content hash is invalid".into());
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct ApprovalIntent {
    draft: SkillCandidateDraftV1,
    final_content_hash: String,
}

fn approval_intent(
    candidate: &SkillCandidateV1,
    request: &SkillCandidateApproveRequestV2,
) -> Result<ApprovalIntent, String> {
    require_pending(candidate)?;
    let current_review_hash = draft_content_hash(&candidate.draft)?;
    let expected = request.expected_content_hash.trim();
    if !valid_content_hash(expected) || expected != current_review_hash {
        return Err("stale skill candidate content hash".into());
    }
    if request.overwrite && !request.user_confirmed_overwrite {
        return Err("explicit overwrite confirmation is required".into());
    }

    let (_, stored_final_hash) = final_draft_content_hash(&candidate.draft)?;
    let requested = request.draft.as_ref().unwrap_or(&candidate.draft);
    let (draft, final_content_hash) = final_draft_content_hash(requested)?;
    match request.final_content_hash.as_deref().map(str::trim) {
        Some(supplied)
            if !valid_content_hash(supplied) || supplied != final_content_hash.as_str() =>
        {
            return Err("stale final skill candidate content hash".into());
        }
        None if final_content_hash != stored_final_hash => {
            return Err("finalContentHash is required for an edited skill candidate draft".into());
        }
        _ => {}
    }
    Ok(ApprovalIntent {
        draft,
        final_content_hash,
    })
}

fn hash_skill_tree(root: &Path) -> Result<String, String> {
    fn collect(
        root: &Path,
        current: &Path,
        files: &mut Vec<(String, Vec<u8>)>,
        total_bytes: &mut usize,
    ) -> Result<(), String> {
        let metadata = fs::symlink_metadata(current)
            .map_err(|error| format!("inspect Skill ownership: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("cannot prove ownership through a symbolic link".into());
        }
        if metadata.is_file() {
            if files.len() >= MAX_OWNED_SKILL_FILES {
                return Err("owned Skill contains too many files".into());
            }
            let bytes = fs::read(current)
                .map_err(|error| format!("read Skill ownership content: {error}"))?;
            *total_bytes = total_bytes.saturating_add(bytes.len());
            if *total_bytes > MAX_OWNED_SKILL_BYTES {
                return Err("owned Skill is too large to verify".into());
            }
            let relative = current
                .strip_prefix(root)
                .map_err(|_| "invalid Skill ownership path".to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, bytes));
            return Ok(());
        }
        if !metadata.is_dir() {
            return Err("owned Skill contains an unsupported filesystem entry".into());
        }
        for entry in fs::read_dir(current)
            .map_err(|error| format!("list Skill ownership content: {error}"))?
        {
            let entry = entry.map_err(|error| format!("list Skill ownership content: {error}"))?;
            collect(root, &entry.path(), files, total_bytes)?;
        }
        Ok(())
    }

    let mut files = Vec::new();
    let mut total_bytes = 0;
    collect(root, root, &mut files, &mut total_bytes)?;
    hash_skill_files(files)
}

fn hash_skill_files(mut files: Vec<(String, Vec<u8>)>) -> Result<String, String> {
    if files.len() > MAX_OWNED_SKILL_FILES {
        return Err("owned Skill contains too many files".into());
    }
    if files
        .iter()
        .try_fold(0_usize, |total, (_, bytes)| total.checked_add(bytes.len()))
        .is_none_or(|total| total > MAX_OWNED_SKILL_BYTES)
    {
        return Err("owned Skill is too large to verify".into());
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn intended_skill_tree_hash(draft: &SkillCandidateDraftV1) -> Result<String, String> {
    let mut files = Vec::with_capacity(draft.references.len() + 1);
    files.push(("SKILL.md".to_string(), draft.skill_md.as_bytes().to_vec()));
    files.extend(draft.references.iter().map(|reference| {
        (
            reference.path.replace('\\', "/"),
            reference.content.as_bytes().to_vec(),
        )
    }));
    hash_skill_files(files)
}

fn require_owned_overwrite(candidates_root: &Path, target: &Path) -> Result<(), String> {
    let target = target
        .canonicalize()
        .map_err(|error| format!("resolve existing Skill target: {error}"))?;
    let current_hash = hash_skill_tree(&target)?;
    // Reuse the store's bounded reader. Missing, malformed, or out-of-bound
    // ownership evidence denies the overwrite.
    for candidate in list_at(candidates_root)? {
        if !matches!(candidate.status, SkillCandidateStatusV1::Approved)
            || !host_candidate_owner(&candidate.owner)
            || candidate.approved_content_hash.as_deref() != Some(current_hash.as_str())
        {
            continue;
        }
        let Some(approved_path) = candidate.approved_path.as_deref() else {
            continue;
        };
        if Path::new(approved_path)
            .canonicalize()
            .is_ok_and(|path| path == target)
        {
            return Ok(());
        }
    }
    Err("refusing to overwrite a Skill not proven to be host_candidate-owned".into())
}

fn bounded(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

fn contains_sensitive_material(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "authorization: bearer ",
        "github_pat_",
        "ghp_",
        "sk-proj-",
        "xoxb-",
        "refresh_token",
        "client_secret",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn load_path(path: &Path) -> Result<SkillCandidateV1, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read skill candidate: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse skill candidate: {e}"))
}

pub fn create_for_session(session_id: &str) -> Result<Option<SkillCandidateV1>, String> {
    let messages = crate::store::load_messages(session_id);
    create_from_messages_at(session_id, &messages, None, &root())
}

fn create_from_messages_at(
    session_id: &str,
    messages: &[crate::store::ChatMessageStored],
    session_title: Option<String>,
    candidates_root: &Path,
) -> Result<Option<SkillCandidateV1>, String> {
    let Some(user_index) = messages.iter().rposition(|message| message.role == "user") else {
        return Ok(None);
    };
    let user = &messages[user_index];
    let Some(assistant) = messages[user_index + 1..]
        .iter()
        .rev()
        .find(|message| message.role == "assistant" && !message.content.trim().is_empty())
    else {
        return Ok(None);
    };
    if assistant.content.chars().count() < 160 || user.content.trim().is_empty() {
        return Ok(None);
    }

    let user_text = bounded(&crate::store::redact_text(&user.content), MAX_USER_CHARS);
    let assistant_text = bounded(
        &crate::store::redact_text(&assistant.content),
        MAX_ASSISTANT_CHARS,
    );
    if contains_sensitive_material(&user_text) || contains_sensitive_material(&assistant_text) {
        return Ok(None);
    }
    let source_hash = sha256(&[
        session_id,
        &user.id,
        &assistant.id,
        &user_text,
        &assistant_text,
    ]);
    let id = source_hash[..32].to_string();
    fs::create_dir_all(candidates_root).map_err(|e| format!("create skill candidates: {e}"))?;
    if list_at(candidates_root)?
        .iter()
        .filter(|item| matches!(item.status, SkillCandidateStatusV1::Pending))
        .count()
        >= MAX_PENDING_CANDIDATES
    {
        return Ok(None);
    }
    let session_title = session_title.unwrap_or_else(|| {
        crate::store::load_sessions_index()
            .into_iter()
            .find(|session| session.id == session_id)
            .map(|session| session.title)
            .unwrap_or_default()
    });
    let name = format!("learned-{}", &source_hash[..10]);
    let description =
        "Review and reuse a workflow learned from a completed local task.".to_string();
    let skill_md = format!(
        "---\nname: {name}\ndescription: {description}\n---\n\n# Review-required workflow candidate\n\nThis draft was generated from visible task output. Verify and generalize every step before enabling it.\n\n## Source request\n\n{user_text}\n\n## Observed outcome\n\n{assistant_text}\n\n## Review checklist\n\n- Remove task-specific paths and identifiers.\n- Confirm prerequisites, failure modes, and safe defaults.\n- Keep secrets, hidden reasoning, and raw tool payloads out of the Skill.\n"
    );
    let draft = SkillCandidateDraftV1 {
        name,
        description,
        skill_md,
        references: Vec::new(),
    };
    let review_content_hash = draft_content_hash(&draft)?;
    let now = Utc::now();
    let mut candidate = SkillCandidateV1 {
        version: 1,
        id: id.clone(),
        status: SkillCandidateStatusV1::Pending,
        created_at: now,
        updated_at: now,
        // V1 used this field as the source-transcript hash. Keep that meaning
        // so already-persisted candidates and callers remain compatible.
        content_hash: source_hash,
        review_content_hash: Some(review_content_hash.clone()),
        source: SkillCandidateSourceV1 {
            session_id: session_id.to_string(),
            session_title,
            message_ids: vec![user.id.clone(), assistant.id.clone()],
        },
        owner: SkillCandidateOwnerV1 {
            kind: HOST_CANDIDATE_OWNER.into(),
            namespace: "sunsetz".into(),
            may_overwrite_external: false,
        },
        draft,
        approved_path: None,
        approved_content_hash: None,
        audit_events: Vec::new(),
    };
    append_audit_event(
        &mut candidate,
        SkillCandidateStatusV1::Pending,
        &review_content_hash,
        now,
    );
    let path = candidate_path_at(candidates_root, &id)?;
    let existing = crate::store_lock::update_json_locked(
        &path,
        || candidate.clone(),
        |stored: &mut SkillCandidateV1| Ok(stored.clone()),
    )?;
    Ok(Some(existing))
}

pub fn list() -> Result<Vec<SkillCandidateV1>, String> {
    list_at(&root())
}

fn list_at(candidates_root: &Path) -> Result<Vec<SkillCandidateV1>, String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(candidates_root) else {
        return Ok(out);
    };
    for entry in entries.flatten().take(MAX_PENDING_CANDIDATES * 2) {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        if let Ok(candidate) = load_path(&path) {
            out.push(candidate);
        }
    }
    out.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(out)
}

pub fn approve(request: SkillCandidateApproveRequestV1) -> Result<SkillDraftSaveResult, String> {
    approve_at(request, &root())
}

fn approve_at(
    request: SkillCandidateApproveRequestV1,
    candidates_root: &Path,
) -> Result<SkillDraftSaveResult, String> {
    let path = candidate_path_at(candidates_root, request.id.trim())?;
    let candidate = load_path(&path)?;
    let expected_content_hash = candidate
        .review_content_hash
        .unwrap_or(draft_content_hash(&candidate.draft)?);
    approve_v2_at(
        SkillCandidateApproveRequestV2 {
            id: request.id,
            expected_content_hash,
            final_content_hash: None,
            scope: request.scope,
            project_path: request.project_path,
            draft: request.draft,
            overwrite: request.overwrite,
            user_confirmed_overwrite: request.user_confirmed_overwrite,
        },
        candidates_root,
    )
}

pub fn approve_v2(request: SkillCandidateApproveRequestV2) -> Result<SkillDraftSaveResult, String> {
    approve_v2_at(request, &root())
}

fn approve_v2_at(
    request: SkillCandidateApproveRequestV2,
    candidates_root: &Path,
) -> Result<SkillDraftSaveResult, String> {
    approve_v2_at_with_commit_hook(request, candidates_root, || Ok(()))
}

fn committed_approval_matches(
    candidate: &SkillCandidateV1,
    result: &SkillDraftSaveResult,
    final_content_hash: &str,
    approved_content_hash: &str,
) -> bool {
    matches!(candidate.status, SkillCandidateStatusV1::Approved)
        && host_candidate_owner(&candidate.owner)
        && candidate.approved_path.as_deref() == Some(result.path.as_str())
        && candidate.approved_content_hash.as_deref() == Some(approved_content_hash)
        && candidate.review_content_hash.as_deref() == Some(final_content_hash)
        && draft_content_hash(&candidate.draft).as_deref() == Ok(final_content_hash)
}

fn persist_approved_candidate(
    path: &Path,
    fallback: &SkillCandidateV1,
    request: &SkillCandidateApproveRequestV2,
    intended: &ApprovalIntent,
    result: &SkillDraftSaveResult,
    approved_content_hash: &str,
) -> Result<SkillCandidateV1, String> {
    let update = crate::store_lock::update_json_locked(
        path,
        || fallback.clone(),
        |stored: &mut SkillCandidateV1| {
            let current = approval_intent(stored, request)?;
            if current.final_content_hash != intended.final_content_hash
                || draft_content_hash(&current.draft)? != draft_content_hash(&intended.draft)?
            {
                return Err("stale final skill candidate content hash".into());
            }
            let now = Utc::now();
            stored.status = SkillCandidateStatusV1::Approved;
            stored.updated_at = now;
            stored.owner.kind = HOST_CANDIDATE_OWNER.into();
            stored.review_content_hash = Some(intended.final_content_hash.clone());
            stored.draft = intended.draft.clone();
            stored.approved_path = Some(result.path.clone());
            stored.approved_content_hash = Some(approved_content_hash.to_string());
            append_audit_event(
                stored,
                SkillCandidateStatusV1::Approved,
                &intended.final_content_hash,
                now,
            );
            Ok(stored.clone())
        },
    );
    match update {
        Ok(candidate) => Ok(candidate),
        Err(error) => {
            // Atomic replacement can become visible before the parent-directory
            // fsync reports an error. Treat that exact terminal record as the
            // committed side of the transaction; every other state rolls back.
            if let Ok(candidate) = load_path(path) {
                if committed_approval_matches(
                    &candidate,
                    result,
                    &intended.final_content_hash,
                    approved_content_hash,
                ) {
                    return Ok(candidate);
                }
            }
            Err(error)
        }
    }
}

fn approve_v2_at_with_commit_hook(
    request: SkillCandidateApproveRequestV2,
    candidates_root: &Path,
    before_candidate_commit: impl FnOnce() -> Result<(), String>,
) -> Result<SkillDraftSaveResult, String> {
    let id = request.id.trim().to_string();
    let path = candidate_path_at(candidates_root, &id)?;
    let fallback = load_path(&path)?;
    let intended = approval_intent(&fallback, &request)?;
    let intended_tree_hash = intended_skill_tree_hash(&intended.draft)?;
    let save_request = SkillDraftSaveRequest {
        name: intended.draft.name.clone(),
        description: intended.draft.description.clone(),
        skill_md: intended.draft.skill_md.clone(),
        references: intended.draft.references.clone(),
        scope: request.scope.clone(),
        project_path: request.project_path.clone(),
        overwrite: request.overwrite,
    };
    let (result, ()) = crate::skill_draft::save_with_target_transaction(
        save_request,
        |target, had_existing| {
            if had_existing {
                require_owned_overwrite(candidates_root, target)?;
            }
            Ok(())
        },
        |result| {
            before_candidate_commit()?;
            let approved_content_hash = hash_skill_tree(Path::new(&result.path))?;
            if approved_content_hash != intended_tree_hash {
                return Err("Skill target content changed before candidate commit".into());
            }
            persist_approved_candidate(
                &path,
                &fallback,
                &request,
                &intended,
                result,
                &approved_content_hash,
            )?;
            Ok(())
        },
    )?;
    Ok(result)
}

pub fn reject(id: &str) -> Result<SkillCandidateV1, String> {
    reject_at(id, &root())
}

fn reject_at(id: &str, candidates_root: &Path) -> Result<SkillCandidateV1, String> {
    transition_compat_at(id, SkillCandidateStatusV1::Rejected, candidates_root)
}

pub fn reject_v2(request: SkillCandidateDecisionRequestV2) -> Result<SkillCandidateV1, String> {
    reject_v2_at(request, &root())
}

fn reject_v2_at(
    request: SkillCandidateDecisionRequestV2,
    candidates_root: &Path,
) -> Result<SkillCandidateV1, String> {
    transition_v2_at(request, SkillCandidateStatusV1::Rejected, candidates_root)
}

pub fn cancel(id: &str) -> Result<SkillCandidateV1, String> {
    cancel_at(id, &root())
}

fn cancel_at(id: &str, candidates_root: &Path) -> Result<SkillCandidateV1, String> {
    transition_compat_at(id, SkillCandidateStatusV1::Cancelled, candidates_root)
}

pub fn cancel_v2(request: SkillCandidateDecisionRequestV2) -> Result<SkillCandidateV1, String> {
    cancel_v2_at(request, &root())
}

fn cancel_v2_at(
    request: SkillCandidateDecisionRequestV2,
    candidates_root: &Path,
) -> Result<SkillCandidateV1, String> {
    transition_v2_at(request, SkillCandidateStatusV1::Cancelled, candidates_root)
}

fn transition_compat_at(
    id: &str,
    next_status: SkillCandidateStatusV1,
    candidates_root: &Path,
) -> Result<SkillCandidateV1, String> {
    let path = candidate_path_at(candidates_root, id.trim())?;
    let candidate = load_path(&path)?;
    let expected_content_hash = candidate
        .review_content_hash
        .unwrap_or(draft_content_hash(&candidate.draft)?);
    transition_v2_at(
        SkillCandidateDecisionRequestV2 {
            id: id.to_string(),
            expected_content_hash,
        },
        next_status,
        candidates_root,
    )
}

fn transition_v2_at(
    request: SkillCandidateDecisionRequestV2,
    next_status: SkillCandidateStatusV1,
    candidates_root: &Path,
) -> Result<SkillCandidateV1, String> {
    let path = candidate_path_at(candidates_root, request.id.trim())?;
    let fallback = load_path(&path)?;
    crate::store_lock::update_json_locked(
        &path,
        || fallback.clone(),
        |stored: &mut SkillCandidateV1| {
            require_pending(stored)?;
            let review_content_hash = draft_content_hash(&stored.draft)?;
            let expected = request.expected_content_hash.trim();
            if !valid_content_hash(expected) || expected != review_content_hash {
                return Err("stale skill candidate content hash".into());
            }
            let now = Utc::now();
            stored.status = next_status.clone();
            stored.updated_at = now;
            stored.owner.kind = HOST_CANDIDATE_OWNER.into();
            stored.review_content_hash = Some(review_content_hash.clone());
            append_audit_event(stored, next_status, &review_content_hash, now);
            Ok(stored.clone())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ChatMessageStored;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sunsetz-skill-candidate-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn message(id: &str, role: &str, content: impl Into<String>) -> ChatMessageStored {
        ChatMessageStored {
            id: id.into(),
            role: role.into(),
            content: content.into(),
            thought: None,
            created_at: Utc::now(),
            is_error: false,
            attachments: None,
            marker: None,
        }
    }

    fn eligible_messages(suffix: &str) -> Vec<ChatMessageStored> {
        vec![
            message(
                &format!("user-{suffix}"),
                "user",
                format!("Turn this verified workflow into a reusable review draft {suffix}"),
            ),
            message("tool-row", "tool", "visible tool output"),
            message(
                &format!("assistant-{suffix}"),
                "assistant",
                format!(
                    "Completed the local workflow safely. {} {suffix}",
                    "The result includes concrete prerequisites, validation steps, failure handling, and a reproducible verification checklist. ".repeat(3)
                ),
            ),
        ]
    }

    fn create_candidate(label: &str, candidates_root: &Path) -> SkillCandidateV1 {
        create_from_messages_at(
            &format!("session-{label}"),
            &eligible_messages(label),
            Some(format!("Candidate {label}")),
            candidates_root,
        )
        .unwrap()
        .expect("eligible transcript should create a candidate")
    }

    fn project_approval_v2(
        candidate: &SkillCandidateV1,
        project_root: &Path,
    ) -> SkillCandidateApproveRequestV2 {
        SkillCandidateApproveRequestV2 {
            id: candidate.id.clone(),
            expected_content_hash: candidate
                .review_content_hash
                .clone()
                .unwrap_or_else(|| draft_content_hash(&candidate.draft).unwrap()),
            final_content_hash: None,
            scope: SkillDraftScope::Project,
            project_path: Some(project_root.to_string_lossy().into_owned()),
            draft: None,
            overwrite: false,
            user_confirmed_overwrite: false,
        }
    }

    fn replace_candidate(candidate: &SkillCandidateV1, candidates_root: &Path) {
        let path = candidate_path_at(candidates_root, &candidate.id).unwrap();
        crate::store_lock::update_json_locked(
            &path,
            || candidate.clone(),
            |stored: &mut SkillCandidateV1| {
                *stored = candidate.clone();
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn candidate_ids_cannot_escape_store() {
        assert!(valid_id("0123456789abcdef0123456789abcdef"));
        assert!(!valid_id("../../settings"));
    }

    #[test]
    fn content_hash_is_stable_and_source_sensitive() {
        assert_eq!(sha256(&["a", "b"]), sha256(&["a", "b"]));
        assert_ne!(sha256(&["a", "b"]), sha256(&["a", "c"]));
    }

    #[test]
    fn final_content_hash_has_a_stable_normalized_test_vector() {
        let draft = SkillCandidateDraftV1 {
            name: " Review Helper ".into(),
            description: "\nSafe.\t".into(),
            skill_md: "line\n".into(),
            references: vec![SkillDraftReference {
                path: " references//guide.md ".into(),
                content: "x".into(),
            }],
        };
        let (normalized, hash) = final_draft_content_hash(&draft).unwrap();
        assert_eq!(normalized.name, "Review Helper");
        assert_eq!(normalized.description, "Safe.");
        assert_eq!(normalized.references[0].path, "references/guide.md");
        assert_eq!(
            hash,
            "d28be6e7bf6675cfd275474932a9282229903874412b50d165fcdbdb62a2b861"
        );
    }

    #[test]
    fn candidate_generation_is_review_only_and_idempotent() {
        let candidates_root = temp_dir("create");
        let messages = eligible_messages("one");

        let candidate = create_from_messages_at(
            "session-one",
            &messages,
            Some("Migration session".into()),
            &candidates_root,
        )
        .unwrap()
        .expect("eligible transcript should create a candidate");

        assert_eq!(candidate.version, 1);
        assert!(matches!(candidate.status, SkillCandidateStatusV1::Pending));
        assert_eq!(candidate.source.session_id, "session-one");
        assert_eq!(candidate.source.session_title, "Migration session");
        assert_eq!(
            candidate.source.message_ids,
            vec!["user-one", "assistant-one"]
        );
        assert_eq!(candidate.owner.kind, HOST_CANDIDATE_OWNER);
        assert_eq!(candidate.owner.namespace, "sunsetz");
        assert!(!candidate.owner.may_overwrite_external);
        assert!(candidate
            .draft
            .skill_md
            .contains("Review-required workflow candidate"));
        assert!(candidate.draft.skill_md.contains("Source request"));
        assert!(candidate.approved_path.is_none());
        assert!(candidate.approved_content_hash.is_none());
        assert_eq!(candidate.id, candidate.content_hash[..32]);
        assert_eq!(
            candidate.review_content_hash.as_deref(),
            Some(draft_content_hash(&candidate.draft).unwrap().as_str())
        );
        assert_eq!(candidate.audit_events.len(), 1);
        assert_eq!(
            candidate.audit_events[0].status,
            SkillCandidateStatusV1::Pending
        );
        assert!(candidate_path_at(&candidates_root, &candidate.id)
            .unwrap()
            .is_file());

        let repeated = create_from_messages_at(
            "session-one",
            &messages,
            Some("Renamed session".into()),
            &candidates_root,
        )
        .unwrap()
        .unwrap();
        assert_eq!(repeated.id, candidate.id);
        assert_eq!(repeated.source.session_title, "Migration session");
        assert_eq!(list_at(&candidates_root).unwrap().len(), 1);

        fs::remove_dir_all(candidates_root).unwrap();
    }

    #[test]
    fn candidate_generation_filters_incomplete_transcripts() {
        let candidates_root = temp_dir("filters");
        assert!(
            create_from_messages_at("s", &[], Some(String::new()), &candidates_root)
                .unwrap()
                .is_none()
        );

        let no_assistant = vec![message("u", "user", "do something")];
        assert!(
            create_from_messages_at("s", &no_assistant, Some(String::new()), &candidates_root)
                .unwrap()
                .is_none()
        );

        let short = vec![
            message("u", "user", "do something"),
            message("a", "assistant", "done"),
        ];
        assert!(
            create_from_messages_at("s", &short, Some(String::new()), &candidates_root)
                .unwrap()
                .is_none()
        );

        let empty_user = vec![
            message("u", "user", "   "),
            message("a", "assistant", "x".repeat(200)),
        ];
        assert!(
            create_from_messages_at("s", &empty_user, Some(String::new()), &candidates_root)
                .unwrap()
                .is_none()
        );

        assert!(contains_sensitive_material("Authorization: Bearer abc"));
        assert!(contains_sensitive_material("client_secret=value"));
        assert!(!contains_sensitive_material(
            "ordinary reusable instructions"
        ));
        assert_eq!(bounded("你好世界", 2), "你好…");
        assert_eq!(bounded("short", 10), "short");
        assert!(list_at(&candidates_root).unwrap().is_empty());

        fs::remove_dir_all(candidates_root).unwrap();
    }

    #[test]
    fn candidate_rejection_is_persisted() {
        let candidates_root = temp_dir("reject");
        let candidate = create_from_messages_at(
            "session-reject",
            &eligible_messages("reject"),
            Some(String::new()),
            &candidates_root,
        )
        .unwrap()
        .unwrap();

        let rejected = reject_at(&candidate.id, &candidates_root).unwrap();
        assert!(matches!(rejected.status, SkillCandidateStatusV1::Rejected));
        let loaded =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert!(matches!(loaded.status, SkillCandidateStatusV1::Rejected));
        assert_eq!(loaded.audit_events.len(), 2);
        assert_eq!(
            loaded.audit_events[1].status,
            SkillCandidateStatusV1::Rejected
        );
        assert!(reject_at(&candidate.id, &candidates_root)
            .unwrap_err()
            .contains("no longer pending"));
        assert!(cancel_at(&candidate.id, &candidates_root)
            .unwrap_err()
            .contains("no longer pending"));
        assert!(reject_at("../../settings", &candidates_root).is_err());

        fs::remove_dir_all(candidates_root).unwrap();
    }

    #[test]
    fn candidate_approval_requires_confirmation_and_writes_project_skill() {
        let candidates_root = temp_dir("approve");
        let project_root = temp_dir("project");
        let candidate = create_from_messages_at(
            "session-approve",
            &eligible_messages("approve"),
            Some("Approval".into()),
            &candidates_root,
        )
        .unwrap()
        .unwrap();

        let unconfirmed = SkillCandidateApproveRequestV1 {
            id: candidate.id.clone(),
            scope: SkillDraftScope::Project,
            project_path: Some(project_root.to_string_lossy().into_owned()),
            draft: None,
            overwrite: true,
            user_confirmed_overwrite: false,
        };
        assert!(approve_at(unconfirmed, &candidates_root)
            .unwrap_err()
            .contains("explicit overwrite confirmation"));

        let result = approve_at(
            SkillCandidateApproveRequestV1 {
                id: candidate.id.clone(),
                scope: SkillDraftScope::Project,
                project_path: Some(project_root.to_string_lossy().into_owned()),
                draft: None,
                overwrite: false,
                user_confirmed_overwrite: false,
            },
            &candidates_root,
        )
        .unwrap();
        assert_eq!(result.scope, "project");
        assert!(!result.overwritten);
        assert!(PathBuf::from(&result.path).join("SKILL.md").is_file());

        let stored =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert!(matches!(stored.status, SkillCandidateStatusV1::Approved));
        assert_eq!(stored.approved_path.as_deref(), Some(result.path.as_str()));
        assert!(stored.approved_content_hash.is_some());
        assert_eq!(stored.content_hash, candidate.content_hash);
        assert_eq!(stored.audit_events.len(), 2);
        assert_eq!(
            stored.audit_events[1].status,
            SkillCandidateStatusV1::Approved
        );
        assert!(approve_at(
            SkillCandidateApproveRequestV1 {
                id: candidate.id,
                scope: SkillDraftScope::Project,
                project_path: Some(project_root.to_string_lossy().into_owned()),
                draft: None,
                overwrite: false,
                user_confirmed_overwrite: false,
            },
            &candidates_root,
        )
        .unwrap_err()
        .contains("no longer pending"));

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn v2_approval_rejects_a_stale_review_hash_without_writing() {
        let candidates_root = temp_dir("stale-hash");
        let project_root = temp_dir("stale-hash-project");
        let candidate = create_candidate("stale-hash", &candidates_root);
        let mut request = project_approval_v2(&candidate, &project_root);
        request.expected_content_hash = "0".repeat(64);

        assert!(approve_v2_at(request, &candidates_root)
            .unwrap_err()
            .contains("stale skill candidate content hash"));
        let stale_decision = SkillCandidateDecisionRequestV2 {
            id: candidate.id.clone(),
            expected_content_hash: "f".repeat(64),
        };
        assert!(reject_v2_at(stale_decision.clone(), &candidates_root)
            .unwrap_err()
            .contains("stale skill candidate content hash"));
        assert!(cancel_v2_at(stale_decision, &candidates_root)
            .unwrap_err()
            .contains("stale skill candidate content hash"));
        let stored =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert_eq!(stored.status, SkillCandidateStatusV1::Pending);
        assert_eq!(stored.audit_events.len(), 1);
        assert!(!project_root.join(".grok").exists());

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn edited_draft_requires_its_own_matching_final_hash() {
        let candidates_root = temp_dir("final-hash");
        let project_root = temp_dir("final-hash-project");
        let candidate = create_candidate("final-hash", &candidates_root);
        let mut edited = candidate.draft.clone();
        edited.skill_md.push_str("\nReviewed edit.\n");
        let (_, edited_hash) = final_draft_content_hash(&edited).unwrap();

        let mut missing = project_approval_v2(&candidate, &project_root);
        missing.draft = Some(edited.clone());
        assert!(approve_v2_at(missing, &candidates_root)
            .unwrap_err()
            .contains("finalContentHash is required"));

        let mut stale = project_approval_v2(&candidate, &project_root);
        stale.draft = Some(edited.clone());
        stale.final_content_hash = Some(candidate.review_content_hash.clone().unwrap());
        assert!(approve_v2_at(stale, &candidates_root)
            .unwrap_err()
            .contains("stale final skill candidate content hash"));

        let mut approved = project_approval_v2(&candidate, &project_root);
        approved.draft = Some(edited);
        approved.final_content_hash = Some(edited_hash.clone());
        let result = approve_v2_at(approved, &candidates_root).unwrap();
        assert!(fs::read_to_string(Path::new(&result.path).join("SKILL.md"))
            .unwrap()
            .contains("Reviewed edit."));
        let stored =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert_eq!(
            stored.review_content_hash.as_deref(),
            Some(edited_hash.as_str())
        );

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn candidate_commit_failure_rolls_back_new_target_and_can_retry() {
        let candidates_root = temp_dir("commit-rollback-new");
        let project_root = temp_dir("commit-rollback-new-project");
        let candidate = create_candidate("commit-rollback-new", &candidates_root);
        let request = project_approval_v2(&candidate, &project_root);
        let target = project_root
            .join(".grok")
            .join("skills")
            .join(format!("{}-skill", candidate.draft.name));
        let target_seen_by_hook = target.clone();

        let error = approve_v2_at_with_commit_hook(request.clone(), &candidates_root, || {
            assert!(target_seen_by_hook.join("SKILL.md").is_file());
            Err("INJECTED_CANDIDATE_COMMIT_FAILURE".into())
        })
        .unwrap_err();
        assert!(error.contains("INJECTED_CANDIDATE_COMMIT_FAILURE"));
        assert!(!target.exists());
        let pending =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert_eq!(pending.status, SkillCandidateStatusV1::Pending);
        assert_eq!(pending.audit_events.len(), 1);

        let result = approve_v2_at(request, &candidates_root).unwrap();
        assert!(Path::new(&result.path).join("SKILL.md").is_file());

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn external_change_after_swap_is_not_approved_and_is_quarantined() {
        use std::io::Write;

        let candidates_root = temp_dir("post-swap-external-change");
        let project_root = temp_dir("post-swap-external-change-project");
        let candidate = create_candidate("post-swap-external-change", &candidates_root);
        let request = project_approval_v2(&candidate, &project_root);
        let target = project_root
            .join(".grok")
            .join("skills")
            .join(format!("{}-skill", candidate.draft.name));
        let target_seen_by_hook = target.clone();

        let error = approve_v2_at_with_commit_hook(request, &candidates_root, move || {
            fs::OpenOptions::new()
                .append(true)
                .open(target_seen_by_hook.join("SKILL.md"))
                .unwrap()
                .write_all(b"\nExternal post-swap change.\n")
                .unwrap();
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("Skill target content changed before candidate commit"));
        assert!(error.contains("SKILL_ROLLBACK_CONFLICT"));
        assert!(!target.exists());
        let quarantine = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.contains(".conflict-"))
            })
            .expect("externally changed target should be quarantined");
        assert!(fs::read_to_string(quarantine.join("SKILL.md"))
            .unwrap()
            .contains("External post-swap change."));
        let pending =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert_eq!(pending.status, SkillCandidateStatusV1::Pending);

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn cancel_is_terminal_and_competing_decisions_are_compare_and_set() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let candidates_root = temp_dir("decision-cas");
        let candidate = create_candidate("decision-cas", &candidates_root);
        let barrier = Arc::new(Barrier::new(3));
        let expected_content_hash = candidate.review_content_hash.clone().unwrap();
        let mut decisions = Vec::new();
        for cancel in [false, true] {
            let barrier = Arc::clone(&barrier);
            let root = candidates_root.clone();
            let id = candidate.id.clone();
            let expected_content_hash = expected_content_hash.clone();
            decisions.push(thread::spawn(move || {
                barrier.wait();
                let request = SkillCandidateDecisionRequestV2 {
                    id,
                    expected_content_hash,
                };
                if cancel {
                    cancel_v2_at(request, &root)
                } else {
                    reject_v2_at(request, &root)
                }
            }));
        }
        barrier.wait();
        let outcomes: Vec<_> = decisions
            .into_iter()
            .map(|decision| decision.join().unwrap())
            .collect();
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_err()).count(),
            1
        );
        assert!(outcomes
            .iter()
            .filter_map(|outcome| outcome.as_ref().err())
            .all(|error| error.contains("no longer pending")));

        let stored =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert!(matches!(
            stored.status,
            SkillCandidateStatusV1::Rejected | SkillCandidateStatusV1::Cancelled
        ));
        assert_eq!(stored.audit_events.len(), 2);
        assert!(cancel_at(&candidate.id, &candidates_root)
            .unwrap_err()
            .contains("no longer pending"));
        assert!(reject_at(&candidate.id, &candidates_root)
            .unwrap_err()
            .contains("no longer pending"));

        fs::remove_dir_all(candidates_root).unwrap();
    }

    #[test]
    fn candidate_decisions_reject_non_host_ownership() {
        let candidates_root = temp_dir("candidate-ownership");
        for owner_kind in ["user", "plugin", "external"] {
            let mut candidate = create_candidate(owner_kind, &candidates_root);
            candidate.owner.kind = owner_kind.into();
            replace_candidate(&candidate, &candidates_root);

            assert!(reject_at(&candidate.id, &candidates_root)
                .unwrap_err()
                .contains("ownership is not host_candidate"));
            let stored =
                load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
            assert_eq!(stored.status, SkillCandidateStatusV1::Pending);
            assert_eq!(stored.audit_events.len(), 1);
        }

        fs::remove_dir_all(candidates_root).unwrap();
    }

    #[test]
    fn legacy_host_generated_json_remains_approvable() {
        let candidates_root = temp_dir("legacy-owner");
        let project_root = temp_dir("legacy-owner-project");
        let candidate = create_candidate("legacy-owner", &candidates_root);
        let path = candidate_path_at(&candidates_root, &candidate.id).unwrap();
        let mut legacy = serde_json::to_value(&candidate).unwrap();
        legacy["owner"]["kind"] = serde_json::json!(LEGACY_HOST_CANDIDATE_OWNER);
        let object = legacy.as_object_mut().unwrap();
        object.remove("reviewContentHash");
        object.remove("approvedContentHash");
        object.remove("auditEvents");
        crate::store_lock::write_bytes_atomic(&path, &serde_json::to_vec_pretty(&legacy).unwrap())
            .unwrap();

        let result = approve_at(
            SkillCandidateApproveRequestV1 {
                id: candidate.id.clone(),
                scope: SkillDraftScope::Project,
                project_path: Some(project_root.to_string_lossy().into_owned()),
                draft: None,
                overwrite: false,
                user_confirmed_overwrite: false,
            },
            &candidates_root,
        )
        .unwrap();
        let stored = load_path(&path).unwrap();
        assert_eq!(stored.status, SkillCandidateStatusV1::Approved);
        assert_eq!(stored.owner.kind, HOST_CANDIDATE_OWNER);
        assert_eq!(stored.content_hash, candidate.content_hash);
        assert_eq!(
            stored.review_content_hash.as_deref(),
            Some(draft_content_hash(&stored.draft).unwrap().as_str())
        );
        assert_eq!(stored.approved_path.as_deref(), Some(result.path.as_str()));
        assert_eq!(stored.audit_events.len(), 1);

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn approval_never_overwrites_an_unowned_skill() {
        let candidates_root = temp_dir("external-target");
        let project_root = temp_dir("external-target-project");
        let candidate = create_candidate("external-target", &candidates_root);
        let external = crate::skill_draft::save(SkillDraftSaveRequest {
            name: candidate.draft.name.clone(),
            description: candidate.draft.description.clone(),
            skill_md: candidate.draft.skill_md.clone(),
            references: candidate.draft.references.clone(),
            scope: SkillDraftScope::Project,
            project_path: Some(project_root.to_string_lossy().into_owned()),
            overwrite: false,
        })
        .unwrap();
        let external_skill_md = Path::new(&external.path).join("SKILL.md");
        let before = fs::read_to_string(&external_skill_md).unwrap();

        let no_overwrite = project_approval_v2(&candidate, &project_root);
        assert!(approve_v2_at(no_overwrite, &candidates_root)
            .unwrap_err()
            .starts_with("SKILL_EXISTS:"));

        let mut overwrite = project_approval_v2(&candidate, &project_root);
        overwrite.overwrite = true;
        overwrite.user_confirmed_overwrite = true;
        assert!(approve_v2_at(overwrite, &candidates_root)
            .unwrap_err()
            .contains("not proven to be host_candidate-owned"));
        assert_eq!(fs::read_to_string(external_skill_md).unwrap(), before);
        let stored =
            load_path(&candidate_path_at(&candidates_root, &candidate.id).unwrap()).unwrap();
        assert_eq!(stored.status, SkillCandidateStatusV1::Pending);

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn approved_host_candidate_can_be_explicitly_overwritten() {
        use std::io::Write;

        let candidates_root = temp_dir("host-overwrite");
        let project_root = temp_dir("host-overwrite-project");
        let first = create_candidate("host-overwrite-first", &candidates_root);
        let first_result =
            approve_v2_at(project_approval_v2(&first, &project_root), &candidates_root).unwrap();

        let second = create_candidate("host-overwrite-second", &candidates_root);
        let mut edited = first.draft.clone();
        edited.skill_md.push_str("\nHost-owned update.\n");
        let (_, edited_hash) = final_draft_content_hash(&edited).unwrap();
        let mut overwrite = project_approval_v2(&second, &project_root);
        overwrite.draft = Some(edited);
        overwrite.final_content_hash = Some(edited_hash);
        overwrite.overwrite = true;
        overwrite.user_confirmed_overwrite = true;
        let original = fs::read_to_string(Path::new(&first_result.path).join("SKILL.md")).unwrap();
        let target_seen_by_hook = first_result.path.clone();
        let error =
            approve_v2_at_with_commit_hook(overwrite.clone(), &candidates_root, move || {
                let path = Path::new(&target_seen_by_hook).join("SKILL.md");
                assert!(fs::read_to_string(&path)
                    .unwrap()
                    .contains("Host-owned update."));
                fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .unwrap()
                    .write_all(b"\nExternal during failed commit.\n")
                    .unwrap();
                Err("INJECTED_OVERWRITE_COMMIT_FAILURE".into())
            })
            .unwrap_err();
        assert!(error.contains("INJECTED_OVERWRITE_COMMIT_FAILURE"));
        assert!(error.contains("SKILL_ROLLBACK_CONFLICT"));
        assert_eq!(
            fs::read_to_string(Path::new(&first_result.path).join("SKILL.md")).unwrap(),
            original
        );
        let quarantined = fs::read_dir(Path::new(&first_result.path).parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.contains(".conflict-"))
            })
            .expect("externally changed replacement should be quarantined");
        assert!(fs::read_to_string(quarantined.join("SKILL.md"))
            .unwrap()
            .contains("External during failed commit."));
        let pending = load_path(&candidate_path_at(&candidates_root, &second.id).unwrap()).unwrap();
        assert_eq!(pending.status, SkillCandidateStatusV1::Pending);

        let result = approve_v2_at(overwrite, &candidates_root).unwrap();

        assert_eq!(result.path, first_result.path);
        assert!(result.overwritten);
        assert!(fs::read_to_string(Path::new(&result.path).join("SKILL.md"))
            .unwrap()
            .contains("Host-owned update."));

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn changed_host_owned_target_fails_the_in_lock_tree_hash_check() {
        use std::io::Write;

        let candidates_root = temp_dir("host-target-cas");
        let project_root = temp_dir("host-target-cas-project");
        let first = create_candidate("host-target-cas-first", &candidates_root);
        let first_result =
            approve_v2_at(project_approval_v2(&first, &project_root), &candidates_root).unwrap();
        let skill_md_path = Path::new(&first_result.path).join("SKILL.md");
        fs::OpenOptions::new()
            .append(true)
            .open(&skill_md_path)
            .unwrap()
            .write_all(b"\nExternal change.\n")
            .unwrap();

        let second = create_candidate("host-target-cas-second", &candidates_root);
        let edited = first.draft.clone();
        let (_, edited_hash) = final_draft_content_hash(&edited).unwrap();
        let mut overwrite = project_approval_v2(&second, &project_root);
        overwrite.draft = Some(edited);
        overwrite.final_content_hash = Some(edited_hash);
        overwrite.overwrite = true;
        overwrite.user_confirmed_overwrite = true;
        assert!(approve_v2_at(overwrite, &candidates_root)
            .unwrap_err()
            .contains("not proven to be host_candidate-owned"));
        assert!(fs::read_to_string(skill_md_path)
            .unwrap()
            .contains("External change."));
        let stored = load_path(&candidate_path_at(&candidates_root, &second.id).unwrap()).unwrap();
        assert_eq!(stored.status, SkillCandidateStatusV1::Pending);

        fs::remove_dir_all(candidates_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[test]
    fn audit_ledger_is_bounded_and_contains_no_candidate_body() {
        let candidates_root = temp_dir("audit");
        let mut candidate = create_candidate("audit", &candidates_root);
        let secret_body = "AUDIT_MUST_NOT_RECORD_THIS_BODY";
        candidate.draft.skill_md.push_str(secret_body);
        candidate.audit_events.clear();
        let hash = draft_content_hash(&candidate.draft).unwrap();
        for _ in 0..(MAX_AUDIT_EVENTS + 7) {
            append_audit_event(
                &mut candidate,
                SkillCandidateStatusV1::Pending,
                &hash,
                Utc::now(),
            );
        }

        assert_eq!(candidate.audit_events.len(), MAX_AUDIT_EVENTS);
        let audit_json = serde_json::to_value(&candidate.audit_events).unwrap();
        let audit_text = serde_json::to_string(&audit_json).unwrap();
        assert!(!audit_text.contains(secret_body));
        for event in audit_json.as_array().unwrap() {
            let fields = event.as_object().unwrap();
            assert_eq!(fields.len(), 5);
            for name in [
                "version",
                "candidateId",
                "status",
                "contentHash",
                "occurredAt",
            ] {
                assert!(fields.contains_key(name));
            }
            assert!(valid_content_hash(fields["contentHash"].as_str().unwrap()));
        }

        fs::remove_dir_all(candidates_root).unwrap();
    }
}
