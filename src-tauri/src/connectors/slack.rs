use super::*;
use serde_json::{json, Value};

const MAX_TEXT_CHARS: usize = 4_000;

fn api_base() -> String {
    std::env::var("SUNSETZ_SLACK_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://slack.com".into())
}

fn http_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    connector_http_client(timeout_secs, "SUNSETZ_SLACK_API_URL")
}

fn apply_headers(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    request
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "Sunsetz-Desktop/1.0")
}

fn clip(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_none() {
        text.to_string()
    } else {
        head
    }
}

fn slack_error_name(payload: &str) -> String {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| payload.chars().take(180).collect())
}

fn is_auth_error(name: &str) -> bool {
    matches!(
        name,
        "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" | "invalid_token"
    )
}

fn map_status(status: u16, payload: &str) -> String {
    let error = slack_error_name(payload);
    if status == 401 || status == 403 || is_auth_error(&error) {
        return format!("CONNECTOR_AUTH_FAILED: {error}");
    }
    format!("slack: {status} {error}")
}

fn parse_ok_payload(status: u16, payload: &str) -> Result<Value, String> {
    if status == 401 || status == 403 {
        return Err(map_status(status, payload));
    }
    let value: Value = serde_json::from_str(payload).unwrap_or_else(|_| json!({ "raw": payload }));
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error");
        if is_auth_error(error) {
            return Err(format!("CONNECTOR_AUTH_FAILED: {error}"));
        }
        return Err(format!("slack: {error}"));
    }
    if !(200..300).contains(&status) {
        return Err(format!("slack: {status} {payload}"));
    }
    Ok(value)
}

async fn slack_send(
    client: &reqwest::Client,
    method: reqwest::Method,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<(u16, String), String> {
    let mut request = apply_headers(
        client.request(method, format!("{}{path}", api_base())),
        token,
    );
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: could not reach Slack ({error})"))?;
    let status = response.status().as_u16();
    let payload = response
        .text()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?;
    Ok((status, payload))
}

async fn slack_json(
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let token = load_credential("slack")
        .map(|value| github::normalize_token(&value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "CONNECTOR_CREDENTIAL_MISSING: Slack is not connected".to_string())?;
    let client = http_client(INVOKE_TIMEOUT_SECS)?;
    let (status, payload) = slack_send(&client, method, path, &token, body).await?;
    parse_ok_payload(status, &payload)
}

fn compact_conversations(payload: &Value) -> Value {
    payload
        .get("channels")
        .and_then(Value::as_array)
        .map(|items| {
            json!(items
                .iter()
                .take(20)
                .map(|item| json!({
                    "id": item.get("id"),
                    "name": item.get("name"),
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or_else(|| payload.clone())
}

fn compact_messages(payload: &Value) -> Value {
    payload
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| {
            json!(items
                .iter()
                .take(20)
                .map(|item| json!({
                    "user": item.get("user"),
                    "text": item.get("text"),
                    "ts": item.get("ts"),
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or_else(|| payload.clone())
}

pub async fn verify_token(token: &str) -> Result<(), String> {
    let token = github::normalize_token(token);
    if token.is_empty() {
        return Err("CONNECTOR_CREDENTIAL_MISSING: paste a Slack bot token".into());
    }
    let client = http_client(GITHUB_CONNECT_TIMEOUT_SECS)?;
    let (status, payload) = slack_send(
        &client,
        reqwest::Method::POST,
        "/api/auth.test",
        &token,
        Some(&json!({})),
    )
    .await?;
    parse_ok_payload(status, &payload).map(|_| ())
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "slack_list_conversations",
                "description": "List public Slack channels the bot can see.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "slack_get_conversation",
                "description": "Read recent messages from one Slack channel.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "channel": { "type": "string", "description": "Channel id, e.g. C01234567." },
                        "limit": { "type": "integer", "description": "1–20. Defaults to 20." }
                    },
                    "required": ["channel"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "slack_post_message",
                "description": "Post a message to a Slack channel. Requires permission.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "channel": { "type": "string", "description": "Channel id or #name the bot can post to." },
                        "text": { "type": "string", "description": "Plain-text message." }
                    },
                    "required": ["channel", "text"]
                }
            }
        }),
    ]
}

pub fn tool_names() -> Vec<String> {
    google_oauth::names_from_defs(&tool_definitions())
}

pub fn write_tools() -> Vec<String> {
    vec!["slack_post_message".into()]
}

pub async fn invoke(name: &str, arguments: &Value) -> String {
    let result = match name {
        "slack_list_conversations" => slack_json(
            reqwest::Method::GET,
            "/api/conversations.list?limit=20&exclude_archived=true&types=public_channel",
            None,
        )
        .await
        .map(|payload| compact_conversations(&payload)),
        "slack_get_conversation" => {
            let channel = google_oauth::arg_str(arguments, "channel");
            if channel.is_empty() {
                return "slack_get_conversation requires `channel`".into();
            }
            let limit = google_oauth::arg_u64(arguments, "limit")
                .unwrap_or(20)
                .clamp(1, 20);
            let encoded = google_oauth::percent_encode(&channel);
            slack_json(
                reqwest::Method::GET,
                &format!("/api/conversations.history?channel={encoded}&limit={limit}"),
                None,
            )
            .await
            .map(|payload| compact_messages(&payload))
        }
        "slack_post_message" => {
            let channel = google_oauth::arg_str(arguments, "channel");
            let text = google_oauth::arg_str(arguments, "text");
            if channel.is_empty() || text.is_empty() {
                return "slack_post_message requires `channel` and `text`".into();
            }
            slack_json(
                reqwest::Method::POST,
                "/api/chat.postMessage",
                Some(&json!({
                    "channel": channel,
                    "text": clip(&text, MAX_TEXT_CHARS),
                })),
            )
            .await
            .map(|payload| {
                json!({
                    "channel": payload.get("channel"),
                    "ts": payload.get("ts"),
                    "ok": payload.get("ok"),
                })
            })
        }
        other => return format!("unknown tool `{other}`"),
    };
    match result {
        Ok(payload) => bound_output(payload.to_string()),
        Err(error) => error,
    }
}
