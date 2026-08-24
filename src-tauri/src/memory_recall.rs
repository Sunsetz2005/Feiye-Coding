//! Read-only, versioned preview for explicit multi-layer memory recall.
//!
//! This module never persists the query, mutates a fact source, or injects
//! anything into the Runtime. Approved Memory candidates remain the trusted
//! layer; cross-session journal hits are returned separately as untrusted
//! evidence for a future visible review surface.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::memory_candidates::{
    MemoryCandidateMutationRequestV1, MemoryCandidateSourceV1, MemoryCandidateStatusV1,
    MemoryCandidateTypeV1, MemoryCandidateV1, MAX_MEMORY_CONTEXT_ITEM_CHARS,
    MAX_MEMORY_CONTEXT_PACK_ITEMS, MAX_MEMORY_CONTEXT_TOTAL_CHARS,
};
use crate::session_search::SessionSearchResultV1;

pub const MEMORY_RECALL_VERSION: u8 = 1;
pub const MAX_MEMORY_RECALL_QUERY_CHARS: usize = 1_000;
pub const MAX_MEMORY_RECALL_EVIDENCE_ITEMS: usize = 8;
pub const MAX_MEMORY_RECALL_EVIDENCE_SNIPPET_CHARS: usize = 500;

const MAX_CURRENT_SESSION_ID_CHARS: usize = 256;
const SESSION_SEARCH_FETCH_LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryRecallRequestV1 {
    pub version: u8,
    pub query: String,
    #[serde(default)]
    pub current_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryRecallCandidateV1 {
    pub candidate_id: String,
    pub content_hash: String,
    #[serde(rename = "type")]
    pub candidate_type: MemoryCandidateTypeV1,
    pub content: String,
    pub provenance: MemoryCandidateSourceV1,
    pub relevance_score: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryRecallEvidenceV1 {
    pub session_id: String,
    pub session_title: String,
    pub message_id: String,
    pub role: String,
    pub snippet: String,
    pub rank: f64,
    pub evidence_only: bool,
    pub untrusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryRecallPreviewV1 {
    pub version: u8,
    pub memory_candidates: Vec<MemoryRecallCandidateV1>,
    /// Can be passed unchanged as `MemoryContextPackRequestV1::selections`.
    pub context_pack_selections: Vec<MemoryCandidateMutationRequestV1>,
    pub session_evidence: Vec<MemoryRecallEvidenceV1>,
}

struct ValidatedRecallRequest {
    query: String,
    current_session_id: Option<String>,
}

fn validate_request(request: &MemoryRecallRequestV1) -> Result<ValidatedRecallRequest, String> {
    if request.version != MEMORY_RECALL_VERSION {
        return Err(format!(
            "unsupported memory recall version: {}",
            request.version
        ));
    }
    if request.query.chars().count() > MAX_MEMORY_RECALL_QUERY_CHARS {
        return Err(format!(
            "memory recall query exceeds {MAX_MEMORY_RECALL_QUERY_CHARS} characters"
        ));
    }
    if request
        .query
        .chars()
        .any(|character| character.is_control() && !character.is_whitespace())
    {
        return Err("memory recall query contains control characters".into());
    }
    let query = request
        .query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if query.is_empty() || normalize_for_match(&query).is_empty() {
        return Err("memory recall query is empty".into());
    }

    let current_session_id = request
        .current_session_id
        .as_deref()
        .map(|value| {
            if value.trim() != value
                || value.is_empty()
                || value.chars().count() > MAX_CURRENT_SESSION_ID_CHARS
                || value.chars().any(char::is_control)
            {
                return Err("invalid current memory recall session id".to_string());
            }
            Ok(value.to_string())
        })
        .transpose()?;

    Ok(ValidatedRecallRequest {
        query,
        current_session_id,
    })
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
    )
}

fn normalize_for_match(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut separated = true;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || is_cjk(character) {
            normalized.push(character);
            separated = false;
        } else if !separated {
            normalized.push(' ');
            separated = true;
        }
    }
    normalized.trim().to_string()
}

fn query_units(query: &str) -> BTreeSet<String> {
    let mut units = BTreeSet::new();
    for segment in query.split_whitespace() {
        let segment_chars = segment.chars().collect::<Vec<_>>();
        if segment_chars.len() > 1 || segment_chars.iter().any(|character| is_cjk(*character)) {
            units.insert(segment.to_string());
        }

        let mut cjk_run = Vec::new();
        let flush_cjk_run = |run: &mut Vec<char>, units: &mut BTreeSet<String>| {
            if run.len() == 1 {
                units.insert(run[0].to_string());
            } else {
                for pair in run.windows(2) {
                    units.insert(pair.iter().collect());
                }
            }
            run.clear();
        };
        for character in segment_chars {
            if is_cjk(character) {
                cjk_run.push(character);
            } else if !cjk_run.is_empty() {
                flush_cjk_run(&mut cjk_run, &mut units);
            }
        }
        if !cjk_run.is_empty() {
            flush_cjk_run(&mut cjk_run, &mut units);
        }
    }
    units
}

fn relevance_score(normalized_query: &str, content: &str) -> u64 {
    let normalized_content = normalize_for_match(content);
    if normalized_content.is_empty() {
        return 0;
    }

    let mut score = 0_u64;
    if normalized_content == normalized_query {
        score = score.saturating_add(1_000_000);
    } else if normalized_content.contains(normalized_query) {
        score = score.saturating_add(100_000);
    }
    for unit in query_units(normalized_query) {
        if unit != normalized_query && normalized_content.contains(&unit) {
            score = score.saturating_add(1_000 + unit.chars().count() as u64 * 10);
        }
    }
    score
}

type PersistedUserMessages = HashMap<String, HashSet<String>>;

fn load_persisted_user_messages_at(
    candidates: &[MemoryCandidateV1],
    sessions_index_path: &Path,
    sessions_root: &Path,
) -> Result<PersistedUserMessages, String> {
    let sessions = match fs::read_to_string(sessions_index_path) {
        Ok(raw) if raw.trim().is_empty() => Vec::new(),
        Ok(raw) => serde_json::from_str::<Vec<crate::store::SessionMeta>>(&raw)
            .map_err(|error| format!("memory recall sessions index: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("memory recall sessions index: {error}")),
    };
    let live_session_ids = sessions
        .into_iter()
        .map(|session| session.id)
        .collect::<HashSet<_>>();
    let source_session_ids = candidates
        .iter()
        .filter(|candidate| candidate.status == MemoryCandidateStatusV1::Approved)
        .map(|candidate| candidate.source.session_id.as_str())
        .filter(|session_id| live_session_ids.contains(*session_id))
        .collect::<BTreeSet<_>>();

    let mut persisted = HashMap::new();
    for session_id in source_session_ids {
        let path = sessions_root.join(session_id).join("messages.json");
        let Ok(raw) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(messages) = serde_json::from_str::<Vec<crate::store::ChatMessageStored>>(&raw)
        else {
            continue;
        };
        persisted.insert(
            session_id.to_string(),
            messages
                .into_iter()
                .filter(|message| message.role == "user")
                .map(|message| message.id)
                .collect(),
        );
    }
    Ok(persisted)
}

fn recall_candidates_from(
    query: &str,
    candidates: &[MemoryCandidateV1],
    persisted_user_messages: &PersistedUserMessages,
) -> Vec<MemoryRecallCandidateV1> {
    let normalized_query = normalize_for_match(query);
    let mut ranked = candidates
        .iter()
        .filter(|candidate| candidate.status == MemoryCandidateStatusV1::Approved)
        .filter(|candidate| {
            persisted_user_messages
                .get(&candidate.source.session_id)
                .is_some_and(|messages| messages.contains(&candidate.source.message_id))
        })
        .filter(|candidate| candidate.content.chars().count() <= MAX_MEMORY_CONTEXT_ITEM_CHARS)
        .filter_map(|candidate| {
            let relevance_score = relevance_score(&normalized_query, &candidate.content);
            (relevance_score > 0).then_some((candidate, relevance_score))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut total_chars = 0_usize;
    let mut recalled = Vec::new();
    for (candidate, relevance_score) in ranked {
        if recalled.len() >= MAX_MEMORY_CONTEXT_PACK_ITEMS {
            break;
        }
        let content_chars = candidate.content.chars().count();
        let Some(next_total) = total_chars.checked_add(content_chars) else {
            continue;
        };
        if next_total > MAX_MEMORY_CONTEXT_TOTAL_CHARS {
            continue;
        }
        total_chars = next_total;
        recalled.push(MemoryRecallCandidateV1 {
            candidate_id: candidate.id.clone(),
            content_hash: candidate.content_hash.clone(),
            candidate_type: candidate.candidate_type,
            content: candidate.content.clone(),
            provenance: candidate.source.clone(),
            relevance_score,
        });
    }
    recalled
}

fn bounded_snippet(value: &str) -> String {
    let value = value.trim();
    if value.chars().count() <= MAX_MEMORY_RECALL_EVIDENCE_SNIPPET_CHARS {
        return value.to_string();
    }
    let mut bounded = value
        .chars()
        .take(MAX_MEMORY_RECALL_EVIDENCE_SNIPPET_CHARS.saturating_sub(1))
        .collect::<String>();
    bounded.push('…');
    bounded
}

fn recall_evidence_from(
    mut hits: Vec<SessionSearchResultV1>,
    current_session_id: Option<&str>,
) -> Vec<MemoryRecallEvidenceV1> {
    hits.retain(|hit| current_session_id != Some(hit.session_id.as_str()));
    hits.sort_by(|left, right| {
        left.rank
            .total_cmp(&right.rank)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.message_id.cmp(&right.message_id))
    });

    let mut seen = HashSet::new();
    hits.into_iter()
        .filter(|hit| seen.insert((hit.session_id.clone(), hit.message_id.clone())))
        .filter_map(|hit| {
            let snippet = bounded_snippet(&crate::store::redact_text(&hit.snippet));
            (!snippet.is_empty()).then_some(MemoryRecallEvidenceV1 {
                session_id: hit.session_id,
                session_title: hit.session_title,
                message_id: hit.message_id,
                role: hit.role,
                snippet,
                rank: hit.rank,
                evidence_only: true,
                untrusted: true,
            })
        })
        .take(MAX_MEMORY_RECALL_EVIDENCE_ITEMS)
        .collect()
}

fn assemble_preview(
    candidates: Vec<MemoryRecallCandidateV1>,
    evidence: Vec<MemoryRecallEvidenceV1>,
) -> MemoryRecallPreviewV1 {
    let context_pack_selections = candidates
        .iter()
        .map(|candidate| MemoryCandidateMutationRequestV1 {
            id: candidate.candidate_id.clone(),
            expected_content_hash: candidate.content_hash.clone(),
        })
        .collect();
    MemoryRecallPreviewV1 {
        version: MEMORY_RECALL_VERSION,
        memory_candidates: candidates,
        context_pack_selections,
        session_evidence: evidence,
    }
}

/// Build an explicit recall preview. The returned value is not sent to ACP or
/// the Runtime; a future caller must expose a separate, user-visible decision.
pub fn preview_memory_recall_v1(
    request: MemoryRecallRequestV1,
) -> Result<MemoryRecallPreviewV1, String> {
    let request = validate_request(&request)?;
    let candidates = crate::memory_candidates::list()?;
    let persisted_user_messages = load_persisted_user_messages_at(
        &candidates,
        &crate::paths::sessions_index_file(),
        &crate::paths::app_data_root().join("sessions"),
    )?;
    let recalled = recall_candidates_from(&request.query, &candidates, &persisted_user_messages);
    let search_hits =
        crate::session_search::search(&request.query, Some(SESSION_SEARCH_FETCH_LIMIT))?;
    let evidence = recall_evidence_from(search_hits, request.current_session_id.as_deref());
    Ok(assemble_preview(recalled, evidence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn candidate(
        id: &str,
        status: MemoryCandidateStatusV1,
        content: String,
        session_id: &str,
        message_id: &str,
    ) -> MemoryCandidateV1 {
        MemoryCandidateV1 {
            id: id.into(),
            status,
            candidate_type: MemoryCandidateTypeV1::UserPreference,
            content_hash: format!("hash-{id}"),
            content,
            source: MemoryCandidateSourceV1 {
                session_id: session_id.into(),
                message_id: message_id.into(),
            },
            ownership: crate::memory_candidates::MemoryCandidateOwnershipV1::HostCandidate,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sources(items: &[(&str, &[&str])]) -> PersistedUserMessages {
        items
            .iter()
            .map(|(session_id, message_ids)| {
                (
                    (*session_id).to_string(),
                    message_ids.iter().map(|id| (*id).to_string()).collect(),
                )
            })
            .collect()
    }

    fn hit(session_id: &str, message_id: &str, rank: f64, snippet: &str) -> SessionSearchResultV1 {
        SessionSearchResultV1 {
            version: 1,
            session_id: session_id.into(),
            session_title: format!("Session {session_id}"),
            message_id: message_id.into(),
            role: "assistant".into(),
            snippet: snippet.into(),
            rank,
        }
    }

    #[test]
    fn request_and_preview_reject_unknown_fields_and_invalid_queries() {
        assert!(serde_json::from_value::<MemoryRecallRequestV1>(json!({
            "version": 1,
            "query": "rust",
            "unknown": true,
        }))
        .is_err());
        assert!(serde_json::from_value::<MemoryRecallPreviewV1>(json!({
            "version": 1,
            "memoryCandidates": [],
            "contextPackSelections": [],
            "sessionEvidence": [],
            "unknown": true,
        }))
        .is_err());

        for query in ["", "  \n\t ", "!!!"] {
            assert!(validate_request(&MemoryRecallRequestV1 {
                version: 1,
                query: query.into(),
                current_session_id: None,
            })
            .is_err());
        }
        assert!(validate_request(&MemoryRecallRequestV1 {
            version: 1,
            query: "x".repeat(MAX_MEMORY_RECALL_QUERY_CHARS + 1),
            current_session_id: None,
        })
        .is_err());
        assert!(validate_request(&MemoryRecallRequestV1 {
            version: 2,
            query: "rust".into(),
            current_session_id: None,
        })
        .is_err());
    }

    #[test]
    fn candidate_recall_is_approved_only_and_requires_live_user_provenance() {
        let candidates = vec![
            candidate(
                "approved",
                MemoryCandidateStatusV1::Approved,
                "Prefer Rust for local tools".into(),
                "live",
                "user-1",
            ),
            candidate(
                "pending",
                MemoryCandidateStatusV1::Pending,
                "Prefer Rust for pending tools".into(),
                "live",
                "user-1",
            ),
            candidate(
                "deleted-session",
                MemoryCandidateStatusV1::Approved,
                "Prefer Rust in a deleted session".into(),
                "deleted",
                "user-2",
            ),
            candidate(
                "deleted-message",
                MemoryCandidateStatusV1::Approved,
                "Prefer Rust from a deleted message".into(),
                "live",
                "missing-user",
            ),
        ];
        let recalled =
            recall_candidates_from("rust", &candidates, &sources(&[("live", &["user-1"])]));
        assert_eq!(
            recalled
                .iter()
                .map(|item| item.candidate_id.as_str())
                .collect::<Vec<_>>(),
            vec!["approved"]
        );
    }

    #[test]
    fn candidate_recall_is_deterministic_bounded_and_context_pack_compatible() {
        let valid_sources = sources(&[(
            "live",
            &[
                "m0", "m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8", "m9", "large",
            ],
        )]);
        let mut candidates = (0..10)
            .map(|index| {
                candidate(
                    &format!("candidate-{index:02}"),
                    MemoryCandidateStatusV1::Approved,
                    format!("rust {}", "x".repeat(600)),
                    "live",
                    &format!("m{index}"),
                )
            })
            .collect::<Vec<_>>();
        candidates.push(candidate(
            "too-large",
            MemoryCandidateStatusV1::Approved,
            format!("rust {}", "x".repeat(MAX_MEMORY_CONTEXT_ITEM_CHARS)),
            "live",
            "large",
        ));

        let first = recall_candidates_from("rust", &candidates, &valid_sources);
        candidates.reverse();
        let second = recall_candidates_from("rust", &candidates, &valid_sources);
        assert_eq!(first, second);
        assert!(first.len() <= MAX_MEMORY_CONTEXT_PACK_ITEMS);
        assert!(
            first
                .iter()
                .map(|item| item.content.chars().count())
                .sum::<usize>()
                <= MAX_MEMORY_CONTEXT_TOTAL_CHARS
        );
        assert!(first.iter().all(|item| item.candidate_id != "too-large"));

        let preview = assemble_preview(first.clone(), Vec::new());
        assert_eq!(preview.context_pack_selections.len(), first.len());
        for (selection, recalled) in preview.context_pack_selections.iter().zip(first) {
            assert_eq!(selection.id, recalled.candidate_id);
            assert_eq!(selection.expected_content_hash, recalled.content_hash);
        }
    }

    #[test]
    fn cjk_candidate_query_uses_phrase_and_bigram_relevance() {
        let candidates = vec![
            candidate(
                "relevant",
                MemoryCandidateStatusV1::Approved,
                "用户喜欢简洁的回答，并要求给出验证证据".into(),
                "live",
                "m1",
            ),
            candidate(
                "irrelevant",
                MemoryCandidateStatusV1::Approved,
                "项目使用 SQLite 保存本地数据".into(),
                "live",
                "m2",
            ),
        ];
        let recalled = recall_candidates_from(
            "喜欢简洁回答",
            &candidates,
            &sources(&[("live", &["m1", "m2"])]),
        );
        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].candidate_id, "relevant");
    }

    #[test]
    fn session_evidence_excludes_current_session_and_is_untrusted_and_bounded() {
        let long = "x".repeat(MAX_MEMORY_RECALL_EVIDENCE_SNIPPET_CHARS + 50);
        let mut hits = vec![hit("current", "m0", -10.0, "current session")];
        hits.extend((0..12).map(|index| {
            hit(
                &format!("session-{index:02}"),
                &format!("message-{index:02}"),
                index as f64,
                &long,
            )
        }));
        let evidence = recall_evidence_from(hits, Some("current"));
        assert_eq!(evidence.len(), MAX_MEMORY_RECALL_EVIDENCE_ITEMS);
        assert!(evidence.iter().all(|item| item.session_id != "current"));
        assert!(evidence
            .iter()
            .all(|item| item.evidence_only && item.untrusted));
        assert!(evidence.iter().all(|item| {
            item.snippet.chars().count() <= MAX_MEMORY_RECALL_EVIDENCE_SNIPPET_CHARS
        }));

        let mut reversed = (0..12)
            .rev()
            .map(|index| {
                hit(
                    &format!("session-{index:02}"),
                    &format!("message-{index:02}"),
                    index as f64,
                    "bounded",
                )
            })
            .collect::<Vec<_>>();
        let first = recall_evidence_from(reversed.clone(), None);
        reversed.reverse();
        assert_eq!(first, recall_evidence_from(reversed, None));
    }

    #[test]
    fn provenance_reads_do_not_modify_json_fact_sources() {
        let root = std::env::temp_dir().join(format!(
            "sunsetz-memory-recall-read-only-{}",
            uuid::Uuid::new_v4()
        ));
        let sessions_root = root.join("sessions");
        let session_dir = sessions_root.join("live");
        fs::create_dir_all(&session_dir).unwrap();
        let sessions_index_path = root.join("sessions_index.json");
        let now = Utc::now();
        let sessions = vec![crate::store::SessionMeta {
            id: "live".into(),
            project_id: None,
            title: "Live".into(),
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
        }];
        let messages = vec![crate::store::ChatMessageStored {
            id: "user-1".into(),
            role: "user".into(),
            content: "Remember Rust".into(),
            thought: None,
            created_at: now,
            is_error: false,
            attachments: None,
            marker: None,
        }];
        fs::write(
            &sessions_index_path,
            serde_json::to_vec_pretty(&sessions).unwrap(),
        )
        .unwrap();
        let messages_path = session_dir.join("messages.json");
        fs::write(
            &messages_path,
            serde_json::to_vec_pretty(&messages).unwrap(),
        )
        .unwrap();
        let before_index = fs::read(&sessions_index_path).unwrap();
        let before_messages = fs::read(&messages_path).unwrap();

        let candidates = vec![candidate(
            "approved",
            MemoryCandidateStatusV1::Approved,
            "Prefer Rust".into(),
            "live",
            "user-1",
        )];
        let persisted =
            load_persisted_user_messages_at(&candidates, &sessions_index_path, &sessions_root)
                .unwrap();
        assert!(persisted["live"].contains("user-1"));
        assert_eq!(fs::read(&sessions_index_path).unwrap(), before_index);
        assert_eq!(fs::read(&messages_path).unwrap(), before_messages);

        let _ = fs::remove_dir_all(root);
    }
}
