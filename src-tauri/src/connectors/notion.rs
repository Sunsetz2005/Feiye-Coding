use super::*;
use serde_json::{json, Value};

const NOTION_VERSION: &str = "2022-06-28";
const MAX_TITLE_CHARS: usize = 2_000;

fn api_base() -> String {
    std::env::var("SUNSETZ_NOTION_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.notion.com".into())
}

fn http_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    connector_http_client(timeout_secs, "SUNSETZ_NOTION_API_URL")
}

fn apply_headers(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    request
        .header("Authorization", format!("Bearer {token}"))
        .header("Notion-Version", NOTION_VERSION)
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

fn notion_message(payload: &str) -> String {
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

fn map_status(status: u16, payload: &str) -> String {
    let message = notion_message(payload);
    if status == 401 || status == 403 {
        return format!("CONNECTOR_AUTH_FAILED: {message}");
    }
    format!("notion: {status} {message}")
}

async fn notion_send(
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
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: could not reach Notion ({error})"))?;
    let status = response.status().as_u16();
    let payload = response
        .text()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?;
    Ok((status, payload))
}

async fn notion_json(
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let token = load_credential("notion")
        .map(|value| github::normalize_token(&value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "CONNECTOR_CREDENTIAL_MISSING: Notion is not connected".to_string())?;
    let client = http_client(INVOKE_TIMEOUT_SECS)?;
    let (status, payload) = notion_send(&client, method, path, &token, body).await?;
    if status == 401 || status == 403 {
        return Err(map_status(status, &payload));
    }
    if !(200..300).contains(&status) {
        return Err(format!("notion: {status} {payload}"));
    }
    serde_json::from_str(&payload).or_else(|_| Ok(json!({ "raw": payload })))
}

fn rich_text_plain(value: &Value) -> String {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("plain_text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn page_title(page: &Value) -> String {
    if let Some(props) = page.get("properties").and_then(Value::as_object) {
        for prop in props.values() {
            if prop.get("type").and_then(Value::as_str) == Some("title") {
                let text = rich_text_plain(prop.get("title").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    return text;
                }
            }
        }
    }
    page.get("url")
        .and_then(Value::as_str)
        .unwrap_or("untitled")
        .to_string()
}

fn blocks_plain(payload: &Value) -> String {
    payload
        .get("results")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    let kind = block.get("type")?.as_str()?;
                    Some(rich_text_plain(
                        block.get(kind)?.get("rich_text").unwrap_or(&Value::Null),
                    ))
                })
                .filter(|text| !text.is_empty())
                .take(20)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn compact_search(payload: &Value) -> Value {
    payload
        .get("results")
        .and_then(Value::as_array)
        .map(|items| {
            json!(items
                .iter()
                .take(20)
                .map(|item| json!({
                    "id": item.get("id"),
                    "url": item.get("url"),
                    "title": page_title(item),
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or_else(|| payload.clone())
}

fn compact_page(page: &Value, blocks: Option<&Value>) -> Value {
    json!({
        "id": page.get("id"),
        "url": page.get("url"),
        "title": page_title(page),
        "text": blocks.map(blocks_plain).unwrap_or_default(),
    })
}

pub async fn verify_token(token: &str) -> Result<(), String> {
    let token = github::normalize_token(token);
    if token.is_empty() {
        return Err(
            "CONNECTOR_CREDENTIAL_MISSING: paste a Notion internal integration token".into(),
        );
    }
    let client = http_client(GITHUB_CONNECT_TIMEOUT_SECS)?;
    let (status, payload) =
        notion_send(&client, reqwest::Method::GET, "/v1/users/me", &token, None).await?;
    if (200..300).contains(&status) {
        return Ok(());
    }
    if status == 401 || status == 403 {
        return Err(map_status(status, &payload));
    }
    Err(format!("CONNECTOR_UNREACHABLE: Notion returned {status}"))
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "notion_search",
                "description": "Search Notion pages the connected integration can access.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search text. Empty lists recent accessible pages." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "notion_get_page",
                "description": "Get one Notion page title, URL, and a bounded plain-text excerpt.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "page_id": { "type": "string", "description": "Notion page id." }
                    },
                    "required": ["page_id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "notion_create_page",
                "description": "Create a Notion page under an existing page. Requires permission.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "parent_page_id": { "type": "string", "description": "Parent page id the integration can write to." },
                        "title": { "type": "string", "description": "Page title." },
                        "content": { "type": "string", "description": "Optional plain-text body as one paragraph." }
                    },
                    "required": ["parent_page_id", "title"]
                }
            }
        }),
    ]
}

pub fn tool_names() -> Vec<String> {
    google_oauth::names_from_defs(&tool_definitions())
}

pub fn write_tools() -> Vec<String> {
    vec!["notion_create_page".into()]
}

pub async fn invoke(name: &str, arguments: &Value) -> String {
    let result = match name {
        "notion_search" => {
            let query = google_oauth::arg_str(arguments, "query");
            notion_json(
                reqwest::Method::POST,
                "/v1/search",
                Some(&json!({
                    "query": query,
                    "page_size": 20,
                    "filter": { "value": "page", "property": "object" }
                })),
            )
            .await
            .map(|payload| compact_search(&payload))
        }
        "notion_get_page" => {
            let id = google_oauth::arg_str(arguments, "page_id");
            if id.is_empty() {
                return "notion_get_page requires `page_id`".into();
            }
            let encoded = google_oauth::percent_encode(&id);
            match notion_json(reqwest::Method::GET, &format!("/v1/pages/{encoded}"), None).await {
                Ok(page) => {
                    let blocks = notion_json(
                        reqwest::Method::GET,
                        &format!("/v1/blocks/{encoded}/children?page_size=20"),
                        None,
                    )
                    .await
                    .ok();
                    Ok(compact_page(&page, blocks.as_ref()))
                }
                Err(error) => Err(error),
            }
        }
        "notion_create_page" => {
            let parent = google_oauth::arg_str(arguments, "parent_page_id");
            let title = google_oauth::arg_str(arguments, "title");
            if parent.is_empty() || title.is_empty() {
                return "notion_create_page requires `parent_page_id` and `title`".into();
            }
            let content = google_oauth::arg_str(arguments, "content");
            let mut payload = json!({
                "parent": { "page_id": parent },
                "properties": {
                    "title": {
                        "title": [{ "text": { "content": clip(&title, MAX_TITLE_CHARS) } }]
                    }
                }
            });
            if !content.is_empty() {
                payload["children"] = json!([{
                    "object": "block",
                    "type": "paragraph",
                    "paragraph": {
                        "rich_text": [{
                            "type": "text",
                            "text": { "content": clip(&content, MAX_TITLE_CHARS) }
                        }]
                    }
                }]);
            }
            notion_json(reqwest::Method::POST, "/v1/pages", Some(&payload))
                .await
                .map(|page| compact_page(&page, None))
        }
        other => return format!("unknown tool `{other}`"),
    };
    match result {
        Ok(payload) => bound_output(payload.to_string()),
        Err(error) => error,
    }
}
