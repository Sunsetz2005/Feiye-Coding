//! Shared Google OAuth (loopback PKCE + refresh) for Gmail, Drive, and Calendar.

use super::*;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::future::Future;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

pub const GOOGLE_CREDENTIAL_ID: &str = "google";
pub const LEGACY_GMAIL_CREDENTIAL_ID: &str = "gmail";
pub const GOOGLE_APP_IDS: &[&str] = &["gmail", "google-drive", "google-calendar"];

pub const GMAIL_SCOPES: &str =
    "https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.compose";
pub const DRIVE_SCOPES: &str =
    "https://www.googleapis.com/auth/drive.readonly https://www.googleapis.com/auth/drive.file";
pub const CALENDAR_SCOPES: &str = "https://www.googleapis.com/auth/calendar.events";

pub fn is_google_app(id: &str) -> bool {
    GOOGLE_APP_IDS.contains(&id)
}

pub fn uses_loopback_mocks() -> bool {
    [
        "SUNSETZ_GMAIL_API_URL",
        "SUNSETZ_DRIVE_API_URL",
        "SUNSETZ_CALENDAR_API_URL",
        "SUNSETZ_GOOGLE_OAUTH_AUTH_URL",
        "SUNSETZ_GOOGLE_OAUTH_TOKEN_URL",
    ]
    .iter()
    .filter_map(|key| std::env::var(key).ok())
    .any(|value| !value.trim().is_empty() && is_loopback_http_url(value.trim()))
}

pub fn http_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("Sunsetz-Desktop/1.0")
        .redirect(reqwest::redirect::Policy::limited(4));
    if uses_loopback_mocks() {
        builder = builder.no_proxy();
    }
    builder.build().map_err(|error| error.to_string())
}

fn token_url() -> String {
    std::env::var("SUNSETZ_GOOGLE_OAUTH_TOKEN_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://oauth2.googleapis.com/token".into())
}

fn auth_url() -> String {
    std::env::var("SUNSETZ_GOOGLE_OAUTH_AUTH_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://accounts.google.com/o/oauth2/v2/auth".into())
}

fn client_id_from_text(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
}

fn bundled_client_id() -> Option<String> {
    client_id_from_text(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/oauth/google-desktop-client-id.txt"
    )))
}

fn home_client_id() -> Option<String> {
    let path = crate::paths::app_data_root().join("google-oauth-client-id.txt");
    fs::read_to_string(path)
        .ok()
        .as_deref()
        .and_then(client_id_from_text)
}

