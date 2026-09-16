//! Built-in kernel context compact.
//!
//! The default Sunsetz agent loop owns compaction. Visible journal history is
//! not rewritten; a per-session sidecar stores the model-facing summary and
//! the first retained message id. `/compact` is a Host command, not a chat
//! question. Auto-compact runs when occupancy is ≥ 85% of a known window, or
//! when reconstructed history would otherwise be silently truncated.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::store::{ChatMessageStored, SessionTokenUsage};

pub const COMPACT_STORE_VERSION: u8 = 1;
pub const AUTO_COMPACT_PERCENT: u64 = 85;
pub const KEEP_RECENT_MESSAGES: usize = 6;
pub const MAX_HISTORY_MESSAGES: usize = 24;
pub const MAX_HISTORY_CHARS: usize = 32_768;
pub const MAX_MESSAGE_CHARS: usize = 4_000;
pub const MAX_SUMMARY_CHARS: usize = 4_000;
pub const MAX_NOTE_CHARS: usize = 500;
pub const MAX_OLDER_CHARS: usize = 48_000;
pub const SUMMARY_PREVIEW_CHARS: usize = 500;
const MAX_SESSION_ID_CHARS: usize = 256;

const SUMMARY_USER_PREFIX: &str = "Compacted prior context:\n\n";
const SUMMARY_ASSISTANT_ACK: &str =
    "Acknowledged. I will continue from the compacted prior context and the recent turns.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactCommand {
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactError {
    NothingToCompact,
    Cancelled,
    EmptySummary,
    Provider(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextCompactV1 {
    pub version: u8,
    pub trigger: String,
    pub summary: String,
    pub cutoff_message_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_message_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CompactPlan<'a> {
    pub older_blocks: Vec<String>,
    pub retained: Vec<&'a ChatMessageStored>,
    pub cutoff_message_id: String,
    pub retained_message_ids: Vec<String>,
}

pub fn parse_manual_command(prompt: &str) -> Option<CompactCommand> {
    let trimmed = prompt.trim();
    let rest = strip_compact_prefix(trimmed)?;
    if rest.is_empty() {
        return Some(CompactCommand { note: None });
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let note = rest.trim();
    if note.is_empty() {
        Some(CompactCommand { note: None })
    } else {
        Some(CompactCommand {
            note: Some(note.chars().take(MAX_NOTE_CHARS).collect()),
        })
    }
}

fn strip_compact_prefix(trimmed: &str) -> Option<&str> {
    const HEAD: &str = "/compact";
    if trimmed.len() < HEAD.len() {
        return None;
    }
    if !trimmed.get(..HEAD.len())?.eq_ignore_ascii_case(HEAD) {
        return None;
    }
    Some(&trimmed[HEAD.len()..])
}

pub fn is_compact_command_text(content: &str) -> bool {
    parse_manual_command(content).is_some()
}

pub fn eligible_model_turns(messages: &[ChatMessageStored]) -> Vec<&ChatMessageStored> {
    messages
        .iter()
        .filter(|message| is_model_turn(message))
        .collect()
}

fn is_model_turn(message: &ChatMessageStored) -> bool {
    if message.is_error || message.content.trim().is_empty() {
        return false;
    }
    if let Some(marker) = message.marker.as_deref() {
        if marker == "tool_step"
            || marker == "turn_cancelled"
            || marker == "context_compact"
            || marker.starts_with("memory")
        {
            return false;
        }
    }
    if message.role != "user" && message.role != "assistant" {
        return false;
    }
    !is_compact_command_text(&message.content)
}

fn drop_current_user<'a>(mut turns: Vec<&'a ChatMessageStored>) -> Vec<&'a ChatMessageStored> {
    if turns.last().is_some_and(|message| message.role == "user") {
        turns.pop();
    }
    turns
}

pub fn artifact_applies(messages: &[ChatMessageStored], artifact: &ContextCompactV1) -> bool {
    if artifact.version != COMPACT_STORE_VERSION {
        return false;
    }
    if artifact.summary.trim().is_empty() || artifact.cutoff_message_id.trim().is_empty() {
        return false;
    }
    messages
        .iter()
        .any(|message| message.id == artifact.cutoff_message_id)
}

fn turns_after_cutoff<'a>(
    messages: &'a [ChatMessageStored],
    cutoff_message_id: &str,
) -> Vec<&'a ChatMessageStored> {
    let Some(index) = messages
        .iter()
        .position(|message| message.id == cutoff_message_id)
    else {
        return Vec::new();
    };
    eligible_model_turns(&messages[index..])
}

