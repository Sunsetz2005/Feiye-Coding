use super::*;
use serde_json::{json, Value};

fn api_base() -> String {
    std::env::var("SUNSETZ_DRIVE_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://www.googleapis.com".into())
}

fn verify_url() -> String {
    format!("{}/drive/v3/about?fields=user", api_base())
}

async fn drive_json(
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    google_oauth::google_json(method, format!("{}{path}", api_base()), body).await
}

fn compact_files(payload: &Value) -> Value {
    payload
        .get("files")
        .and_then(Value::as_array)
        .map(|items| {
            json!(items
                .iter()
                .take(20)
                .map(|item| json!({
                    "id": item.get("id"),
                    "name": item.get("name"),
                    "mimeType": item.get("mimeType"),
                    "modifiedTime": item.get("modifiedTime"),
                    "webViewLink": item.get("webViewLink"),
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or_else(|| payload.clone())
}

fn export_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "application/vnd.google-apps.document" => Some("text/plain"),
        "application/vnd.google-apps.spreadsheet" => Some("text/csv"),
        "application/vnd.google-apps.presentation" => Some("text/plain"),
        _ => None,
    }
}

fn multipart_create(name: &str, body: &str) -> (String, String) {
    let boundary = "sunsetz-drive";
    let metadata = json!({
        "name": name,
        "mimeType": "application/vnd.google-apps.document",
    });
    let payload = format!(
        "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{body}\r\n--{boundary}--\r\n",
        metadata = metadata
    );
    (format!("multipart/related; boundary={boundary}"), payload)
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "google-drive_list_files",
                "description": "List or search Google Drive files. Optional `q` uses Drive search syntax.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "q": { "type": "string", "description": "Drive query, e.g. name contains 'briefing' and trashed = false." },
                        "max_results": { "type": "integer", "description": "1–20. Defaults to 10." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "google-drive_get_file",
                "description": "Get Drive file metadata. Google Docs, Sheets, and Slides are exported as text when possible.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Drive file id." }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "google-drive_create_file",
                "description": "Create a Google Doc from a title and optional plain text. Requires permission.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Document title." },
                        "body": { "type": "string", "description": "Optional plain-text body." }
                    },
                    "required": ["name"]
                }
            }
        }),
    ]
}

pub fn tool_names() -> Vec<String> {
    google_oauth::names_from_defs(&tool_definitions())
}

pub fn write_tools() -> Vec<String> {
    vec!["google-drive_create_file".into()]
}

pub async fn connect(credential: Option<&str>) -> Result<(), String> {
    google_oauth::connect_google_app(google_oauth::DRIVE_SCOPES, credential, |token| async move {
        google_oauth::verify_with_token(&verify_url(), &token).await
    })
    .await
}

pub async fn invoke(name: &str, arguments: &Value) -> String {
    let result = match name {
        "google-drive_list_files" => {
            let query = google_oauth::arg_str(arguments, "q");
            let max = google_oauth::arg_u64(arguments, "max_results")
                .unwrap_or(10)
                .clamp(1, 20);
            let mut path = format!(
                "/drive/v3/files?pageSize={max}&fields=files(id,name,mimeType,modifiedTime,webViewLink)"
            );
            if !query.is_empty() {
                path.push_str("&q=");
                path.push_str(&google_oauth::percent_encode(&query));
            }
            drive_json(reqwest::Method::GET, &path, None)
                .await
                .map(|payload| compact_files(&payload))
        }
        "google-drive_get_file" => {
            let id = google_oauth::arg_str(arguments, "id");
            if id.is_empty() {
                return "google-drive_get_file requires `id`".into();
            }
            let encoded = google_oauth::percent_encode(&id);
            let meta = drive_json(
                reqwest::Method::GET,
                &format!(
                    "/drive/v3/files/{encoded}?fields=id,name,mimeType,modifiedTime,webViewLink,size"
                ),
                None,
            )
            .await;
            match meta {
                Ok(mut payload) => {
                    let mime = payload
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if let Some(export) = export_mime(&mime) {
                        let export_path = format!(
                            "/drive/v3/files/{encoded}/export?mimeType={}",
                            google_oauth::percent_encode(export)
                        );
                        if let Ok(text) = google_oauth::google_send(
                            reqwest::Method::GET,
                            format!("{}{export_path}", api_base()),
                            None,
                            None,
                            None,
                        )
                        .await
                        {
                            if (200..300).contains(&text.0) {
                                let snippet: String = text.1.chars().take(8_192).collect();
                                payload["text"] = json!(snippet);
                            }
                        }
                    }
                    Ok(payload)
                }
                Err(error) => Err(error),
            }
        }
        "google-drive_create_file" => {
            let name = google_oauth::arg_str(arguments, "name");
            if name.is_empty() {
                return "google-drive_create_file requires `name`".into();
            }
            let body = google_oauth::arg_str(arguments, "body");
            if body.is_empty() {
                drive_json(
                    reqwest::Method::POST,
                    "/drive/v3/files",
                    Some(&json!({
                        "name": name,
                        "mimeType": "application/vnd.google-apps.document",
                    })),
                )
                .await
            } else {
                let (content_type, raw) = multipart_create(&name, &body);
                let (status, payload) = match google_oauth::google_send(
                    reqwest::Method::POST,
                    format!("{}/upload/drive/v3/files?uploadType=multipart", api_base()),
                    None,
                    Some(&content_type),
                    Some(raw),
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => return error,
                };
                if !(200..300).contains(&status) {
                    return format!("google: {status} {payload}");
                }
                serde_json::from_str(&payload).or_else(|_| Ok(json!({ "raw": payload })))
            }
        }
        other => return format!("unknown tool `{other}`"),
    };
    match result {
        Ok(payload) => bound_output(payload.to_string()),
        Err(error) => error,
    }
}