fn client_id() -> Option<String> {
    std::env::var("SUNSETZ_GOOGLE_OAUTH_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(bundled_client_id)
        .or_else(home_client_id)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

pub fn stamp_token_payload(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object
            .entry("obtained_at")
            .or_insert_with(|| json!(now_unix()));
    }
    value
}

pub fn access_token_from_value(value: &Value) -> Option<String> {
    value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

fn token_needs_refresh(value: &Value) -> bool {
    if value.get("refresh_token").and_then(Value::as_str).is_none() {
        return false;
    }
    let obtained = value
        .get("obtained_at")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let expires_in = value.get("expires_in").and_then(Value::as_u64).unwrap_or(0);
    if obtained == 0 || expires_in == 0 {
        return false;
    }
    now_unix() >= obtained.saturating_add(expires_in.saturating_sub(60))
}

pub fn parse_access_token(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("CONNECTOR_CREDENTIAL_MISSING: Google is not connected".into());
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(token) = value.get("access_token").and_then(Value::as_str) {
            if !token.trim().is_empty() {
                return Ok(token.trim().to_string());
            }
        }
    }
    Ok(trimmed.to_string())
}

pub fn load_google_credential() -> Option<String> {
    load_credential(GOOGLE_CREDENTIAL_ID).or_else(|| load_credential(LEGACY_GMAIL_CREDENTIAL_ID))
}

pub fn save_google_credential(raw: &str) -> Result<(), String> {
    save_credential(GOOGLE_CREDENTIAL_ID, raw)
}

pub fn delete_google_credential_if_unused() -> Result<(), String> {
    let listed = list_connectors().unwrap_or_default();
    if listed
        .iter()
        .any(|row| is_google_app(&row.id) && row.connected)
    {
        return Ok(());
    }
    let _ = delete_credential(GOOGLE_CREDENTIAL_ID);
    let _ = delete_credential(LEGACY_GMAIL_CREDENTIAL_ID);
    Ok(())
}

fn stored_scope_string() -> String {
    load_google_credential()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("scope")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default()
}

pub fn union_scopes(existing: &str, extra: &str) -> String {
    let mut seen = std::collections::BTreeSet::new();
    for scope in existing.split_whitespace().chain(extra.split_whitespace()) {
        if !scope.is_empty() {
            seen.insert(scope.to_string());
        }
    }
    seen.into_iter().collect::<Vec<_>>().join(" ")
}

fn merge_token_payload(new: Value, extra_scope: &str) -> Value {
    let mut value = stamp_token_payload(new);
    if value.get("refresh_token").and_then(Value::as_str).is_none() {
        if let Some(old) = load_google_credential() {
            if let Ok(old_value) = serde_json::from_str::<Value>(&old) {
                if let Some(refresh) = old_value.get("refresh_token") {
                    value["refresh_token"] = refresh.clone();
                }
                if value.get("scope").and_then(Value::as_str).is_none() {
                    if let Some(scope) = old_value.get("scope") {
                        value["scope"] = scope.clone();
                    }
                }
            }
        }
    }
    let granted = value.get("scope").and_then(Value::as_str).unwrap_or("");
    let merged = union_scopes(granted, extra_scope);
    if !merged.is_empty() {
        value["scope"] = json!(merged);
    }
    value
}

fn map_google_status(status: u16, payload: &str) -> String {
    let snippet: String = payload.chars().take(180).collect();
    if status == 401 || status == 403 {
        return format!("CONNECTOR_AUTH_FAILED: {snippet}");
    }
    format!("google: {status} {snippet}")
}

async fn refresh_access(refresh_token: &str) -> Result<Value, String> {
    let client_id = client_id().ok_or_else(|| {
        "CONNECTOR_OAUTH_CLIENT_MISSING: Google sign-in is not configured".to_string()
    })?;
    let client = http_client(GITHUB_CONNECT_TIMEOUT_SECS)?;
    let form = [
        ("client_id", client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    let response = client
        .post(token_url())
        .form(&form)
        .send()
        .await
        .map_err(|error| {
            format!("CONNECTOR_UNREACHABLE: could not reach Google OAuth ({error})")
        })?;
    let status = response.status().as_u16();
    let payload = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(map_google_status(status, &payload));
    }
    let mut value: Value = serde_json::from_str(&payload)
        .map_err(|_| "CONNECTOR_AUTH_FAILED: Google token response was not JSON".to_string())?;
    if access_token_from_value(&value).is_none() {
        return Err("CONNECTOR_AUTH_FAILED: Google did not return an access token".into());
    }
    if value.get("refresh_token").and_then(Value::as_str).is_none() {
        value["refresh_token"] = json!(refresh_token);
    }
    Ok(stamp_token_payload(value))
}

pub async fn persist_token_value(value: Value) -> Result<String, String> {
    let stamped = merge_token_payload(value, "");
    let raw = stamped.to_string();
    save_google_credential(&raw)?;
    access_token_from_value(&stamped)
        .ok_or_else(|| "CONNECTOR_AUTH_FAILED: Google did not return an access token".into())
}

pub async fn bearer_token() -> Result<String, String> {
    let raw = load_google_credential()
        .ok_or_else(|| "CONNECTOR_CREDENTIAL_MISSING: Google is not connected".to_string())?;
    if let Ok(value) = serde_json::from_str::<Value>(raw.trim()) {
        if token_needs_refresh(&value) {
            if let Some(refresh) = value
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty())
            {
                let refreshed = refresh_access(refresh).await?;
                return persist_token_value(refreshed).await;
            }
        }
        if let Some(token) = access_token_from_value(&value) {
            return Ok(token);
        }
    }
    parse_access_token(&raw)
}

fn apply_bearer(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    request
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
}

pub async fn google_send(
    method: reqwest::Method,
    url: String,
    body: Option<&Value>,
    content_type: Option<&str>,
    raw_body: Option<String>,
) -> Result<(u16, String), String> {
    let mut token = bearer_token().await?;
    let client = http_client(INVOKE_TIMEOUT_SECS)?;
    let send_once = |token: &str| {
        let mut request = apply_bearer(client.request(method.clone(), &url), token);
        if let Some(raw) = raw_body.as_ref() {
            if let Some(content_type) = content_type {
                request = request.header("Content-Type", content_type);
            }
            request = request.body(raw.clone());
        } else if let Some(body) = body {
            request = request.json(body);
        }
        request
    };
    let response = send_once(&token)
        .send()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: could not reach Google ({error})"))?;
    let mut status = response.status().as_u16();
    let mut payload = response
        .text()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?;
    if status == 401 {
        if let Ok(value) =
            serde_json::from_str::<Value>(&load_google_credential().unwrap_or_default())
        {
            if let Some(refresh) = value
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty())
            {
                if let Ok(refreshed) = refresh_access(refresh).await {
                    if let Ok(next) = persist_token_value(refreshed).await {
                        token = next;
                        let retry = send_once(&token).send().await.map_err(|error| {
                            format!("CONNECTOR_UNREACHABLE: could not reach Google ({error})")
                        })?;
                        status = retry.status().as_u16();
                        payload = retry
                            .text()
                            .await
                            .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?;
                    }
                }
            }
        }
    }
    Ok((status, payload))
}

