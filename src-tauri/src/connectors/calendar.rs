use super::*;
use serde_json::{json, Value};

fn api_base() -> String {
    std::env::var("SUNSETZ_CALENDAR_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://www.googleapis.com".into())
}

fn verify_url() -> String {
    format!(
        "{}/calendar/v3/users/me/calendarList?maxResults=1",
        api_base()
    )
}

async fn calendar_json(
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    google_oauth::google_json(method, format!("{}{path}", api_base()), body).await
}

fn compact_events(payload: &Value) -> Value {
    payload
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            json!(items
                .iter()
                .take(20)
                .map(|item| json!({
                    "id": item.get("id"),
                    "summary": item.get("summary"),
                    "start": item.get("start"),
                    "end": item.get("end"),
                    "htmlLink": item.get("htmlLink"),
                    "status": item.get("status"),
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or_else(|| payload.clone())
}

fn event_time(raw: &str) -> Value {
    if raw.len() == 10 && raw.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
        json!({ "date": raw })
    } else {
        json!({ "dateTime": raw })
    }
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "google-calendar_list_events",
                "description": "List upcoming Google Calendar events on the primary calendar.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "time_min": { "type": "string", "description": "RFC3339 lower bound. Defaults to now." },
                        "time_max": { "type": "string", "description": "RFC3339 upper bound." },
                        "max_results": { "type": "integer", "description": "1–20. Defaults to 10." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "google-calendar_get_event",
                "description": "Get one Google Calendar event by id from the primary calendar.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Calendar event id." }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "google-calendar_create_event",
                "description": "Create an event on the primary Google Calendar. Requires permission.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "summary": { "type": "string", "description": "Event title." },
                        "start": { "type": "string", "description": "RFC3339 start or YYYY-MM-DD." },
                        "end": { "type": "string", "description": "RFC3339 end or YYYY-MM-DD." },
                        "description": { "type": "string", "description": "Optional event notes." }
                    },
                    "required": ["summary", "start", "end"]
                }
            }
        }),
    ]
}

pub fn tool_names() -> Vec<String> {
    google_oauth::names_from_defs(&tool_definitions())
}

pub fn write_tools() -> Vec<String> {
    vec!["google-calendar_create_event".into()]
}

pub async fn connect(credential: Option<&str>) -> Result<(), String> {
    google_oauth::connect_google_app(
        google_oauth::CALENDAR_SCOPES,
        credential,
        |token| async move { google_oauth::verify_with_token(&verify_url(), &token).await },
    )
    .await
}

pub async fn invoke(name: &str, arguments: &Value) -> String {
    let result = match name {
        "google-calendar_list_events" => {
            let max = google_oauth::arg_u64(arguments, "max_results")
                .unwrap_or(10)
                .clamp(1, 20);
            let mut path = format!(
                "/calendar/v3/calendars/primary/events?singleEvents=true&orderBy=startTime&maxResults={max}"
            );
            let time_min = google_oauth::arg_str(arguments, "time_min");
            if !time_min.is_empty() {
                path.push_str("&timeMin=");
                path.push_str(&google_oauth::percent_encode(&time_min));
            }
            let time_max = google_oauth::arg_str(arguments, "time_max");
            if !time_max.is_empty() {
                path.push_str("&timeMax=");
                path.push_str(&google_oauth::percent_encode(&time_max));
            }
            calendar_json(reqwest::Method::GET, &path, None)
                .await
                .map(|payload| compact_events(&payload))
        }
        "google-calendar_get_event" => {
            let id = google_oauth::arg_str(arguments, "id");
            if id.is_empty() {
                return "google-calendar_get_event requires `id`".into();
            }
            calendar_json(
                reqwest::Method::GET,
                &format!(
                    "/calendar/v3/calendars/primary/events/{}",
                    google_oauth::percent_encode(&id)
                ),
                None,
            )
            .await
        }
        "google-calendar_create_event" => {
            let summary = google_oauth::arg_str(arguments, "summary");
            let start = google_oauth::arg_str(arguments, "start");
            let end = google_oauth::arg_str(arguments, "end");
            if summary.is_empty() || start.is_empty() || end.is_empty() {
                return "google-calendar_create_event requires `summary`, `start`, and `end`"
                    .into();
            }
            let mut body = json!({
                "summary": summary,
                "start": event_time(&start),
                "end": event_time(&end),
            });
            let description = google_oauth::arg_str(arguments, "description");
            if !description.is_empty() {
                body["description"] = json!(description);
            }
            calendar_json(
                reqwest::Method::POST,
                "/calendar/v3/calendars/primary/events",
                Some(&body),
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