pub fn plan_compact<'a>(
    messages: &'a [ChatMessageStored],
    artifact: Option<&ContextCompactV1>,
) -> Result<CompactPlan<'a>, CompactError> {
    let (mut older_source, recent_pool): (Vec<String>, Vec<&ChatMessageStored>) =
        if let Some(existing) = artifact.filter(|item| artifact_applies(messages, item)) {
            let after =
                drop_current_user(turns_after_cutoff(messages, &existing.cutoff_message_id));
            let mut older = vec![existing.summary.clone()];
            if after.len() > KEEP_RECENT_MESSAGES {
                older.extend(
                    after[..after.len() - KEEP_RECENT_MESSAGES]
                        .iter()
                        .map(|message| format_block(message)),
                );
            }
            (older, after)
        } else {
            let turns = drop_current_user(eligible_model_turns(messages));
            let older = if turns.len() > KEEP_RECENT_MESSAGES {
                turns[..turns.len() - KEEP_RECENT_MESSAGES]
                    .iter()
                    .map(|message| format_block(message))
                    .collect()
            } else {
                Vec::new()
            };
            (older, turns)
        };

    if recent_pool.len() <= KEEP_RECENT_MESSAGES && older_source.is_empty() {
        return Err(CompactError::NothingToCompact);
    }
    if recent_pool.len() <= KEEP_RECENT_MESSAGES {
        return Err(CompactError::NothingToCompact);
    }

    let retained = recent_pool[recent_pool.len() - KEEP_RECENT_MESSAGES..].to_vec();
    let cutoff_message_id = retained
        .first()
        .map(|message| message.id.clone())
        .ok_or(CompactError::NothingToCompact)?;
    trim_older_blocks(&mut older_source);
    if older_source.is_empty() {
        return Err(CompactError::NothingToCompact);
    }
    Ok(CompactPlan {
        older_blocks: older_source,
        retained_message_ids: retained.iter().map(|message| message.id.clone()).collect(),
        retained,
        cutoff_message_id,
    })
}

fn format_block(message: &ChatMessageStored) -> String {
    let role = if message.role == "user" {
        "User"
    } else {
        "Assistant"
    };
    let content: String = message.content.chars().take(MAX_MESSAGE_CHARS).collect();
    format!("### {role}\n{content}")
}

fn trim_older_blocks(blocks: &mut Vec<String>) {
    let mut chars = blocks
        .iter()
        .map(|block| block.chars().count())
        .sum::<usize>();
    while chars > MAX_OLDER_CHARS && blocks.len() > 1 {
        let removed = blocks.remove(0);
        chars = chars.saturating_sub(removed.chars().count());
    }
}

pub fn should_auto_compact(
    messages: &[ChatMessageStored],
    artifact: Option<&ContextCompactV1>,
    last_usage: Option<&SessionTokenUsage>,
) -> bool {
    if plan_compact(messages, artifact).is_err() {
        return false;
    }
    if occupancy_at_or_above_threshold(last_usage) {
        return true;
    }
    would_hard_truncate(messages, artifact)
}

fn occupancy_at_or_above_threshold(last_usage: Option<&SessionTokenUsage>) -> bool {
    let Some(usage) = last_usage else {
        return false;
    };
    let Some(window) = usage.context_window_tokens.filter(|window| *window > 0) else {
        return false;
    };
    usage.used_tokens.saturating_mul(100) >= window.saturating_mul(AUTO_COMPACT_PERCENT)
}

fn would_hard_truncate(
    messages: &[ChatMessageStored],
    artifact: Option<&ContextCompactV1>,
) -> bool {
    let turns = model_turns_for_history(messages, artifact);
    if turns.len() > MAX_HISTORY_MESSAGES {
        return true;
    }
    let mut chars = 0usize;
    for message in &turns {
        chars = chars.saturating_add(message.content.chars().take(MAX_MESSAGE_CHARS).count());
        if chars > MAX_HISTORY_CHARS {
            return true;
        }
    }
    false
}

fn model_turns_for_history<'a>(
    messages: &'a [ChatMessageStored],
    artifact: Option<&ContextCompactV1>,
) -> Vec<&'a ChatMessageStored> {
    if let Some(existing) = artifact.filter(|item| artifact_applies(messages, item)) {
        drop_current_user(turns_after_cutoff(messages, &existing.cutoff_message_id))
    } else {
        drop_current_user(eligible_model_turns(messages))
    }
}

