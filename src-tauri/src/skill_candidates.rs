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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCandidateStatusV1 {
    Pending,
    Approved,
    Rejected,
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
pub struct SkillCandidateV1 {
    pub version: u8,
    pub id: String,
    pub status: SkillCandidateStatusV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub content_hash: String,
    pub source: SkillCandidateSourceV1,
    pub owner: SkillCandidateOwnerV1,
    pub draft: SkillCandidateDraftV1,
    pub approved_path: Option<String>,
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
    let content_hash = sha256(&[
        session_id,
        &user.id,
        &assistant.id,
        &user_text,
        &assistant_text,
    ]);
    let id = content_hash[..32].to_string();
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
    let name = format!("learned-{}", &content_hash[..10]);
    let description =
        "Review and reuse a workflow learned from a completed local task.".to_string();
    let skill_md = format!(
        "---\nname: {name}\ndescription: {description}\n---\n\n# Review-required workflow candidate\n\nThis draft was generated from visible task output. Verify and generalize every step before enabling it.\n\n## Source request\n\n{user_text}\n\n## Observed outcome\n\n{assistant_text}\n\n## Review checklist\n\n- Remove task-specific paths and identifiers.\n- Confirm prerequisites, failure modes, and safe defaults.\n- Keep secrets, hidden reasoning, and raw tool payloads out of the Skill.\n"
    );
    let now = Utc::now();
    let candidate = SkillCandidateV1 {
        version: 1,
        id: id.clone(),
        status: SkillCandidateStatusV1::Pending,
        created_at: now,
        updated_at: now,
        content_hash,
        source: SkillCandidateSourceV1 {
            session_id: session_id.to_string(),
            session_title,
            message_ids: vec![user.id.clone(), assistant.id.clone()],
        },
        owner: SkillCandidateOwnerV1 {
            kind: "host_generated".into(),
            namespace: "sunsetz".into(),
            may_overwrite_external: false,
        },
        draft: SkillCandidateDraftV1 {
            name,
            description,
            skill_md,
            references: Vec::new(),
        },
        approved_path: None,
    };
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
    if !matches!(candidate.status, SkillCandidateStatusV1::Pending) {
        return Err("skill candidate is no longer pending".into());
    }
    if request.overwrite && !request.user_confirmed_overwrite {
        return Err("explicit overwrite confirmation is required".into());
    }
    let draft = request.draft.unwrap_or_else(|| candidate.draft.clone());
    let result = crate::skill_draft::save(SkillDraftSaveRequest {
        name: draft.name,
        description: draft.description,
        skill_md: draft.skill_md,
        references: draft.references,
        scope: request.scope,
        project_path: request.project_path,
        overwrite: request.overwrite,
    })?;
    crate::store_lock::update_json_locked(
        &path,
        || candidate.clone(),
        |stored: &mut SkillCandidateV1| {
            stored.status = SkillCandidateStatusV1::Approved;
            stored.updated_at = Utc::now();
            stored.approved_path = Some(result.path.clone());
            Ok(())
        },
    )?;
    Ok(result)
}

pub fn reject(id: &str) -> Result<SkillCandidateV1, String> {
    reject_at(id, &root())
}

fn reject_at(id: &str, candidates_root: &Path) -> Result<SkillCandidateV1, String> {
    let path = candidate_path_at(candidates_root, id.trim())?;
    let fallback = load_path(&path)?;
    crate::store_lock::update_json_locked(
        &path,
        || fallback.clone(),
        |stored: &mut SkillCandidateV1| {
            stored.status = SkillCandidateStatusV1::Rejected;
            stored.updated_at = Utc::now();
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
        assert_eq!(candidate.owner.kind, "host_generated");
        assert_eq!(candidate.owner.namespace, "sunsetz");
        assert!(!candidate.owner.may_overwrite_external);
        assert!(candidate
            .draft
            .skill_md
            .contains("Review-required workflow candidate"));
        assert!(candidate.draft.skill_md.contains("Source request"));
        assert!(candidate.approved_path.is_none());
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
}
