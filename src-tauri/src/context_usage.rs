//! Exact context usage recovered from Runtime telemetry.
//!
//! ACP `turn_completed.usage` is cumulative across every model call in an
//! agent turn. Context occupancy is not that sum: it is the prompt/output size
//! of the final inference. The Runtime writes that per-inference measurement to
//! `logs/unified.jsonl`, so we pair the ACP event with the latest matching
//! session telemetry entry.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub reasoning_tokens: u64,
}

fn json_u64(value: &Value, camel: &str, snake: &str) -> u64 {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn parse_inference_line(line: &str, agent_session_id: &str) -> Option<InferenceUsage> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("sid").and_then(Value::as_str) != Some(agent_session_id)
        || value.get("msg").and_then(Value::as_str) != Some("shell.turn.inference_done")
    {
        return None;
    }
    let ctx = value.get("ctx")?;
    let input_tokens = json_u64(ctx, "promptTokens", "prompt_tokens");
    let output_tokens = json_u64(ctx, "completionTokens", "completion_tokens");
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }
    Some(InferenceUsage {
        input_tokens,
        output_tokens,
        cached_read_tokens: json_u64(ctx, "cachedPromptTokens", "cached_prompt_tokens"),
        reasoning_tokens: json_u64(ctx, "reasoningTokens", "reasoning_tokens"),
    })
}

/// Read only the tail of the Runtime log; newest matching inference wins.
pub fn latest_inference_usage(agent_home: &Path, agent_session_id: &str) -> Option<InferenceUsage> {
    if agent_session_id.trim().is_empty() {
        return None;
    }
    let path = agent_home.join("logs").join("unified.jsonl");
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    const MAX_TAIL_BYTES: u64 = 1024 * 1024;
    let start = len.saturating_sub(MAX_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut raw = String::new();
    file.read_to_string(&mut raw).ok()?;
    for line in raw.lines().rev() {
        if let Some(usage) = parse_inference_line(line, agent_session_id) {
            return Some(usage);
        }
    }
    None
}

/// Context capacity from provider model metadata.
///
/// DeepSeek's official V4 API publishes a 1M context window. Unknown custom
/// models intentionally return `None`; the UI must not invent a denominator.
pub fn context_window_tokens(model_id: &str) -> Option<u64> {
    let id = model_id.trim().to_ascii_lowercase();
    match id.as_str() {
        "deepseek-v4-pro" | "deepseek-v4-flash" => Some(1_000_000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_inference_measurement() {
        let line = r#"{"sid":"s-1","msg":"shell.turn.inference_done","ctx":{"prompt_tokens":34128,"cached_prompt_tokens":26752,"completion_tokens":641,"reasoning_tokens":53}}"#;
        assert_eq!(
            parse_inference_line(line, "s-1"),
            Some(InferenceUsage {
                input_tokens: 34_128,
                output_tokens: 641,
                cached_read_tokens: 26_752,
                reasoning_tokens: 53,
            })
        );
        assert!(parse_inference_line(line, "other").is_none());
    }

    #[test]
    fn exposes_only_known_context_windows() {
        assert_eq!(context_window_tokens("deepseek-v4-pro"), Some(1_000_000));
        assert_eq!(context_window_tokens("custom-model"), None);
    }
}