pub fn history_for_model(
    messages: &[ChatMessageStored],
    artifact: Option<&ContextCompactV1>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let turns = if let Some(existing) = artifact.filter(|item| artifact_applies(messages, item)) {
        out.extend(summary_prefix(&existing.summary));
        model_turns_for_history(messages, Some(existing))
    } else {
        model_turns_for_history(messages, None)
    };
    out.extend(truncate_newest(turns));
    out
}

fn summary_prefix(summary: &str) -> Vec<Value> {
    let summary = summary.trim();
    if summary.is_empty() {
        return Vec::new();
    }
    vec![
        json!({
            "role": "user",
            "content": format!("{SUMMARY_USER_PREFIX}{summary}"),
        }),
        json!({
            "role": "assistant",
            "content": SUMMARY_ASSISTANT_ACK,
        }),
    ]
}

fn truncate_newest(turns: Vec<&ChatMessageStored>) -> Vec<Value> {
    let turns = if turns.len() > MAX_HISTORY_MESSAGES {
        turns[turns.len() - MAX_HISTORY_MESSAGES..].to_vec()
    } else {
        turns
    };
    let mut newest_first = Vec::new();
    let mut chars = 0usize;
    for message in turns.iter().rev() {
        let content: String = message.content.chars().take(MAX_MESSAGE_CHARS).collect();
        let next = chars.saturating_add(content.chars().count());
        if next > MAX_HISTORY_CHARS && !newest_first.is_empty() {
            break;
        }
        chars = next;
        newest_first.push(json!({
            "role": message.role,
            "content": content,
        }));
    }
    newest_first.reverse();
    newest_first
}

pub fn build_summarize_messages(plan: &CompactPlan<'_>, note: Option<&str>) -> Vec<Value> {
    let mut body = String::from(
        "Compress the earlier turns of a coding-agent session into a factual summary \
for future model context. Keep user goals, decisions, file paths, constraints, and errors. \
Omit greetings, duplicated tool noise, and secrets. Write plain prose. Do not start the next task.\n\n",
    );
    if let Some(note) = note.map(str::trim).filter(|value| !value.is_empty()) {
        body.push_str("User note (prefer keeping):\n");
        body.push_str(note);
        body.push_str("\n\n");
    }
    body.push_str("Earlier conversation:\n\n");
    body.push_str(&plan.older_blocks.join("\n\n"));
    vec![
        json!({
            "role": "system",
            "content": "You write compact session summaries for another model. No tools. No chit-chat.",
        }),
        json!({
            "role": "user",
            "content": body,
        }),
    ]
}

pub fn finish_artifact(
    plan: CompactPlan<'_>,
    summary: String,
    trigger: &str,
    note: Option<&str>,
    model_id: Option<&str>,
    tokens_before: Option<u64>,
) -> Result<ContextCompactV1, CompactError> {
    let summary: String = summary.trim().chars().take(MAX_SUMMARY_CHARS).collect();
    if summary.is_empty() {
        return Err(CompactError::EmptySummary);
    }
    let trigger = if trigger.eq_ignore_ascii_case("manual") {
        "manual"
    } else {
        "auto"
    };
    Ok(ContextCompactV1 {
        version: COMPACT_STORE_VERSION,
        trigger: trigger.into(),
        summary,
        cutoff_message_id: plan.cutoff_message_id,
        retained_message_ids: plan.retained_message_ids,
        tokens_before,
        tokens_after: None,
        note: note
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(MAX_NOTE_CHARS).collect()),
        model_id: model_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        created_at: Utc::now(),
    })
}

pub fn summary_preview(summary: &str) -> Option<String> {
    let preview: String = summary.trim().chars().take(SUMMARY_PREVIEW_CHARS).collect();
    if preview.is_empty() {
        None
    } else {
        Some(preview)
    }
}

pub fn remap_artifact_ids(
    artifact: &ContextCompactV1,
    id_map: &HashMap<String, String>,
) -> Option<ContextCompactV1> {
    let cutoff = id_map.get(&artifact.cutoff_message_id)?.clone();
    let retained: Vec<String> = artifact
        .retained_message_ids
        .iter()
        .filter_map(|id| id_map.get(id).cloned())
        .collect();
    if retained.is_empty() {
        return None;
    }
    let mut next = artifact.clone();
    next.cutoff_message_id = cutoff;
    next.retained_message_ids = retained;
    Some(next)
}