pub async fn google_json(
    method: reqwest::Method,
    url: String,
    body: Option<&Value>,
) -> Result<Value, String> {
    let (status, payload) = google_send(method, url, body, None, None).await?;
    if !(200..300).contains(&status) {
        return Err(map_google_status(status, &payload));
    }
    serde_json::from_str(&payload).or_else(|_| Ok(json!({ "raw": payload })))
}

pub fn percent_encode(raw: &str) -> String {
    raw.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((high << 4) | low);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pkce_pair() -> (String, String) {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let mut raw = uuid::Uuid::new_v4().as_bytes().to_vec();
    raw.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    let verifier = engine.encode(raw);
    let challenge = engine.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (percent_decode(name) == key).then(|| percent_decode(value))
    })
}

fn oauth_callback_page(ok: bool) -> String {
    let title = if ok {
        "Sunsetz is connected to Google"
    } else {
        "Sunsetz could not connect Google"
    };
    let body = if ok {
        "You can close this window and return to Sunsetz."
    } else {
        "Sign-in was cancelled or failed. You can close this window."
    };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title></head><body style=\"font-family:-apple-system,BlinkMacSystemFont,sans-serif;padding:48px;text-align:center;color:#111\"><h1 style=\"font-size:20px;margin-bottom:8px\">Sunsetz</h1><p>{body}</p></body></html>"
    )
}

fn wait_for_oauth_code(listener: TcpListener, expected_state: String) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?;
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut raw = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                    if raw.windows(4).any(|window| window == b"\r\n\r\n") || raw.len() > 16_384 {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&raw);
                let first = request.lines().next().unwrap_or_default();
                let path = first.split_whitespace().nth(1).unwrap_or("/");
                let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
                let failed = query_value(query, "error").is_some();
                let page = oauth_callback_page(!failed);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                );
                if let Some(error) = query_value(query, "error") {
                    return Err(format!("CONNECTOR_AUTH_FAILED: Google returned {error}"));
                }
                if let Some(got) = query_value(query, "state") {
                    if got != expected_state {
                        return Err(
                            "CONNECTOR_AUTH_FAILED: Google sign-in state did not match".into()
                        );
                    }
                }
                return query_value(query, "code")
                    .filter(|code| !code.is_empty())
                    .ok_or_else(|| {
                        "CONNECTOR_AUTH_FAILED: Google did not return an authorization code".into()
                    });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() > Duration::from_secs(120) {
                    return Err(
                        "CONNECTOR_AUTH_FAILED: Google sign-in timed out or was cancelled".into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(format!("CONNECTOR_UNREACHABLE: {error}"));
            }
        }
    }
}

fn open_system_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

