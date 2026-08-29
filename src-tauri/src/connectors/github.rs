use super::*;
use serde_json::{json, Value};

pub(super) fn normalize_token(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches(|ch| ch == '"' || ch == '\'');
    let without_scheme = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .or_else(|| trimmed.strip_prefix("token "))
        .unwrap_or(trimmed);
    without_scheme.trim().to_string()
}

fn api_base() -> String {
    std::env::var("SUNSETZ_GITHUB_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.github.com".into())
}

fn apply_github_headers(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    request
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "Sunsetz-Desktop/1.0")
}

fn github_message(payload: &str) -> String {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| payload.chars().take(180).collect())
}

fn map_github_status(status: u16, payload: &str) -> String {
    let message = github_message(payload);
    if status == 401 {
        return format!("CONNECTOR_AUTH_FAILED: {message}");
    }
    if status == 403 {
        let lower = message.to_ascii_lowercase();
        if lower.contains("saml") || lower.contains("sso") {
            return format!("CONNECTOR_AUTH_FAILED: GitHub SSO required. {message}");
        }
        return format!("CONNECTOR_AUTH_FAILED: {message}");
    }
    format!("github: {status} {message}")
}

async fn github_send(
    client: &reqwest::Client,
    method: reqwest::Method,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<(u16, String), String> {
    let mut request = apply_github_headers(
        client.request(method, format!("{}{path}", api_base())),
        token,
    );
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: could not reach GitHub ({error})"))?;
    let status = response.status().as_u16();
    let payload = response
        .text()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?;
    Ok((status, payload))
}

async fn github_json(
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let token = load_credential("github")
        .map(|value| normalize_token(&value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "CONNECTOR_CREDENTIAL_MISSING: GitHub is not connected".to_string())?;
    let client = github_http_client(INVOKE_TIMEOUT_SECS)?;
    let (status, payload) = github_send(&client, method, path, &token, body).await?;
    if status == 401 || status == 403 {
        return Err(map_github_status(status, &payload));
    }
    if !(200..300).contains(&status) {
        return Err(format!("github: {status} {payload}"));
    }
    serde_json::from_str(&payload).or_else(|_| Ok(json!({ "raw": payload })))
}

async fn github_get(path: &str) -> Result<Value, String> {
    github_json(reqwest::Method::GET, path, None).await
}

async fn github_post(path: &str, body: &Value) -> Result<Value, String> {
    github_json(reqwest::Method::POST, path, Some(body)).await
}

fn compact_items(payload: &Value, extra_skip: &[&str]) -> Value {
    payload
        .as_array()
        .map(|items| {
            json!(items
                .iter()
                .filter(|item| extra_skip.iter().all(|key| item.get(key).is_none()))
                .take(20)
                .map(|item| json!({
                    "number": item.get("number"),
                    "title": item.get("title"),
                    "state": item.get("state"),
                    "html_url": item.get("html_url"),
                    "user": item.pointer("/user/login"),
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or_else(|| payload.clone())
}

pub async fn verify_token(token: &str) -> Result<String, String> {
    let token = normalize_token(token);
    if token.is_empty() {
        return Err("CONNECTOR_CREDENTIAL_MISSING: paste a GitHub personal access token".into());
    }
    let client = github_http_client(GITHUB_CONNECT_TIMEOUT_SECS)?;
    let (status, payload) =
        github_send(&client, reqwest::Method::GET, "/user", &token, None).await?;
    if (200..300).contains(&status) {
        let body: Value = serde_json::from_str(&payload).unwrap_or(json!({}));
        return Ok(body
            .get("login")
            .and_then(Value::as_str)
            .unwrap_or("github")
            .to_string());
    }
    if status == 401 {
        return Err(map_github_status(status, &payload));
    }
    // Fine-grained tokens with only repo access cannot read /user (403).
    // /rate_limit is enough to prove the token is accepted.
    if status == 403 {
        let (limit_status, limit_payload) =
            github_send(&client, reqwest::Method::GET, "/rate_limit", &token, None).await?;
        if (200..300).contains(&limit_status) {
            return Ok("github".into());
        }
        if limit_status == 401 || limit_status == 403 {
            return Err(map_github_status(limit_status, &limit_payload));
        }
        return Err(map_github_status(status, &payload));
    }
    Err(format!("CONNECTOR_UNREACHABLE: GitHub returned {status}"))
}

pub fn tool_definitions() -> Vec<Value> {
    let owner_repo = json!({
        "owner": { "type": "string", "description": "GitHub owner or organization." },
        "repo": { "type": "string", "description": "Repository name." }
    });
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "github_list_pull_requests",
                "description": "List pull requests in a GitHub repository.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "owner": owner_repo["owner"].clone(),
                        "repo": owner_repo["repo"].clone(),
                        "state": { "type": "string", "description": "open, closed, or all. Defaults to open." }
                    },
                    "required": ["owner", "repo"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "github_list_issues",
                "description": "List issues in a GitHub repository (pull requests excluded).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "owner": owner_repo["owner"].clone(),
                        "repo": owner_repo["repo"].clone(),
                        "state": { "type": "string", "description": "open, closed, or all. Defaults to open." }
                    },
                    "required": ["owner", "repo"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "github_get_item",
                "description": "Get one issue or pull request by number.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "owner": owner_repo["owner"].clone(),
                        "repo": owner_repo["repo"].clone(),
                        "number": { "type": "integer", "description": "Issue or pull request number." }
                    },
                    "required": ["owner", "repo", "number"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "github_create_issue",
                "description": "Create an issue in a GitHub repository. Requires permission.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "owner": owner_repo["owner"].clone(),
                        "repo": owner_repo["repo"].clone(),
                        "title": { "type": "string", "description": "Issue title." },
                        "body": { "type": "string", "description": "Optional Markdown body." }
                    },
                    "required": ["owner", "repo", "title"]
                }
            }
        }),
    ]
}

pub fn tool_names() -> Vec<String> {
    google_oauth::names_from_defs(&tool_definitions())
}

pub fn write_tools() -> Vec<String> {
    vec!["github_create_issue".into()]
}

pub async fn invoke(name: &str, arguments: &Value) -> String {
    let owner = google_oauth::arg_str(arguments, "owner");
    let repo = google_oauth::arg_str(arguments, "repo");
    if owner.is_empty() || repo.is_empty() {
        return "github tools require `owner` and `repo`".into();
    }
    let result = match name {
        "github_list_pull_requests" => {
            let state = google_oauth::arg_str(arguments, "state");
            let state = if state.is_empty() { "open" } else { &state };
            github_get(&format!(
                "/repos/{owner}/{repo}/pulls?state={state}&per_page=20"
            ))
            .await
            .map(|payload| compact_items(&payload, &[]))
        }
        "github_list_issues" => {
            let state = google_oauth::arg_str(arguments, "state");
            let state = if state.is_empty() { "open" } else { &state };
            github_get(&format!(
                "/repos/{owner}/{repo}/issues?state={state}&per_page=20"
            ))
            .await
            .map(|payload| compact_items(&payload, &["pull_request"]))
        }
        "github_get_item" => {
            let Some(number) = google_oauth::arg_u64(arguments, "number") else {
                return "github_get_item requires `number`".into();
            };
            github_get(&format!("/repos/{owner}/{repo}/issues/{number}")).await
        }
        "github_create_issue" => {
            let title = google_oauth::arg_str(arguments, "title");
            if title.is_empty() {
                return "github_create_issue requires `title`".into();
            }
            github_post(
                &format!("/repos/{owner}/{repo}/issues"),
                &json!({
                    "title": title,
                    "body": google_oauth::arg_str(arguments, "body"),
                }),
            )
            .await
        }
        other => return format!("unknown tool `{other}`"),
    };
    match result {
        Ok(payload) => bound_output(payload.to_string()),
        Err(error) => error,
    }
}