pub async fn run_compact<F, Fut>(
    messages: &[ChatMessageStored],
    artifact: Option<&ContextCompactV1>,
    note: Option<&str>,
    trigger: &str,
    last_usage: Option<&SessionTokenUsage>,
    model_id: Option<&str>,
    complete: F,
) -> Result<ContextCompactV1, CompactError>
where
    F: FnOnce(Vec<Value>) -> Fut,
    Fut: Future<Output = Result<String, CompactError>>,
{
    let plan = plan_compact(messages, artifact)?;
    let request = build_summarize_messages(&plan, note);
    let summary = complete(request).await?;
    finish_artifact(
        plan,
        summary,
        trigger,
        note,
        model_id,
        last_usage.map(|usage| usage.used_tokens),
    )
}

fn validate_session_id(session_id: &str) -> Result<String, String> {
    let value = session_id.trim();
    if value.is_empty() || value.chars().count() > MAX_SESSION_ID_CHARS {
        return Err("invalid compact session id".into());
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("invalid compact session id".into());
    }
    Ok(value.to_string())
}

pub fn store_path(session_id: &str) -> Result<PathBuf, String> {
    let session_id = validate_session_id(session_id)?;
    Ok(crate::paths::session_dir(&session_id).join("context-compact.v1.json"))
}

pub fn load_from_path(path: &std::path::Path) -> Result<Option<ContextCompactV1>, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(None),
        Ok(raw) => {
            let artifact: ContextCompactV1 = serde_json::from_str(&raw)
                .map_err(|error| format!("parse {}: {error}", path.display()))?;
            if artifact.version != COMPACT_STORE_VERSION
                || artifact.summary.trim().is_empty()
                || artifact.cutoff_message_id.trim().is_empty()
            {
                return Ok(None);
            }
            Ok(Some(artifact))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

pub fn save_to_path(path: &std::path::Path, artifact: &ContextCompactV1) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("compact dir: {error}"))?;
    }
    let owned = artifact.clone();
    crate::store_lock::update_json_locked(
        path,
        || owned.clone(),
        |current| {
            *current = owned.clone();
            Ok(())
        },
    )
}

pub fn load_v1(session_id: &str) -> Result<Option<ContextCompactV1>, String> {
    load_from_path(&store_path(session_id)?)
}

pub fn save_v1(session_id: &str, artifact: &ContextCompactV1) -> Result<(), String> {
    save_to_path(&store_path(session_id)?, artifact)
}

