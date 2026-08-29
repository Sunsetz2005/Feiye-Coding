use super::*;
use base64::Engine;
use serde_json::{json, Value};

fn api_base() -> String {
    std::env::var("SUNSETZ_GMAIL_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://gmail.googleapis.com".into())
}

fn verify_url() -> String {
    format!("{}/gmail/v1/users/me/profile", api_base())
}

async fn gmail_json(
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    google_oauth::google_json(method, format!("{}{path}", api_base()), body).await
}

fn rfc822_draft(to: &str, subject: &str, body: &str) -> String {
    format!("To: {to}\r\nSubject: {subject}\r\n\r\n{body}")
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "gmail_list_messages",
                "description": "List recent Gmail messages. Optional `query` uses Gmail search syntax.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Gmail search query, e.g. from:alice newer_than:7d." },
                        "max_results": { "type": "integer", "description": "1–20. Defaults to 10." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "gmail_get_message",
                "description": "Get one Gmail message by id, including a plain-text snippet.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Gmail message id." }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "gmail_create_draft",
                "description": "Create a Gmail draft. Requires permission.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "to": { "type": "string", "description": "Recipient email." },
                        "subject": { "type": "string", "description": "Subject line." },
                        "body": { "type": "string", "description": "Plain-text body." }
                    },
                    "required": ["to", "subject"]
                }
            }
        }),
    ]
}

pub fn tool_names() -> Vec<String> {
    google_oauth::names_from_defs(&tool_definitions())
}

pub fn write_tools() -> Vec<String> {
    vec!["gmail_create_draft".into()]
}

pub async fn connect(credential: Option<&str>) -> Result<(), String> {
    google_oauth::connect_google_app(google_oauth::GMAIL_SCOPES, credential, |token| async move {
        google_oauth::verify_with_token(&verify_url(), &token).await
    })
    .await
}

pub async fn invoke(name: &str, arguments: &Value) -> String {
    let result = match name {
        "gmail_list_messages" => {
            let query = google_oauth::arg_str(arguments, "query");
            let max = google_oauth::arg_u64(arguments, "max_results")
                .unwrap_or(10)
                .clamp(1, 20);
            let mut path = format!("/gmail/v1/users/me/messages?maxResults={max}");
            if !query.is_empty() {
                path.push_str("&q=");
                path.push_str(&google_oauth::percent_encode(&query));
            }
            gmail_json(reqwest::Method::GET, &path, None).await
        }
        "gmail_get_message" => {
            let id = google_oauth::arg_str(arguments, "id");
            if id.is_empty() {
                return "gmail_get_message requires `id`".into();
            }
            gmail_json(
                reqwest::Method::GET,
                &format!("/gmail/v1/users/me/messages/{id}?format=metadata"),
                None,
            )
            .await
        }
        "gmail_create_draft" => {
            let to = google_oauth::arg_str(arguments, "to");
            let subject = google_oauth::arg_str(arguments, "subject");
            if to.is_empty() || subject.is_empty() {
                return "gmail_create_draft requires `to` and `subject`".into();
            }
            let raw = rfc822_draft(&to, &subject, &google_oauth::arg_str(arguments, "body"));
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
            gmail_json(
                reqwest::Method::POST,
                "/gmail/v1/users/me/drafts",
                Some(&json!({ "message": { "raw": encoded } })),
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
