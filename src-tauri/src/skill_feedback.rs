//! Evidence-only feedback loop for explicitly selected Skills.
//!
//! Ranking returns suggestions and cannot invoke a Skill. The use ledger stores
//! only bounded identity/status evidence, never prompts, tool payloads, hidden
//! reasoning, or failure text. Improvement proposals are review DTOs; this
//! module has no Skill filesystem writer or overwrite capability.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SKILL_FEEDBACK_VERSION: u8 = 1;
pub const MAX_SKILL_RANKING_RESULTS: usize = 8;
pub const MAX_SKILL_USE_RECORDS: usize = 512;
pub const MAX_SKILLS_PER_TURN: usize = 8;

const MAX_RANKING_SKILLS: usize = 256;
const MAX_QUERY_BYTES: usize = 2_048;
const MAX_QUERY_TERMS: usize = 32;
const MAX_ID_BYTES: usize = 256;
const MAX_NAME_BYTES: usize = 128;
const MAX_DESCRIPTION_BYTES: usize = 2_048;
const MAX_WHEN_TO_USE_BYTES: usize = 2_048;
const MAX_STATUS_EVENTS: usize = 4;
const MAX_PROPOSAL_EVIDENCE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillIdentityV1 {
    pub id: String,
    pub name: String,
    pub tree_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_candidate_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillMetadataV1 {
    pub identity: SkillIdentityV1,
    pub description: String,
    pub when_to_use: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillMetadataRankingRequestV1 {
    pub version: u8,
    pub query: String,
    pub skills: Vec<SkillMetadataV1>,
    pub max_results: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillRankingDispositionV1 {
    SuggestionOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillMetadataRankingItemV1 {
    pub skill: SkillIdentityV1,
    pub score: u32,
    pub matched_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillMetadataRankingResultV1 {
    pub version: u8,
    pub disposition: SkillRankingDispositionV1,
    pub requires_explicit_acceptance: bool,
    pub items: Vec<SkillMetadataRankingItemV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillUseSelectionV1 {
    Explicit,
    AcceptedSuggestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillUseStatusV1 {
    Prepared,
    Dispatching,
    Applied,
    Succeeded,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillSelectionRequestV1 {
    pub id: String,
    pub expected_tree_hash: String,
    pub selection: SkillUseSelectionV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillUseFeedbackRatingV1 {
    Helpful,
    Unhelpful,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillUseStatusEventV1 {
    pub status: SkillUseStatusV1,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillUseFeedbackV1 {
    pub rating: SkillUseFeedbackRatingV1,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillUseRecordV1 {
    pub version: u8,
    pub id: String,
    pub revision: u64,
    pub selection: SkillUseSelectionV1,
    pub session_id: String,
    pub turn_id: String,
    pub skill: SkillIdentityV1,
    pub status: SkillUseStatusV1,
    pub status_events: Vec<SkillUseStatusEventV1>,
    pub feedback: Option<SkillUseFeedbackV1>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillUsePrepareRequestV1 {
    pub version: u8,
    pub selection: SkillUseSelectionV1,
    pub session_id: String,
    pub turn_id: String,
    pub skill: SkillIdentityV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillUseTransitionRequestV1 {
    pub version: u8,
    pub id: String,
    pub expected_revision: u64,
    pub expected_skill_tree_hash: String,
    pub next_status: SkillUseStatusV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillUseFeedbackRequestV1 {
    pub version: u8,
    pub id: String,
    pub expected_revision: u64,
    pub expected_skill_tree_hash: String,
    pub feedback: SkillUseFeedbackRatingV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillImprovementProposalRequestV1 {
    pub version: u8,
    pub skill_id: String,
    pub expected_skill_tree_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillImprovementOwnershipV1 {
    HostCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillImprovementProposalStatusV1 {
    PendingReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillImprovementOwnerV1 {
    pub kind: SkillImprovementOwnershipV1,
    pub namespace: String,
    pub may_overwrite_external: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillImprovementLineageV1 {
    pub prior_skill_tree_hash: String,
    pub source_candidate_id: Option<String>,
    pub evidence_use_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillImprovementProposalV1 {
    pub version: u8,
    pub id: String,
    pub status: SkillImprovementProposalStatusV1,
    pub owner: SkillImprovementOwnerV1,
    pub skill_id: String,
    pub skill_name: String,
    pub lineage: SkillImprovementLineageV1,
    pub successful_use_count: usize,
    pub helpful_use_count: usize,
    pub repeated_evidence: bool,
    pub requires_user_review: bool,
    pub may_write_skill: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillUseStoreV1 {
    version: u8,
    uses: Vec<SkillUseRecordV1>,
}

impl Default for SkillUseStoreV1 {
    fn default() -> Self {
        Self {
            version: SKILL_FEEDBACK_VERSION,
            uses: Vec::new(),
        }
    }
}

fn store_path() -> PathBuf {
    crate::paths::app_data_root().join("skill-uses.v1.json")
}

/// Rank metadata only. The result carries no execution token or invocation API.
pub fn rank_metadata_v1(
    request: SkillMetadataRankingRequestV1,
) -> Result<SkillMetadataRankingResultV1, String> {
    require_version(request.version)?;
    if request.max_results == 0 || request.max_results > MAX_SKILL_RANKING_RESULTS {
        return Err(format!(
            "SKILL_RANKING_LIMIT: maxResults must be 1..={MAX_SKILL_RANKING_RESULTS}"
        ));
    }
    if request.skills.len() > MAX_RANKING_SKILLS {
        return Err(format!(
            "SKILL_RANKING_LIMIT: more than {MAX_RANKING_SKILLS} skills"
        ));
    }
    validate_bounded_text("ranking query", &request.query, MAX_QUERY_BYTES)?;
    let query_terms = tokenize(&request.query);
    if query_terms.len() > MAX_QUERY_TERMS {
        return Err(format!(
            "SKILL_RANKING_LIMIT: more than {MAX_QUERY_TERMS} query terms"
        ));
    }

    let mut ids = HashSet::with_capacity(request.skills.len());
    let mut items = Vec::new();
    for metadata in request.skills {
        let identity = normalize_skill_identity(metadata.identity, &identity_redact)?;
        if !ids.insert(identity.id.clone()) {
            return Err("SKILL_RANKING_INVALID: duplicate skill identity".into());
        }
        validate_bounded_text(
            "skill description",
            &metadata.description,
            MAX_DESCRIPTION_BYTES,
        )?;
        validate_bounded_text(
            "skill whenToUse",
            &metadata.when_to_use,
            MAX_WHEN_TO_USE_BYTES,
        )?;
        let name_terms = tokenize(&identity.name);
        let description_terms = tokenize(&metadata.description);
        let when_terms = tokenize(&metadata.when_to_use);
        let mut score = 0_u32;
        let mut matched_terms = Vec::new();
        for term in &query_terms {
            let mut matched = false;
            if name_terms.contains(term) {
                score += 8;
                matched = true;
            }
            if when_terms.contains(term) {
                score += 5;
                matched = true;
            }
            if description_terms.contains(term) {
                score += 2;
                matched = true;
            }
            if matched {
                matched_terms.push(term.clone());
            }
        }
        if score > 0 {
            items.push(SkillMetadataRankingItemV1 {
                skill: identity,
                score,
                matched_terms,
            });
        }
    }
    items.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| {
                left.skill
                    .name
                    .to_lowercase()
                    .cmp(&right.skill.name.to_lowercase())
            })
            .then_with(|| left.skill.id.cmp(&right.skill.id))
    });
    items.truncate(request.max_results);
    Ok(SkillMetadataRankingResultV1 {
        version: SKILL_FEEDBACK_VERSION,
        disposition: SkillRankingDispositionV1::SuggestionOnly,
        requires_explicit_acceptance: true,
        items,
    })
}

/// Persist `prepared` before the caller applies or invokes the Skill.
/// Repeating the same session/turn/skill request is idempotent.
pub fn prepare_use_v1(request: SkillUsePrepareRequestV1) -> Result<SkillUseRecordV1, String> {
    prepare_uses_at(
        &store_path(),
        vec![request],
        Utc::now(),
        &crate::store::redact_text,
    )
    .and_then(|mut records| {
        records
            .pop()
            .ok_or_else(|| "SKILL_USE_INVALID: missing prepared record".to_string())
    })
}

pub fn prepare_uses_v1(
    requests: Vec<SkillUsePrepareRequestV1>,
) -> Result<Vec<SkillUseRecordV1>, String> {
    prepare_uses_at(
        &store_path(),
        requests,
        Utc::now(),
        &crate::store::redact_text,
    )
}

pub fn transition_use_v1(request: SkillUseTransitionRequestV1) -> Result<SkillUseRecordV1, String> {
    transition_uses_at(
        &store_path(),
        vec![request],
        Utc::now(),
        &crate::store::redact_text,
    )
    .and_then(|mut records| {
        records
            .pop()
            .ok_or_else(|| "SKILL_USE_INVALID: missing transitioned record".to_string())
    })
}

pub fn transition_uses_v1(
    requests: Vec<SkillUseTransitionRequestV1>,
) -> Result<Vec<SkillUseRecordV1>, String> {
    transition_uses_at(
        &store_path(),
        requests,
        Utc::now(),
        &crate::store::redact_text,
    )
}

pub fn record_feedback_v1(request: SkillUseFeedbackRequestV1) -> Result<SkillUseRecordV1, String> {
    record_feedback_at(
        &store_path(),
        request,
        Utc::now(),
        &crate::store::redact_text,
    )
}

pub fn list_uses_v1() -> Result<Vec<SkillUseRecordV1>, String> {
    list_uses_at(&store_path(), &crate::store::redact_text)
}

/// Build a bounded review proposal from successful evidence. This is read-only.
pub fn build_improvement_proposal_v1(
    request: SkillImprovementProposalRequestV1,
) -> Result<Option<SkillImprovementProposalV1>, String> {
    build_improvement_proposal_at(&store_path(), request, &crate::store::redact_text)
}

/// Recover non-terminal rows left by a previous Host process. Prepared rows
/// were never dispatched and are always interrupted. Dispatching/applied rows
/// are interrupted only when the Host owned the local Runtime process, because
/// a remote ACP process may still be running after the desktop disconnects.
pub fn recover_orphaned_uses_v1(local_runtime_stopped: bool) -> Result<usize, String> {
    recover_orphaned_uses_at(
        &store_path(),
        local_runtime_stopped,
        Utc::now(),
        &crate::store::redact_text,
    )
}

fn recover_orphaned_uses_at(
    path: &Path,
    local_runtime_stopped: bool,
    now: DateTime<Utc>,
    redact: &impl Fn(&str) -> String,
) -> Result<usize, String> {
    update_store_at(path, redact, |store| {
        let mut recovered = 0;
        for record in &mut store.uses {
            let recover = record.status == SkillUseStatusV1::Prepared
                || (local_runtime_stopped
                    && matches!(
                        record.status,
                        SkillUseStatusV1::Dispatching | SkillUseStatusV1::Applied
                    ));
            if !recover {
                continue;
            }
            let occurred_at = now.max(record.updated_at);
            record.status = SkillUseStatusV1::Interrupted;
            record.status_events.push(SkillUseStatusEventV1 {
                status: SkillUseStatusV1::Interrupted,
                occurred_at,
            });
            record.revision = next_revision(record.revision)?;
            record.updated_at = occurred_at;
            recovered += 1;
        }
        Ok(recovered)
    })
}

fn prepare_use_at(
    path: &Path,
    request: SkillUsePrepareRequestV1,
    now: DateTime<Utc>,
    redact: &impl Fn(&str) -> String,
) -> Result<SkillUseRecordV1, String> {
    prepare_uses_at(path, vec![request], now, redact).and_then(|mut records| {
        records
            .pop()
            .ok_or_else(|| "SKILL_USE_INVALID: missing prepared record".to_string())
    })
}

fn prepare_uses_at(
    path: &Path,
    requests: Vec<SkillUsePrepareRequestV1>,
    now: DateTime<Utc>,
    redact: &impl Fn(&str) -> String,
) -> Result<Vec<SkillUseRecordV1>, String> {
    if requests.is_empty() || requests.len() > MAX_SKILLS_PER_TURN {
        return Err(format!(
            "SKILL_USE_LIMIT: each turn requires 1..={MAX_SKILLS_PER_TURN} Skills"
        ));
    }
    let mut normalized = Vec::with_capacity(requests.len());
    let mut request_keys = HashSet::with_capacity(requests.len());
    for request in requests {
        require_version(request.version)?;
        let session_id = normalize_identifier("session id", &request.session_id, redact)?;
        let turn_id = normalize_identifier("turn id", &request.turn_id, redact)?;
        let skill = normalize_skill_identity(request.skill, redact)?;
        if !request_keys.insert((session_id.clone(), turn_id.clone(), skill.id.clone())) {
            return Err("SKILL_USE_INVALID: duplicate turn Skill selection".into());
        }
        normalized.push((request.selection, session_id, turn_id, skill));
    }
    update_store_at(path, redact, |store| {
        let mut records = Vec::with_capacity(normalized.len());
        for (selection, session_id, turn_id, skill) in normalized {
            if let Some(existing) = store.uses.iter().find(|record| {
                record.session_id == session_id
                    && record.turn_id == turn_id
                    && record.skill.id == skill.id
            }) {
                if existing.selection == selection && existing.skill == skill {
                    records.push(existing.clone());
                    continue;
                }
                return Err(
                    "SKILL_USE_CONFLICT: turn already records a different Skill selection".into(),
                );
            }
            let record = SkillUseRecordV1 {
                version: SKILL_FEEDBACK_VERSION,
                id: uuid::Uuid::new_v4().to_string(),
                revision: 1,
                selection,
                session_id,
                turn_id,
                skill,
                status: SkillUseStatusV1::Prepared,
                status_events: vec![SkillUseStatusEventV1 {
                    status: SkillUseStatusV1::Prepared,
                    occurred_at: now,
                }],
                feedback: None,
                created_at: now,
                updated_at: now,
            };
            store.uses.push(record.clone());
            records.push(record);
        }
        Ok(records)
    })
}

fn transition_use_at(
    path: &Path,
    request: SkillUseTransitionRequestV1,
    now: DateTime<Utc>,
    redact: &impl Fn(&str) -> String,
) -> Result<SkillUseRecordV1, String> {
    transition_uses_at(path, vec![request], now, redact).and_then(|mut records| {
        records
            .pop()
            .ok_or_else(|| "SKILL_USE_INVALID: missing transitioned record".to_string())
    })
}

fn transition_uses_at(
    path: &Path,
    requests: Vec<SkillUseTransitionRequestV1>,
    now: DateTime<Utc>,
    redact: &impl Fn(&str) -> String,
) -> Result<Vec<SkillUseRecordV1>, String> {
    if requests.is_empty() || requests.len() > MAX_SKILLS_PER_TURN {
        return Err(format!(
            "SKILL_USE_LIMIT: each transition requires 1..={MAX_SKILLS_PER_TURN} Skills"
        ));
    }
    let mut normalized = Vec::with_capacity(requests.len());
    let mut ids = HashSet::with_capacity(requests.len());
    for request in requests {
        require_version(request.version)?;
        validate_uuid("use id", &request.id)?;
        let id = request.id.trim().to_string();
        if !ids.insert(id.clone()) {
            return Err("SKILL_USE_INVALID: duplicate use transition".into());
        }
        let expected_hash = normalize_hash(&request.expected_skill_tree_hash)?;
        if request.expected_revision == 0 || request.next_status == SkillUseStatusV1::Prepared {
            return Err("SKILL_USE_INVALID: invalid transition request".into());
        }
        normalized.push((
            id,
            request.expected_revision,
            expected_hash,
            request.next_status,
        ));
    }
    update_store_at(path, redact, |store| {
        let mut positions = Vec::with_capacity(normalized.len());
        for (id, expected_revision, expected_hash, next_status) in &normalized {
            let position = store
                .uses
                .iter()
                .position(|record| record.id == *id)
                .ok_or_else(|| "SKILL_USE_NOT_FOUND: evidence record does not exist".to_string())?;
            let record = &store.uses[position];
            if record.revision != *expected_revision {
                return Err(format!(
                    "STALE_SKILL_USE: expected revision {expected_revision}, current revision {}",
                    record.revision
                ));
            }
            if record.skill.tree_hash != *expected_hash {
                return Err("STALE_SKILL_TREE_HASH: Skill tree changed after preparation".into());
            }
            if !valid_transition(record.status, *next_status) {
                return Err("SKILL_USE_INVALID_TRANSITION: status change is not allowed".into());
            }
            positions.push(position);
        }
        let mut records = Vec::with_capacity(positions.len());
        for (position, (_, _, _, next_status)) in positions.into_iter().zip(normalized.iter()) {
            let record = &mut store.uses[position];
            let occurred_at = now.max(record.updated_at);
            record.status = *next_status;
            record.status_events.push(SkillUseStatusEventV1 {
                status: *next_status,
                occurred_at,
            });
            record.revision = next_revision(record.revision)?;
            record.updated_at = occurred_at;
            records.push(record.clone());
        }
        Ok(records)
    })
}

fn record_feedback_at(
    path: &Path,
    request: SkillUseFeedbackRequestV1,
    now: DateTime<Utc>,
    redact: &impl Fn(&str) -> String,
) -> Result<SkillUseRecordV1, String> {
    require_version(request.version)?;
    validate_uuid("use id", &request.id)?;
    let id = request.id.trim().to_string();
    let expected_hash = normalize_hash(&request.expected_skill_tree_hash)?;
    if request.expected_revision == 0 {
        return Err("SKILL_USE_INVALID: expected revision must be positive".into());
    }
    update_store_at(path, redact, |store| {
        let record = find_and_compare(store, &id, request.expected_revision, &expected_hash)?;
        if record.feedback.is_some() {
            return Err("SKILL_USE_FEEDBACK_EXISTS: feedback is already recorded".into());
        }
        if !terminal(record.status) {
            return Err("SKILL_USE_INVALID: feedback requires a terminal use".into());
        }
        if request.feedback == SkillUseFeedbackRatingV1::Helpful
            && record.status != SkillUseStatusV1::Succeeded
        {
            return Err("SKILL_USE_INVALID: failed use cannot be marked helpful".into());
        }
        let occurred_at = now.max(record.updated_at);
        record.feedback = Some(SkillUseFeedbackV1 {
            rating: request.feedback,
            occurred_at,
        });
        record.revision = next_revision(record.revision)?;
        record.updated_at = occurred_at;
        Ok(record.clone())
    })
}

fn list_uses_at(
    path: &Path,
    redact: &impl Fn(&str) -> String,
) -> Result<Vec<SkillUseRecordV1>, String> {
    let mut records = read_store_at(path, redact)?.uses;
    records.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(records)
}

fn build_improvement_proposal_at(
    path: &Path,
    request: SkillImprovementProposalRequestV1,
    redact: &impl Fn(&str) -> String,
) -> Result<Option<SkillImprovementProposalV1>, String> {
    require_version(request.version)?;
    let skill_id = normalize_identifier("skill id", &request.skill_id, redact)?;
    let expected_hash = normalize_hash(&request.expected_skill_tree_hash)?;
    let store = read_store_at(path, redact)?;
    let records = store
        .uses
        .iter()
        .filter(|record| record.skill.id == skill_id)
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(None);
    }
    // Store order is the lock-serialized preparation order. Do not infer the
    // latest Skill tree from wall-clock timestamps, which may tie or regress.
    let latest = *records.last().expect("non-empty checked above");
    if latest.skill.tree_hash != expected_hash {
        return Err("STALE_SKILL_TREE_HASH: latest evidence uses another Skill tree".into());
    }
    let matching = records
        .into_iter()
        .filter(|record| record.skill.tree_hash == expected_hash)
        .collect::<Vec<_>>();
    if matching.iter().any(|record| {
        record
            .feedback
            .as_ref()
            .is_some_and(|feedback| feedback.rating == SkillUseFeedbackRatingV1::Unhelpful)
    }) {
        return Ok(None);
    }
    let mut successful = matching
        .into_iter()
        .filter(|record| record.status == SkillUseStatusV1::Succeeded)
        .collect::<Vec<_>>();
    successful.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let helpful_use_count = successful
        .iter()
        .filter(|record| {
            record
                .feedback
                .as_ref()
                .is_some_and(|feedback| feedback.rating == SkillUseFeedbackRatingV1::Helpful)
        })
        .count();
    let repeated_evidence = successful.len() >= 2;
    if helpful_use_count == 0 && !repeated_evidence {
        return Ok(None);
    }

    let mut selected = Vec::new();
    for record in successful.iter().filter(|record| record.feedback.is_some()) {
        selected.push(*record);
    }
    for record in successful.iter().rev() {
        if selected.iter().all(|selected| selected.id != record.id) {
            selected.push(*record);
        }
        if selected.len() >= MAX_PROPOSAL_EVIDENCE {
            break;
        }
    }
    selected.truncate(MAX_PROPOSAL_EVIDENCE);
    selected.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let evidence_use_ids = selected
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    let id = proposal_id(&skill_id, &expected_hash, &evidence_use_ids);
    Ok(Some(SkillImprovementProposalV1 {
        version: SKILL_FEEDBACK_VERSION,
        id,
        status: SkillImprovementProposalStatusV1::PendingReview,
        owner: SkillImprovementOwnerV1 {
            kind: SkillImprovementOwnershipV1::HostCandidate,
            namespace: "sunsetz".into(),
            may_overwrite_external: false,
        },
        skill_id,
        skill_name: latest.skill.name.clone(),
        lineage: SkillImprovementLineageV1 {
            prior_skill_tree_hash: expected_hash,
            source_candidate_id: latest.skill.source_candidate_id.clone(),
            evidence_use_ids,
        },
        successful_use_count: successful.len(),
        helpful_use_count,
        repeated_evidence,
        requires_user_review: true,
        may_write_skill: false,
    }))
}

fn update_store_at<R>(
    path: &Path,
    redact: &impl Fn(&str) -> String,
    update: impl FnOnce(&mut SkillUseStoreV1) -> Result<R, String>,
) -> Result<R, String> {
    crate::store_lock::update_json_locked(path, SkillUseStoreV1::default, |store| {
        validate_store(store, redact)?;
        let result = update(store)?;
        trim_store(store)?;
        validate_store(store, redact)?;
        Ok(result)
    })
}

fn read_store_at(path: &Path, redact: &impl Fn(&str) -> String) -> Result<SkillUseStoreV1, String> {
    let store = match fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => SkillUseStoreV1::default(),
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| format!("SKILL_USE_PARSE: {}: {error}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SkillUseStoreV1::default(),
        Err(error) => return Err(format!("SKILL_USE_READ: {}: {error}", path.display())),
    };
    validate_store(&store, redact)?;
    Ok(store)
}

fn validate_store(store: &SkillUseStoreV1, redact: &impl Fn(&str) -> String) -> Result<(), String> {
    require_version(store.version)?;
    if store.uses.len() > MAX_SKILL_USE_RECORDS {
        return Err("SKILL_USE_LIMIT: ledger exceeds maximum rows".into());
    }
    let mut ids = HashSet::with_capacity(store.uses.len());
    let mut turns = HashSet::with_capacity(store.uses.len());
    let mut identities: HashMap<(&str, &str), (&str, Option<&str>)> = HashMap::new();
    for record in &store.uses {
        validate_record(record, redact)?;
        if !ids.insert(record.id.as_str()) {
            return Err("SKILL_USE_INVALID: duplicate use id".into());
        }
        if !turns.insert((
            record.session_id.as_str(),
            record.turn_id.as_str(),
            record.skill.id.as_str(),
        )) {
            return Err("SKILL_USE_INVALID: duplicate turn/skill evidence".into());
        }
        let identity_key = (record.skill.id.as_str(), record.skill.tree_hash.as_str());
        let identity_value = (
            record.skill.name.as_str(),
            record.skill.source_candidate_id.as_deref(),
        );
        if identities
            .insert(identity_key, identity_value)
            .is_some_and(|existing| existing != identity_value)
        {
            return Err("SKILL_USE_INVALID: inconsistent Skill lineage".into());
        }
    }
    Ok(())
}

fn validate_record(
    record: &SkillUseRecordV1,
    redact: &impl Fn(&str) -> String,
) -> Result<(), String> {
    require_version(record.version)?;
    validate_uuid("stored use id", &record.id)?;
    let session_id = normalize_identifier("session id", &record.session_id, redact)?;
    let turn_id = normalize_identifier("turn id", &record.turn_id, redact)?;
    let skill = normalize_skill_identity(record.skill.clone(), redact)?;
    if session_id != record.session_id || turn_id != record.turn_id || skill != record.skill {
        return Err("SKILL_USE_INVALID: stored identity is not normalized".into());
    }
    if record.status_events.is_empty() || record.status_events.len() > MAX_STATUS_EVENTS {
        return Err("SKILL_USE_INVALID: invalid status event count".into());
    }
    if record.status_events[0].status != SkillUseStatusV1::Prepared
        || record.status_events[0].occurred_at != record.created_at
    {
        return Err("SKILL_USE_INVALID: use was not written prepared-first".into());
    }
    for window in record.status_events.windows(2) {
        if !valid_transition(window[0].status, window[1].status)
            || window[1].occurred_at < window[0].occurred_at
        {
            return Err("SKILL_USE_INVALID: invalid status history".into());
        }
    }
    let last_event = record
        .status_events
        .last()
        .expect("non-empty checked above");
    if last_event.status != record.status {
        return Err("SKILL_USE_INVALID: status does not match history".into());
    }
    let expected_revision =
        record.status_events.len() as u64 + u64::from(record.feedback.is_some());
    if record.revision != expected_revision {
        return Err("SKILL_USE_INVALID: revision does not match evidence history".into());
    }
    let expected_updated_at = if let Some(feedback) = &record.feedback {
        if !terminal(record.status)
            || feedback.occurred_at < last_event.occurred_at
            || (feedback.rating == SkillUseFeedbackRatingV1::Helpful
                && record.status != SkillUseStatusV1::Succeeded)
        {
            return Err("SKILL_USE_INVALID: invalid feedback evidence".into());
        }
        feedback.occurred_at
    } else {
        last_event.occurred_at
    };
    if record.updated_at != expected_updated_at || record.updated_at < record.created_at {
        return Err("SKILL_USE_INVALID: invalid evidence timestamps".into());
    }
    Ok(())
}

fn trim_store(store: &mut SkillUseStoreV1) -> Result<(), String> {
    if store.uses.len() <= MAX_SKILL_USE_RECORDS {
        return Ok(());
    }
    let excess = store.uses.len() - MAX_SKILL_USE_RECORDS;
    let mut removable = store
        .uses
        .iter()
        .enumerate()
        // Never discard negative feedback: forgetting it could turn the
        // remaining successes into a later improvement proposal. If negative
        // or active rows alone fill the bound, the next prepare fails closed.
        .filter(|(_, record)| {
            terminal(record.status)
                && !record
                    .feedback
                    .as_ref()
                    .is_some_and(|feedback| feedback.rating == SkillUseFeedbackRatingV1::Unhelpful)
        })
        .map(|(index, record)| (index, record.updated_at, record.id.clone()))
        .collect::<Vec<_>>();
    removable.sort_by(|left, right| (left.1, &left.2).cmp(&(right.1, &right.2)));
    if removable.len() < excess {
        return Err("SKILL_USE_LIMIT: too many active use records".into());
    }
    let mut remove_indices = removable
        .into_iter()
        .take(excess)
        .map(|(index, _, _)| index)
        .collect::<Vec<_>>();
    remove_indices.sort_unstable_by(|left, right| right.cmp(left));
    for index in remove_indices {
        store.uses.remove(index);
    }
    Ok(())
}

fn find_and_compare<'a>(
    store: &'a mut SkillUseStoreV1,
    id: &str,
    expected_revision: u64,
    expected_hash: &str,
) -> Result<&'a mut SkillUseRecordV1, String> {
    let record = store
        .uses
        .iter_mut()
        .find(|record| record.id == id)
        .ok_or_else(|| "SKILL_USE_NOT_FOUND: evidence record does not exist".to_string())?;
    if record.revision != expected_revision {
        return Err(format!(
            "STALE_SKILL_USE: expected revision {expected_revision}, current revision {}",
            record.revision
        ));
    }
    if record.skill.tree_hash != expected_hash {
        return Err("STALE_SKILL_TREE_HASH: Skill tree changed after preparation".into());
    }
    Ok(record)
}

fn valid_transition(from: SkillUseStatusV1, to: SkillUseStatusV1) -> bool {
    matches!(
        (from, to),
        // Prepared -> Applied remains valid for v1 rows created before the
        // durable dispatch barrier was introduced.
        (SkillUseStatusV1::Prepared, SkillUseStatusV1::Applied)
            | (SkillUseStatusV1::Prepared, SkillUseStatusV1::Dispatching)
            | (SkillUseStatusV1::Prepared, SkillUseStatusV1::Failed)
            | (SkillUseStatusV1::Prepared, SkillUseStatusV1::Interrupted)
            | (SkillUseStatusV1::Dispatching, SkillUseStatusV1::Applied)
            | (SkillUseStatusV1::Dispatching, SkillUseStatusV1::Succeeded)
            | (SkillUseStatusV1::Dispatching, SkillUseStatusV1::Failed)
            | (SkillUseStatusV1::Dispatching, SkillUseStatusV1::Interrupted)
            | (SkillUseStatusV1::Applied, SkillUseStatusV1::Succeeded)
            | (SkillUseStatusV1::Applied, SkillUseStatusV1::Failed)
            | (SkillUseStatusV1::Applied, SkillUseStatusV1::Interrupted)
    )
}

fn terminal(status: SkillUseStatusV1) -> bool {
    matches!(
        status,
        SkillUseStatusV1::Succeeded | SkillUseStatusV1::Failed | SkillUseStatusV1::Interrupted
    )
}

fn normalize_skill_identity(
    identity: SkillIdentityV1,
    redact: &impl Fn(&str) -> String,
) -> Result<SkillIdentityV1, String> {
    let id = normalize_identifier("skill id", &identity.id, redact)?;
    let name = identity.name.trim();
    validate_bounded_text("skill name", name, MAX_NAME_BYTES)?;
    if name.is_empty() || redacted(name, redact) || contains_sensitive_material(name) {
        return Err("SKILL_USE_SENSITIVE: unsafe Skill name".into());
    }
    let tree_hash = normalize_hash(&identity.tree_hash)?;
    let source_candidate_id = identity
        .source_candidate_id
        .map(|value| normalize_candidate_id(&value))
        .transpose()?;
    Ok(SkillIdentityV1 {
        id,
        name: name.to_string(),
        tree_hash,
        source_candidate_id,
    })
}

fn normalize_identifier(
    label: &str,
    value: &str,
    redact: &impl Fn(&str) -> String,
) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(format!("SKILL_USE_INVALID: invalid {label}"));
    }
    if redacted(value, redact) || contains_sensitive_material(value) {
        return Err(format!("SKILL_USE_SENSITIVE: unsafe {label}"));
    }
    Ok(value.to_string())
}

fn normalize_candidate_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SKILL_USE_INVALID: invalid source candidate id".into());
    }
    Ok(value.to_ascii_lowercase())
}

fn normalize_hash(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SKILL_USE_INVALID: invalid Skill tree hash".into());
    }
    Ok(value.to_ascii_lowercase())
}

fn validate_uuid(label: &str, value: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(value.trim())
        .map(|_| ())
        .map_err(|_| format!("SKILL_USE_INVALID: invalid {label}"))
}

fn validate_bounded_text(label: &str, value: &str, max_bytes: usize) -> Result<(), String> {
    if value.len() > max_bytes {
        return Err(format!("SKILL_FEEDBACK_LIMIT: {label} is too long"));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !character.is_whitespace())
    {
        return Err(format!(
            "SKILL_FEEDBACK_INVALID: {label} contains control characters"
        ));
    }
    Ok(())
}

fn tokenize(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn redacted(value: &str, redact: &impl Fn(&str) -> String) -> bool {
    redact(value).trim() != value.trim()
}

fn identity_redact(value: &str) -> String {
    value.to_string()
}

fn contains_sensitive_material(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "-----begin private key-----",
        "authorization: bearer ",
        "github_pat_",
        "ghp_",
        "sk-proj-",
        "xoxb-",
        "refresh_token",
        "client_secret",
        "password=",
        "api_key=",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn proposal_id(skill_id: &str, tree_hash: &str, evidence_use_ids: &[String]) -> String {
    let mut hasher = Sha256::new();
    for part in std::iter::once(skill_id)
        .chain(std::iter::once(tree_hash))
        .chain(evidence_use_ids.iter().map(String::as_str))
    {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn next_revision(revision: u64) -> Result<u64, String> {
    revision
        .checked_add(1)
        .ok_or_else(|| "SKILL_USE_LIMIT: revision overflow".to_string())
}

fn require_version(version: u8) -> Result<(), String> {
    if version == SKILL_FEEDBACK_VERSION {
        Ok(())
    } else {
        Err("SKILL_FEEDBACK_UNSUPPORTED_VERSION: expected schema v1".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use std::sync::{Arc, Barrier};
    use std::thread;

    struct TestStore {
        root: PathBuf,
        path: PathBuf,
    }

    impl TestStore {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "sunsetz-skill-feedback-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&root).unwrap();
            let path = root.join("skill-uses.v1.json");
            Self { root, path }
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-24T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn hash(character: char) -> String {
        character.to_string().repeat(64)
    }

    fn identity(tree_hash: &str) -> SkillIdentityV1 {
        SkillIdentityV1 {
            id: "skill-a".into(),
            name: "review-skill".into(),
            tree_hash: tree_hash.into(),
            source_candidate_id: Some("a".repeat(32)),
        }
    }

    fn metadata(id: &str, name: &str, description: &str, when_to_use: &str) -> SkillMetadataV1 {
        SkillMetadataV1 {
            identity: SkillIdentityV1 {
                id: id.into(),
                name: name.into(),
                tree_hash: hash('a'),
                source_candidate_id: None,
            },
            description: description.into(),
            when_to_use: when_to_use.into(),
        }
    }

    fn prepare_request(
        session_id: &str,
        turn_id: &str,
        tree_hash: &str,
    ) -> SkillUsePrepareRequestV1 {
        SkillUsePrepareRequestV1 {
            version: SKILL_FEEDBACK_VERSION,
            selection: SkillUseSelectionV1::Explicit,
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            skill: identity(tree_hash),
        }
    }

    fn transition_request(
        record: &SkillUseRecordV1,
        next_status: SkillUseStatusV1,
    ) -> SkillUseTransitionRequestV1 {
        SkillUseTransitionRequestV1 {
            version: SKILL_FEEDBACK_VERSION,
            id: record.id.clone(),
            expected_revision: record.revision,
            expected_skill_tree_hash: record.skill.tree_hash.clone(),
            next_status,
        }
    }

    fn succeed(path: &Path, record: &SkillUseRecordV1, now: DateTime<Utc>) -> SkillUseRecordV1 {
        let applied = transition_use_at(
            path,
            transition_request(record, SkillUseStatusV1::Applied),
            now + Duration::seconds(1),
            &identity_redact,
        )
        .unwrap();
        transition_use_at(
            path,
            transition_request(&applied, SkillUseStatusV1::Succeeded),
            now + Duration::seconds(2),
            &identity_redact,
        )
        .unwrap()
    }

    fn proposal_request(tree_hash: &str) -> SkillImprovementProposalRequestV1 {
        SkillImprovementProposalRequestV1 {
            version: SKILL_FEEDBACK_VERSION,
            skill_id: "skill-a".into(),
            expected_skill_tree_hash: tree_hash.into(),
        }
    }

    #[test]
    fn unknown_fields_are_rejected_at_request_and_store_boundaries() {
        let ranking = serde_json::json!({
            "version": 1,
            "query": "review",
            "skills": [],
            "maxResults": 2,
            "autoInvoke": true
        });
        assert!(serde_json::from_value::<SkillMetadataRankingRequestV1>(ranking).is_err());

        let store = TestStore::new("unknown-fields");
        fs::write(
            &store.path,
            r#"{"version":1,"uses":[],"rawToolPayload":{}}"#,
        )
        .unwrap();
        assert!(list_uses_at(&store.path, &identity_redact)
            .unwrap_err()
            .contains("SKILL_USE_PARSE"));
    }

    #[test]
    fn multi_skill_dispatch_barrier_is_atomic_and_stale_batch_changes_nothing() {
        let store = TestStore::new("batch-dispatch");
        let now = fixed_now();
        let first = prepare_request("session-a", "turn-a", &hash('a'));
        let mut second = prepare_request("session-a", "turn-a", &hash('b'));
        second.skill.id = "skill-b".into();
        second.skill.name = "format-skill".into();
        let prepared =
            prepare_uses_at(&store.path, vec![first, second], now, &identity_redact).unwrap();
        let dispatching = transition_uses_at(
            &store.path,
            prepared
                .iter()
                .map(|record| transition_request(record, SkillUseStatusV1::Dispatching))
                .collect(),
            now + Duration::seconds(1),
            &identity_redact,
        )
        .unwrap();
        assert!(dispatching
            .iter()
            .all(|record| record.status == SkillUseStatusV1::Dispatching));

        let mut stale = dispatching
            .iter()
            .map(|record| transition_request(record, SkillUseStatusV1::Applied))
            .collect::<Vec<_>>();
        stale[1].expected_revision += 1;
        assert!(transition_uses_at(
            &store.path,
            stale,
            now + Duration::seconds(2),
            &identity_redact,
        )
        .unwrap_err()
        .contains("STALE_SKILL_USE"));
        let stored = list_uses_at(&store.path, &identity_redact).unwrap();
        assert!(stored
            .iter()
            .all(|record| record.status == SkillUseStatusV1::Dispatching));
    }

    #[test]
    fn startup_recovery_interrupts_only_rows_the_host_can_prove_stopped() {
        let store = TestStore::new("startup-recovery");
        let now = fixed_now();
        let mut requests = Vec::new();
        for (index, name) in ["prepared", "dispatching", "applied"]
            .into_iter()
            .enumerate()
        {
            let mut request =
                prepare_request("session-a", "turn-a", &hash(char::from(b'a' + index as u8)));
            request.skill.id = format!("skill-{index}");
            request.skill.name = name.into();
            requests.push(request);
        }
        let prepared = prepare_uses_at(&store.path, requests, now, &identity_redact).unwrap();
        let dispatching = transition_uses_at(
            &store.path,
            prepared[1..]
                .iter()
                .map(|record| transition_request(record, SkillUseStatusV1::Dispatching))
                .collect(),
            now + Duration::seconds(1),
            &identity_redact,
        )
        .unwrap();
        transition_use_at(
            &store.path,
            transition_request(&dispatching[1], SkillUseStatusV1::Applied),
            now + Duration::seconds(2),
            &identity_redact,
        )
        .unwrap();

        assert_eq!(
            recover_orphaned_uses_at(
                &store.path,
                false,
                now + Duration::seconds(3),
                &identity_redact,
            )
            .unwrap(),
            1
        );
        let remote = list_uses_at(&store.path, &identity_redact).unwrap();
        assert_eq!(
            remote
                .iter()
                .filter(|record| record.status == SkillUseStatusV1::Interrupted)
                .count(),
            1
        );
        assert_eq!(
            recover_orphaned_uses_at(
                &store.path,
                true,
                now + Duration::seconds(4),
                &identity_redact,
            )
            .unwrap(),
            2
        );
        assert!(list_uses_at(&store.path, &identity_redact)
            .unwrap()
            .iter()
            .all(|record| record.status == SkillUseStatusV1::Interrupted));
    }

    #[test]
    fn ranking_is_deterministic_bounded_and_suggestion_only() {
        let request = SkillMetadataRankingRequestV1 {
            version: SKILL_FEEDBACK_VERSION,
            query: "pdf citation review".into(),
            skills: vec![
                metadata(
                    "skill-citation",
                    "citation-helper",
                    "review citations",
                    "write academic sources",
                ),
                metadata(
                    "skill-pdf",
                    "pdf-review",
                    "citation workflow",
                    "review pdf evidence",
                ),
                metadata("skill-image", "image-helper", "render art", "make images"),
            ],
            max_results: 2,
        };
        let first = rank_metadata_v1(request.clone()).unwrap();
        let second = rank_metadata_v1(request).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.disposition, SkillRankingDispositionV1::SuggestionOnly);
        assert!(first.requires_explicit_acceptance);
        assert_eq!(first.items.len(), 2);
        assert_eq!(first.items[0].skill.id, "skill-pdf");
        assert!(first.items[0].score > first.items[1].score);
        assert_eq!(
            first.items[0].matched_terms,
            vec!["citation", "pdf", "review"]
        );

        let over_limit = SkillMetadataRankingRequestV1 {
            version: SKILL_FEEDBACK_VERSION,
            query: "review".into(),
            skills: Vec::new(),
            max_results: MAX_SKILL_RANKING_RESULTS + 1,
        };
        assert!(rank_metadata_v1(over_limit)
            .unwrap_err()
            .contains("SKILL_RANKING_LIMIT"));
    }

    #[test]
    fn prepare_is_write_first_idempotent_and_contains_no_raw_payload_fields() {
        let store = TestStore::new("prepare");
        let now = fixed_now();
        let request = prepare_request("session-1", "turn-1", &hash('a'));
        let first = prepare_use_at(&store.path, request.clone(), now, &identity_redact).unwrap();
        let repeated = prepare_use_at(
            &store.path,
            request,
            now + Duration::seconds(1),
            &identity_redact,
        )
        .unwrap();
        assert_eq!(first.id, repeated.id);
        assert_eq!(first.status, SkillUseStatusV1::Prepared);
        assert_eq!(first.revision, 1);
        assert_eq!(first.status_events.len(), 1);
        assert_eq!(
            list_uses_at(&store.path, &identity_redact).unwrap().len(),
            1
        );
        let json = serde_json::to_value(&first).unwrap();
        assert!(json.get("rawToolPayload").is_none());
        assert!(json.get("reasoning").is_none());
        assert!(json.get("error").is_none());

        let mut sensitive = prepare_request("session-2", "turn-2", &hash('a'));
        sensitive.skill.name = "sk-proj-secret-material".into();
        assert!(
            prepare_use_at(&store.path, sensitive, now, &identity_redact)
                .unwrap_err()
                .contains("SKILL_USE_SENSITIVE")
        );
    }

    #[test]
    fn failed_use_never_creates_improvement_evidence() {
        let store = TestStore::new("failure-no-learning");
        let now = fixed_now();
        let tree_hash = hash('a');
        let prepared = prepare_use_at(
            &store.path,
            prepare_request("session-1", "turn-1", &tree_hash),
            now,
            &identity_redact,
        )
        .unwrap();
        let failed = transition_use_at(
            &store.path,
            transition_request(&prepared, SkillUseStatusV1::Failed),
            now + Duration::seconds(1),
            &identity_redact,
        )
        .unwrap();
        assert_eq!(failed.status, SkillUseStatusV1::Failed);
        assert!(build_improvement_proposal_at(
            &store.path,
            proposal_request(&tree_hash),
            &identity_redact
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn helpful_success_creates_host_owned_review_only_proposal() {
        let store = TestStore::new("helpful-proposal");
        let now = fixed_now();
        let tree_hash = hash('a');
        let prepared = prepare_use_at(
            &store.path,
            prepare_request("session-1", "turn-1", &tree_hash),
            now,
            &identity_redact,
        )
        .unwrap();
        let succeeded = succeed(&store.path, &prepared, now);
        let feedback = record_feedback_at(
            &store.path,
            SkillUseFeedbackRequestV1 {
                version: SKILL_FEEDBACK_VERSION,
                id: succeeded.id.clone(),
                expected_revision: succeeded.revision,
                expected_skill_tree_hash: tree_hash.clone(),
                feedback: SkillUseFeedbackRatingV1::Helpful,
            },
            now + Duration::seconds(3),
            &identity_redact,
        )
        .unwrap();
        assert_eq!(feedback.revision, 4);

        let proposal = build_improvement_proposal_at(
            &store.path,
            proposal_request(&tree_hash),
            &identity_redact,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            proposal.owner.kind,
            SkillImprovementOwnershipV1::HostCandidate
        );
        assert_eq!(proposal.owner.namespace, "sunsetz");
        assert!(!proposal.owner.may_overwrite_external);
        assert!(proposal.requires_user_review);
        assert!(!proposal.may_write_skill);
        assert_eq!(proposal.helpful_use_count, 1);
        assert_eq!(proposal.successful_use_count, 1);
        assert!(!proposal.repeated_evidence);
        assert_eq!(proposal.lineage.prior_skill_tree_hash, tree_hash);
        assert_eq!(proposal.lineage.evidence_use_ids, vec![succeeded.id]);
        assert_eq!(proposal.lineage.source_candidate_id, Some("a".repeat(32)));
    }

    #[test]
    fn repeated_success_is_evidence_but_unhelpful_feedback_blocks_it() {
        let store = TestStore::new("repeated-proposal");
        let now = fixed_now();
        let tree_hash = hash('a');
        let first = prepare_use_at(
            &store.path,
            prepare_request("session-1", "turn-1", &tree_hash),
            now,
            &identity_redact,
        )
        .unwrap();
        let first = succeed(&store.path, &first, now);
        let second = prepare_use_at(
            &store.path,
            prepare_request("session-1", "turn-2", &tree_hash),
            now + Duration::minutes(1),
            &identity_redact,
        )
        .unwrap();
        let second = succeed(&store.path, &second, now + Duration::minutes(1));

        let proposal = build_improvement_proposal_at(
            &store.path,
            proposal_request(&tree_hash),
            &identity_redact,
        )
        .unwrap()
        .unwrap();
        assert!(proposal.repeated_evidence);
        assert_eq!(proposal.successful_use_count, 2);
        assert_eq!(proposal.helpful_use_count, 0);
        assert_eq!(proposal.lineage.evidence_use_ids.len(), 2);
        let repeated = build_improvement_proposal_at(
            &store.path,
            proposal_request(&tree_hash),
            &identity_redact,
        )
        .unwrap()
        .unwrap();
        assert_eq!(proposal, repeated);

        record_feedback_at(
            &store.path,
            SkillUseFeedbackRequestV1 {
                version: SKILL_FEEDBACK_VERSION,
                id: second.id,
                expected_revision: second.revision,
                expected_skill_tree_hash: tree_hash.clone(),
                feedback: SkillUseFeedbackRatingV1::Unhelpful,
            },
            now + Duration::minutes(2),
            &identity_redact,
        )
        .unwrap();
        assert!(build_improvement_proposal_at(
            &store.path,
            proposal_request(&tree_hash),
            &identity_redact
        )
        .unwrap()
        .is_none());
        assert_eq!(first.status, SkillUseStatusV1::Succeeded);
    }

    #[test]
    fn stale_tree_hash_and_revision_fail_closed() {
        let store = TestStore::new("stale");
        let now = fixed_now();
        let tree_hash = hash('a');
        let prepared = prepare_use_at(
            &store.path,
            prepare_request("session-1", "turn-1", &tree_hash),
            now,
            &identity_redact,
        )
        .unwrap();
        let mut stale_hash = transition_request(&prepared, SkillUseStatusV1::Applied);
        stale_hash.expected_skill_tree_hash = hash('b');
        assert!(transition_use_at(
            &store.path,
            stale_hash,
            now + Duration::seconds(1),
            &identity_redact
        )
        .unwrap_err()
        .contains("STALE_SKILL_TREE_HASH"));

        let applied = transition_use_at(
            &store.path,
            transition_request(&prepared, SkillUseStatusV1::Applied),
            now + Duration::seconds(1),
            &identity_redact,
        )
        .unwrap();
        assert!(transition_use_at(
            &store.path,
            transition_request(&prepared, SkillUseStatusV1::Failed),
            now + Duration::seconds(2),
            &identity_redact
        )
        .unwrap_err()
        .contains("STALE_SKILL_USE"));
        assert_eq!(
            list_uses_at(&store.path, &identity_redact).unwrap()[0].revision,
            applied.revision
        );
        assert!(build_improvement_proposal_at(
            &store.path,
            proposal_request(&hash('b')),
            &identity_redact
        )
        .unwrap_err()
        .contains("STALE_SKILL_TREE_HASH"));
    }

    #[test]
    fn concurrent_terminal_cas_has_exactly_one_winner() {
        let store = TestStore::new("concurrent-cas");
        let now = fixed_now();
        let tree_hash = hash('a');
        let prepared = prepare_use_at(
            &store.path,
            prepare_request("session-1", "turn-1", &tree_hash),
            now,
            &identity_redact,
        )
        .unwrap();
        let applied = transition_use_at(
            &store.path,
            transition_request(&prepared, SkillUseStatusV1::Applied),
            now + Duration::seconds(1),
            &identity_redact,
        )
        .unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut threads = Vec::new();
        for status in [SkillUseStatusV1::Succeeded, SkillUseStatusV1::Failed] {
            let path = store.path.clone();
            let barrier = Arc::clone(&barrier);
            let request = transition_request(&applied, status);
            threads.push(thread::spawn(move || {
                barrier.wait();
                transition_use_at(&path, request, now + Duration::seconds(2), &identity_redact)
            }));
        }
        barrier.wait();
        let results = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|error| error.contains("STALE_SKILL_USE")))
                .count(),
            1
        );
        let stored = list_uses_at(&store.path, &identity_redact).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].revision, 3);
        assert!(terminal(stored[0].status));
    }

    #[test]
    fn bounded_ledger_prunes_old_terminal_evidence_and_keeps_new_prepared_use() {
        let store = TestStore::new("bounded-ledger");
        let now = fixed_now();
        let tree_hash = hash('a');
        let uses = (0..MAX_SKILL_USE_RECORDS)
            .map(|index| {
                let created_at = now + Duration::seconds(index as i64);
                let feedback = (index == 0).then_some(SkillUseFeedbackV1 {
                    rating: SkillUseFeedbackRatingV1::Unhelpful,
                    occurred_at: created_at,
                });
                SkillUseRecordV1 {
                    version: SKILL_FEEDBACK_VERSION,
                    id: uuid::Uuid::new_v4().to_string(),
                    revision: 3 + u64::from(feedback.is_some()),
                    selection: SkillUseSelectionV1::Explicit,
                    session_id: format!("session-{index}"),
                    turn_id: format!("turn-{index}"),
                    skill: identity(&tree_hash),
                    status: SkillUseStatusV1::Succeeded,
                    status_events: vec![
                        SkillUseStatusEventV1 {
                            status: SkillUseStatusV1::Prepared,
                            occurred_at: created_at,
                        },
                        SkillUseStatusEventV1 {
                            status: SkillUseStatusV1::Applied,
                            occurred_at: created_at,
                        },
                        SkillUseStatusEventV1 {
                            status: SkillUseStatusV1::Succeeded,
                            occurred_at: created_at,
                        },
                    ],
                    feedback,
                    created_at,
                    updated_at: created_at,
                }
            })
            .collect::<Vec<_>>();
        let negative_id = uses[0].id.clone();
        let initial = SkillUseStoreV1 {
            version: SKILL_FEEDBACK_VERSION,
            uses,
        };
        validate_store(&initial, &identity_redact).unwrap();
        crate::store_lock::write_bytes_atomic(
            &store.path,
            &serde_json::to_vec_pretty(&initial).unwrap(),
        )
        .unwrap();

        let prepared = prepare_use_at(
            &store.path,
            prepare_request("session-new", "turn-new", &tree_hash),
            now + Duration::hours(1),
            &identity_redact,
        )
        .unwrap();
        let loaded = list_uses_at(&store.path, &identity_redact).unwrap();
        assert_eq!(loaded.len(), MAX_SKILL_USE_RECORDS);
        assert!(loaded.iter().any(|record| {
            record.id == prepared.id && record.status == SkillUseStatusV1::Prepared
        }));
        assert!(loaded.iter().any(|record| record.id == negative_id));
        assert_eq!(
            loaded
                .iter()
                .filter(|record| record.status == SkillUseStatusV1::Succeeded)
                .count(),
            MAX_SKILL_USE_RECORDS - 1
        );
        assert!(build_improvement_proposal_at(
            &store.path,
            proposal_request(&tree_hash),
            &identity_redact
        )
        .unwrap()
        .is_none());
    }
}