pub fn delete_v1(session_id: &str) -> Result<(), String> {
    let path = store_path(session_id)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove {}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_message(id: &str, role: &str, content: &str) -> ChatMessageStored {
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

    fn turns(pairs: &[(&str, &str, &str)]) -> Vec<ChatMessageStored> {
        pairs
            .iter()
            .map(|(id, role, content)| sample_message(id, role, content))
            .collect()
    }

    fn usage(used: u64, window: Option<u64>) -> SessionTokenUsage {
        SessionTokenUsage {
            used_tokens: used,
            input_tokens: used,
            output_tokens: 0,
            cached_read_tokens: 0,
            reasoning_tokens: 0,
            turn_input_tokens: used,
            turn_output_tokens: 0,
            model_calls: 1,
            model_id: "deepseek-v4-pro".into(),
            context_window_tokens: window,
            updated_at: Utc::now(),
            source: "runtime".into(),
        }
    }

    #[test]
    fn parses_manual_compact_commands() {
        assert_eq!(
            parse_manual_command("/compact"),
            Some(CompactCommand { note: None })
        );
        assert_eq!(
            parse_manual_command("  /COMPACT keep auth  "),
            Some(CompactCommand {
                note: Some("keep auth".into())
            })
        );
        assert!(parse_manual_command("/compaction").is_none());
        assert!(parse_manual_command("please /compact").is_none());
        assert!(parse_manual_command("compact this").is_none());
        assert!(parse_manual_command("性能优化建议").is_none());
    }

    #[test]
    fn journal_history_drops_current_user_and_markers() {
        let history = history_for_model(
            &[
                sample_message("u1", "user", "first"),
                sample_message("a1", "assistant", "ok"),
                ChatMessageStored {
                    marker: Some("tool_step".into()),
                    ..sample_message("t1", "tool", "ignored")
                },
                sample_message("u2", "user", "current"),
            ],
            None,
        );
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["content"], "first");
        assert_eq!(history[1]["content"], "ok");
    }

    #[test]
    fn history_skips_compact_slash_commands() {
        let history = history_for_model(
            &[
                sample_message("u1", "user", "first"),
                sample_message("a1", "assistant", "ok"),
                sample_message("c1", "user", "/compact keep auth"),
            ],
            None,
        );
        assert_eq!(history.len(), 2);
        assert_eq!(history[1]["content"], "ok");
    }

    #[test]
    fn char_truncation_keeps_newest_turns() {
        let mut messages = Vec::new();
        for index in 0..6 {
            messages.push(sample_message(
                &format!("u{index}"),
                "user",
                &"x".repeat(8_000),
            ));
            messages.push(sample_message(
                &format!("a{index}"),
                "assistant",
                &"y".repeat(8_000),
            ));
        }
        messages.push(sample_message("now", "user", "current"));
        let history = history_for_model(&messages, None);
        assert!(!history.is_empty());
        assert_eq!(
            history.last().unwrap()["content"]
                .as_str()
                .unwrap()
                .chars()
                .next(),
            Some('y')
        );
        let chars: usize = history
            .iter()
            .map(|item| item["content"].as_str().unwrap().chars().count())
            .sum();
        assert!(chars <= MAX_HISTORY_CHARS);
    }

    #[test]
    fn plan_keeps_recent_turns_and_summarizes_the_rest() {
        let mut pairs = Vec::new();
        for index in 0..6 {
            pairs.push((format!("u{index}"), "user".to_string(), format!("q{index}")));
            pairs.push((
                format!("a{index}"),
                "assistant".to_string(),
                format!("a{index}"),
            ));
        }
        let messages: Vec<ChatMessageStored> = pairs
            .iter()
            .map(|(id, role, content)| sample_message(id, role, content))
            .collect();
        let plan = plan_compact(&messages, None).expect("enough history");
        assert_eq!(plan.retained.len(), KEEP_RECENT_MESSAGES);
        assert_eq!(plan.cutoff_message_id, "u3");
        assert!(plan.older_blocks.join("\n").contains("q0"));
        assert!(!plan.older_blocks.join("\n").contains("q5"));
    }

    #[test]
    fn too_short_history_is_not_compacted() {
        let messages = turns(&[("u1", "user", "hi"), ("a1", "assistant", "hello")]);
        assert!(matches!(
            plan_compact(&messages, None),
            Err(CompactError::NothingToCompact)
        ));
        assert!(!should_auto_compact(&messages, None, None));
    }

    #[test]
    fn auto_compact_uses_known_window_threshold() {
        let mut messages = Vec::new();
        for index in 0..8 {
            messages.push(sample_message(&format!("u{index}"), "user", "q"));
            messages.push(sample_message(&format!("a{index}"), "assistant", "a"));
        }
        assert!(!should_auto_compact(
            &messages,
            None,
            Some(&usage(84, Some(100)))
        ));
        assert!(should_auto_compact(
            &messages,
            None,
            Some(&usage(85, Some(100)))
        ));
        assert!(!should_auto_compact(
            &messages,
            None,
            Some(&usage(90_000, None))
        ));
    }

    #[test]
    fn auto_compact_when_hard_truncation_would_drop_turns() {
        let mut messages = Vec::new();
        for index in 0..(MAX_HISTORY_MESSAGES + 4) {
            messages.push(sample_message(&format!("u{index}"), "user", "q"));
            messages.push(sample_message(&format!("a{index}"), "assistant", "a"));
        }
        assert!(should_auto_compact(&messages, None, None));
    }

    #[test]
    fn history_uses_sidecar_summary_and_cutoff() {
        let messages = turns(&[
            ("u1", "user", "old question"),
            ("a1", "assistant", "old answer"),
            ("u2", "user", "recent q"),
            ("a2", "assistant", "recent a"),
            ("u3", "user", "now"),
        ]);
        let artifact = ContextCompactV1 {
            version: 1,
            trigger: "manual".into(),
            summary: "Auth lives in session.ts".into(),
            cutoff_message_id: "u2".into(),
            retained_message_ids: vec!["u2".into(), "a2".into()],
            tokens_before: Some(9_000),
            tokens_after: None,
            note: Some("keep auth".into()),
            model_id: None,
            created_at: Utc::now(),
        };
        let history = history_for_model(&messages, Some(&artifact));
        assert_eq!(history[0]["role"], "user");
        assert!(history[0]["content"]
            .as_str()
            .unwrap()
            .contains("Auth lives in session.ts"));
        assert_eq!(history[1]["role"], "assistant");
        assert_eq!(history[2]["content"], "recent q");
        assert_eq!(history[3]["content"], "recent a");
        assert!(history.iter().all(|item| item["content"] != "old question"));
        assert!(history.iter().all(|item| item["content"] != "now"));
    }

    #[test]
    fn stale_cutoff_falls_back_to_truncation() {
        let messages = turns(&[("u1", "user", "first"), ("a1", "assistant", "ok")]);
        let artifact = ContextCompactV1 {
            version: 1,
            trigger: "auto".into(),
            summary: "gone".into(),
            cutoff_message_id: "missing".into(),
            retained_message_ids: vec!["missing".into()],
            tokens_before: None,
            tokens_after: None,
            note: None,
            model_id: None,
            created_at: Utc::now(),
        };
        let history = history_for_model(&messages, Some(&artifact));
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["content"], "first");
    }

    #[test]
    fn remap_requires_cutoff_in_the_fork() {
        let artifact = ContextCompactV1 {
            version: 1,
            trigger: "auto".into(),
            summary: "keep".into(),
            cutoff_message_id: "a1".into(),
            retained_message_ids: vec!["a1".into(), "u2".into()],
            tokens_before: None,
            tokens_after: None,
            note: None,
            model_id: None,
            created_at: Utc::now(),
        };
        let mut map = HashMap::new();
        map.insert("a1".into(), "fork-0".into());
        map.insert("u2".into(), "fork-1".into());
        let remapped = remap_artifact_ids(&artifact, &map).unwrap();
        assert_eq!(remapped.cutoff_message_id, "fork-0");
        assert_eq!(remapped.retained_message_ids, vec!["fork-0", "fork-1"]);
        assert!(remap_artifact_ids(&artifact, &HashMap::new()).is_none());
    }

    #[tokio::test]
    async fn run_compact_writes_summary_without_inventing_tokens_after() {
        let mut messages = Vec::new();
        for index in 0..8 {
            messages.push(sample_message(&format!("u{index}"), "user", "q"));
            messages.push(sample_message(&format!("a{index}"), "assistant", "a"));
        }
        let artifact = run_compact(
            &messages,
            None,
            Some("keep auth"),
            "manual",
            Some(&usage(12_000, Some(100_000))),
            Some("deepseek-v4-pro"),
            |_request| async { Ok("Auth is in session.ts.".into()) },
        )
        .await
        .unwrap();
        assert_eq!(artifact.trigger, "manual");
        assert_eq!(artifact.summary, "Auth is in session.ts.");
        assert_eq!(artifact.note.as_deref(), Some("keep auth"));
        assert_eq!(artifact.tokens_before, Some(12_000));
        assert_eq!(artifact.tokens_after, None);
        assert_eq!(artifact.retained_message_ids.len(), KEEP_RECENT_MESSAGES);
    }

    #[tokio::test]
    async fn run_compact_rejects_empty_model_summary() {
        let mut messages = Vec::new();
        for index in 0..8 {
            messages.push(sample_message(&format!("u{index}"), "user", "q"));
            messages.push(sample_message(&format!("a{index}"), "assistant", "a"));
        }
        let error = run_compact(
            &messages,
            None,
            None,
            "auto",
            None,
            None,
            |_request| async { Ok("   ".into()) },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, CompactError::EmptySummary));
    }

    #[test]
    fn save_and_load_roundtrip_on_temp_path() {
        let root = std::env::temp_dir().join(format!(
            "sunsetz-compact-roundtrip-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("context-compact.v1.json");
        let artifact = ContextCompactV1 {
            version: 1,
            trigger: "auto".into(),
            summary: "summary body".into(),
            cutoff_message_id: "m1".into(),
            retained_message_ids: vec!["m1".into()],
            tokens_before: Some(100),
            tokens_after: None,
            note: None,
            model_id: Some("deepseek-v4-pro".into()),
            created_at: Utc::now(),
        };
        save_to_path(&path, &artifact).unwrap();
        let loaded = load_from_path(&path).unwrap().unwrap();
        assert_eq!(loaded.summary, "summary body");
        assert_eq!(loaded.cutoff_message_id, "m1");
        let _ = std::fs::remove_dir_all(&root);
    }
}