async fn exchange_code(
    client_id: &str,
    redirect_uri: &str,
    code: &str,
    verifier: &str,
) -> Result<Value, String> {
    let client = http_client(GITHUB_CONNECT_TIMEOUT_SECS)?;
    let form = [
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    let response = client
        .post(token_url())
        .form(&form)
        .send()
        .await
        .map_err(|error| {
            format!("CONNECTOR_UNREACHABLE: could not reach Google OAuth ({error})")
        })?;
    let status = response.status().as_u16();
    let payload = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(map_google_status(status, &payload));
    }
    let value: Value = serde_json::from_str(&payload)
        .map_err(|_| "CONNECTOR_AUTH_FAILED: Google token response was not JSON".to_string())?;
    if access_token_from_value(&value).is_none() {
        return Err("CONNECTOR_AUTH_FAILED: Google did not return an access token".into());
    }
    Ok(stamp_token_payload(value))
}

async fn launch_auth(url: &str) -> Result<(), String> {
    if is_loopback_http_url(&auth_url()) {
        let client = http_client(8)?;
        let mut last = "auth endpoint did not respond".to_string();
        for _ in 0..20 {
            match client.get(url).send().await {
                Ok(_) => return Ok(()),
                Err(error) => last = error.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        return Err(format!("CONNECTOR_UNREACHABLE: {last}"));
    }
    open_system_browser(url)
}

async fn oauth_browser(scopes: &str) -> Result<Value, String> {
    let client_id = client_id().ok_or_else(|| {
        "CONNECTOR_OAUTH_CLIENT_MISSING: Google sign-in is not configured".to_string()
    })?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/");
    let (verifier, challenge) = pkce_pair();
    let state = uuid::Uuid::new_v4().to_string();
    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent&include_granted_scopes=true",
        auth_url(),
        percent_encode(&client_id),
        percent_encode(&redirect),
        percent_encode(scopes),
        percent_encode(&state),
        percent_encode(&challenge)
    );
    let wait = tokio::task::spawn_blocking(move || wait_for_oauth_code(listener, state));
    if let Err(error) = launch_auth(&url).await {
        let _ = wait.await;
        return Err(error);
    }
    let code = wait
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: {error}"))??;
    exchange_code(&client_id, &redirect, &code, &verifier).await
}

pub fn arg_str(arguments: &Value, key: &str) -> String {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn arg_u64(arguments: &Value, key: &str) -> Option<u64> {
    arguments.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })
}

pub fn names_from_defs(defs: &[Value]) -> Vec<String> {
    defs.iter()
        .filter_map(|tool| {
            tool.pointer("/function/name")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

pub async fn verify_with_token(url: &str, token: &str) -> Result<(), String> {
    let token = parse_access_token(token)?;
    let client = http_client(GITHUB_CONNECT_TIMEOUT_SECS)?;
    let response = apply_bearer(client.get(url), &token)
        .send()
        .await
        .map_err(|error| format!("CONNECTOR_UNREACHABLE: could not reach Google ({error})"))?;
    let status = response.status().as_u16();
    let payload = response.text().await.unwrap_or_default();
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(map_google_status(status, &payload))
}

pub async fn connect_google_app<F, Fut>(
    needed_scopes: &str,
    credential: Option<&str>,
    verify: F,
) -> Result<(), String>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    if let Some(raw) = credential.map(str::trim).filter(|value| !value.is_empty()) {
        verify(raw.to_string()).await?;
        let stored = if raw.starts_with('{') {
            merge_token_payload(
                serde_json::from_str(raw).unwrap_or_else(|_| json!({ "access_token": raw })),
                needed_scopes,
            )
            .to_string()
        } else {
            json!({
                "access_token": raw,
                "scope": needed_scopes,
            })
            .to_string()
        };
        save_google_credential(&stored)?;
        return Ok(());
    }
    if let Some(stored) = load_google_credential() {
        match verify(stored.clone()).await {
            Ok(()) => return Ok(()),
            Err(error) if error.contains("CONNECTOR_AUTH_FAILED") => {}
            Err(error) => return Err(error),
        }
    }
    let scopes = union_scopes(&stored_scope_string(), needed_scopes);
    let payload = oauth_browser(&scopes).await?;
    let merged = merge_token_payload(payload, &scopes);
    let raw = merged.to_string();
    verify(raw.clone()).await?;
    save_google_credential(&raw)?;
    Ok(())
}
