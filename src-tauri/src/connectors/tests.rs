use super::*;
use serde_json::json;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    home: PathBuf,
}

impl EnvGuard {
    fn new(label: &str) -> Self {
        let home = std::env::temp_dir().join(format!(
            "sunsetz-connector-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let _ = fs::create_dir_all(&home);
        std::env::set_var("SUNSETZ_HOME", &home);
        std::env::remove_var("SUNSETZ_OPEN_CONNECTOR_URL");
        std::env::remove_var("SUNSETZ_GITHUB_API_URL");
        std::env::remove_var("SUNSETZ_GMAIL_API_URL");
        std::env::remove_var("SUNSETZ_DRIVE_API_URL");
        std::env::remove_var("SUNSETZ_CALENDAR_API_URL");
        std::env::remove_var("SUNSETZ_GOOGLE_OAUTH_CLIENT_ID");
        std::env::remove_var("SUNSETZ_GOOGLE_OAUTH_AUTH_URL");
        std::env::remove_var("SUNSETZ_GOOGLE_OAUTH_TOKEN_URL");
        Self { home }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        std::env::remove_var("SUNSETZ_HOME");
        std::env::remove_var("SUNSETZ_OPEN_CONNECTOR_URL");
        std::env::remove_var("SUNSETZ_GITHUB_API_URL");
        std::env::remove_var("SUNSETZ_GMAIL_API_URL");
        std::env::remove_var("SUNSETZ_DRIVE_API_URL");
        std::env::remove_var("SUNSETZ_CALENDAR_API_URL");
        std::env::remove_var("SUNSETZ_GOOGLE_OAUTH_CLIENT_ID");
        std::env::remove_var("SUNSETZ_GOOGLE_OAUTH_AUTH_URL");
        std::env::remove_var("SUNSETZ_GOOGLE_OAUTH_TOKEN_URL");
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn spawn_http(routes: Vec<(String, u16, String)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(false).ok();
    let addr = listener.local_addr().expect("addr");
    thread::spawn(move || {
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            while raw.windows(4).all(|window| window != b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&buf[..n]),
                }
                if raw.len() > 16_384 {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&raw);
            let first = request.lines().next().unwrap_or_default();
            let path = first.split_whitespace().nth(1).unwrap_or("/");
            let (status, body) = routes
                .iter()
                .find(|(prefix, _, _)| path.starts_with(prefix.as_str()))
                .map(|(_, status, body)| (*status, body.clone()))
                .unwrap_or((404, "{\"ok\":false}".into()));
            let reason = if status < 400 { "OK" } else { "ERR" };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

fn spawn_google_oauth_mock() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(false).ok();
    let addr = listener.local_addr().expect("addr");
    thread::spawn(move || {
        for _ in 0..16 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut raw = Vec::new();
            let mut buf = [0u8; 2048];
            while raw.windows(4).all(|window| window != b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&buf[..n]),
                }
                if raw.len() > 16_384 {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&raw);
            let first = request.lines().next().unwrap_or_default();
            let path = first.split_whitespace().nth(1).unwrap_or("/");
            if path.starts_with("/oauth") {
                let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
                let mut redirect = String::new();
                let mut state = String::new();
                for pair in query.split('&') {
                    let Some((name, value)) = pair.split_once('=') else {
                        continue;
                    };
                    let decoded = value
                        .replace("%3A", ":")
                        .replace("%3a", ":")
                        .replace("%2F", "/")
                        .replace("%2f", "/");
                    if name == "redirect_uri" {
                        redirect = decoded;
                    } else if name == "state" {
                        state = decoded;
                    }
                }
                let location = format!("{redirect}?code=oauth-code&state={state}");
                let _ = write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            } else if path.starts_with("/token") {
                let body = "{\"access_token\":\"ya29.oauth\",\"refresh_token\":\"1//refresh\",\"expires_in\":3600,\"token_type\":\"Bearer\",\"scope\":\"https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/drive.readonly\"}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else if path.contains("/gmail/v1/users/me/profile") {
                let body = "{\"emailAddress\":\"ada@example.com\"}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else if path.contains("/drive/v3/about") {
                let body = "{\"user\":{\"emailAddress\":\"ada@example.com\"}}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else if path.contains("/calendar/v3/users/me/calendarList") {
                let body = "{\"items\":[{\"id\":\"primary\"}]}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else {
                let _ = write!(
                    stream,
                    "HTTP/1.1 404 ERR\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

#[test]
fn catalog_ids_are_stable_and_unique() {
    let mut ids: Vec<_> = CATALOG.iter().map(|row| row.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), CATALOG.len());
    assert!(CATALOG.iter().any(|row| row.id == "gmail"));
    assert!(CATALOG.iter().any(|row| row.id == "github"));
    assert!(CATALOG.iter().any(|row| row.id == "google-drive"));
    assert!(CATALOG.iter().any(|row| row.id == "google-calendar"));
}

#[tokio::test]
async fn unknown_connector_is_fail_closed() {
    let _lock = ENV_LOCK.lock().unwrap();
    let error = connect_connector("not-a-plugin", None).await.unwrap_err();
    assert!(error.starts_with("CONNECTOR_UNKNOWN:"));
}

#[tokio::test]
async fn connect_without_runtime_does_not_mark_installed() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("missing");
    let error = connect_connector("slack", None).await.unwrap_err();
    assert!(error.contains("CONNECTOR_RUNTIME_MISSING"));
    let listed = list_connectors().unwrap();
    let slack = listed.iter().find(|row| row.id == "slack").unwrap();
    assert!(!slack.connected);
    assert!(!slack.enabled);
}

#[test]
fn blank_runtime_url_is_treated_as_missing() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("SUNSETZ_OPEN_CONNECTOR_URL", "   ");
    assert!(open_connector_endpoint().is_none());
    std::env::remove_var("SUNSETZ_OPEN_CONNECTOR_URL");
}

#[tokio::test]
async fn disconnect_unknown_is_fail_closed() {
    let error = disconnect_connector("not-a-plugin").await.unwrap_err();
    assert!(error.starts_with("CONNECTOR_UNKNOWN:"));
}

#[tokio::test]
async fn fake_runtime_url_does_not_mark_connected() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("fake-url");
    std::env::set_var("SUNSETZ_OPEN_CONNECTOR_URL", "http://127.0.0.1:9");
    let error = connect_connector("slack", None).await.unwrap_err();
    assert!(error.contains("CONNECTOR_PROBE_FAILED"));
    let listed = list_connectors().unwrap();
    let slack = listed.iter().find(|row| row.id == "slack").unwrap();
    assert!(!slack.connected);
}

#[test]
fn non_loopback_url_is_rejected() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("SUNSETZ_OPEN_CONNECTOR_URL", "https://example.com");
    let error = configured_open_connector_url().unwrap_err();
    assert!(error.contains("CONNECTOR_RUNTIME_REJECTED"));
    std::env::remove_var("SUNSETZ_OPEN_CONNECTOR_URL");
}

#[tokio::test]
async fn open_connector_connects_after_health_and_tools() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("oc-ok");
    let url = spawn_http(vec![
        ("/health".into(), 200, "{\"ok\":true}".into()),
        (
            "/v1/connectors/slack/tools".into(),
            200,
            "{\"tools\":[{\"function\":{\"name\":\"slack_search\"}}]}".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_OPEN_CONNECTOR_URL", &url);
    let connected = connect_connector("slack", None).await.expect("connect");
    assert!(connected.connected);
    assert_eq!(connected.tools, vec!["slack_search"]);
    let disconnected = disconnect_connector("slack").await.expect("disconnect");
    assert!(!disconnected.connected);
}

#[tokio::test]
async fn remote_tools_cannot_override_host_tools() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("override");
    let url = spawn_http(vec![
        ("/health".into(), 200, "{\"ok\":true}".into()),
        (
            "/v1/connectors/slack/tools".into(),
            200,
            "{\"tools\":[{\"function\":{\"name\":\"write_file\"}}]}".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_OPEN_CONNECTOR_URL", &url);
    let error = connect_connector("slack", None).await.unwrap_err();
    assert!(error.contains("CONNECTOR_PROBE_FAILED"));
    assert!(!list_connectors()
        .unwrap()
        .iter()
        .any(|row| row.id == "slack" && row.connected));
}

#[tokio::test]
async fn gmail_connect_without_client_or_token_is_fail_closed() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gmail-missing");
    let error = connect_connector("gmail", None).await.unwrap_err();
    assert!(error.contains("CONNECTOR_OAUTH_CLIENT_MISSING"));
    assert!(!list_connectors()
        .unwrap()
        .iter()
        .any(|row| row.id == "gmail" && row.connected));
}

#[tokio::test]
async fn gmail_browser_oauth_connects_against_mock() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gmail-oauth");
    let base = spawn_google_oauth_mock();
    std::env::set_var("SUNSETZ_GOOGLE_OAUTH_CLIENT_ID", "test-client");
    std::env::set_var("SUNSETZ_GOOGLE_OAUTH_AUTH_URL", format!("{base}/oauth"));
    std::env::set_var("SUNSETZ_GOOGLE_OAUTH_TOKEN_URL", format!("{base}/token"));
    std::env::set_var("SUNSETZ_GMAIL_API_URL", &base);
    let connected = connect_connector("gmail", None)
        .await
        .expect("browser oauth");
    assert!(connected.connected);
    assert!(connected
        .tools
        .iter()
        .any(|name| name == "gmail_list_messages"));
    let stored = load_credential("google").expect("stored google token");
    assert!(stored.contains("refresh_token"), "{stored}");
}

#[tokio::test]
async fn gmail_connects_with_access_token_against_mock_api() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gmail-ok");
    let url = spawn_http(vec![(
        "/gmail/v1/users/me/profile".into(),
        200,
        "{\"emailAddress\":\"ada@example.com\"}".into(),
    )]);
    std::env::set_var("SUNSETZ_GMAIL_API_URL", &url);
    let connected = connect_connector("gmail", Some("ya29.test"))
        .await
        .expect("connect");
    assert!(connected.connected);
    assert!(connected
        .tools
        .iter()
        .any(|name| name == "gmail_list_messages"));
    let disconnected = disconnect_connector("gmail").await.expect("disconnect");
    assert!(!disconnected.connected);
    assert!(load_credential("google").is_none());
}

#[tokio::test]
async fn gmail_rejects_unauthorized_token() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gmail-401");
    let url = spawn_http(vec![(
        "/gmail/v1/users/me/profile".into(),
        401,
        "{\"error\":\"invalid_token\"}".into(),
    )]);
    std::env::set_var("SUNSETZ_GMAIL_API_URL", &url);
    let error = connect_connector("gmail", Some("ya29.bad"))
        .await
        .unwrap_err();
    assert!(error.contains("CONNECTOR_AUTH_FAILED"));
    assert!(!list_connectors()
        .unwrap()
        .iter()
        .any(|row| row.id == "gmail" && row.connected));
}

#[tokio::test]
async fn drive_reuses_google_token_without_browser() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("drive-reuse");
    let url = spawn_http(vec![
        (
            "/gmail/v1/users/me/profile".into(),
            200,
            "{\"emailAddress\":\"ada@example.com\"}".into(),
        ),
        (
            "/drive/v3/about".into(),
            200,
            "{\"user\":{\"emailAddress\":\"ada@example.com\"}}".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_GMAIL_API_URL", &url);
    std::env::set_var("SUNSETZ_DRIVE_API_URL", &url);
    connect_connector("gmail", Some("ya29.shared"))
        .await
        .expect("gmail");
    let connected = connect_connector("google-drive", None)
        .await
        .expect("drive reuse");
    assert!(connected.connected);
    assert!(connected
        .tools
        .iter()
        .any(|name| name == "google-drive_list_files"));
    assert!(load_credential("google").is_some());
}

fn spawn_drive_incremental_mock() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(false).ok();
    let addr = listener.local_addr().expect("addr");
    thread::spawn(move || {
        let mut about_hits = 0u8;
        for _ in 0..16 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut raw = Vec::new();
            let mut buf = [0u8; 2048];
            while raw.windows(4).all(|window| window != b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&buf[..n]),
                }
                if raw.len() > 16_384 {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&raw);
            let first = request.lines().next().unwrap_or_default();
            let path = first.split_whitespace().nth(1).unwrap_or("/");
            if path.starts_with("/oauth") {
                let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
                let mut redirect = String::new();
                let mut state = String::new();
                for pair in query.split('&') {
                    let Some((name, value)) = pair.split_once('=') else {
                        continue;
                    };
                    let decoded = value
                        .replace("%3A", ":")
                        .replace("%3a", ":")
                        .replace("%2F", "/")
                        .replace("%2f", "/");
                    if name == "redirect_uri" {
                        redirect = decoded;
                    } else if name == "state" {
                        state = decoded;
                    }
                }
                let location = format!("{redirect}?code=oauth-code&state={state}");
                let _ = write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            } else if path.starts_with("/token") {
                let body = "{\"access_token\":\"ya29.drive-scope\",\"refresh_token\":\"1//refresh\",\"expires_in\":3600,\"token_type\":\"Bearer\"}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else if path.contains("/drive/v3/about") {
                about_hits = about_hits.saturating_add(1);
                let (status, body) = if about_hits == 1 {
                    (403, "{\"error\":{\"message\":\"insufficientPermissions\"}}")
                } else {
                    (200, "{\"user\":{\"emailAddress\":\"ada@example.com\"}}")
                };
                let reason = if status < 400 { "OK" } else { "ERR" };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else if path.contains("/gmail/v1/users/me/profile") {
                let body = "{\"emailAddress\":\"ada@example.com\"}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else {
                let _ = write!(
                    stream,
                    "HTTP/1.1 404 ERR\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

#[tokio::test]
async fn drive_incremental_oauth_when_existing_token_lacks_scope() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("drive-incremental");
    let base = spawn_drive_incremental_mock();
    std::env::set_var("SUNSETZ_GMAIL_API_URL", &base);
    std::env::set_var("SUNSETZ_DRIVE_API_URL", &base);
    std::env::set_var("SUNSETZ_GOOGLE_OAUTH_CLIENT_ID", "test-client");
    std::env::set_var("SUNSETZ_GOOGLE_OAUTH_AUTH_URL", format!("{base}/oauth"));
    std::env::set_var("SUNSETZ_GOOGLE_OAUTH_TOKEN_URL", format!("{base}/token"));
    connect_connector("gmail", Some("ya29.gmail-only"))
        .await
        .expect("gmail");
    let connected = connect_connector("google-drive", None)
        .await
        .expect("incremental");
    assert!(connected.connected);
}

#[tokio::test]
async fn disconnect_gmail_keeps_google_token_if_drive_connected() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("google-shared-disconnect");
    let url = spawn_http(vec![
        (
            "/gmail/v1/users/me/profile".into(),
            200,
            "{\"emailAddress\":\"ada@example.com\"}".into(),
        ),
        (
            "/drive/v3/about".into(),
            200,
            "{\"user\":{\"emailAddress\":\"ada@example.com\"}}".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_GMAIL_API_URL", &url);
    std::env::set_var("SUNSETZ_DRIVE_API_URL", &url);
    connect_connector("gmail", Some("ya29.shared"))
        .await
        .expect("gmail");
    connect_connector("google-drive", None)
        .await
        .expect("drive");
    disconnect_connector("gmail")
        .await
        .expect("disconnect gmail");
    assert!(load_credential("google").is_some());
    assert!(list_connectors()
        .unwrap()
        .iter()
        .any(|row| row.id == "google-drive" && row.connected));
    disconnect_connector("google-drive")
        .await
        .expect("disconnect drive");
    assert!(load_credential("google").is_none());
}

#[tokio::test]
async fn drive_invoke_lists_files() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("drive-invoke");
    let url = spawn_http(vec![
        (
            "/drive/v3/about".into(),
            200,
            "{\"user\":{\"emailAddress\":\"ada@example.com\"}}".into(),
        ),
        (
            "/drive/v3/files".into(),
            200,
            "{\"files\":[{\"id\":\"1\",\"name\":\"Briefing\",\"mimeType\":\"application/vnd.google-apps.document\",\"modifiedTime\":\"2026-08-01T00:00:00Z\",\"webViewLink\":\"https://docs.google.com/document/d/1\"}]}".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_DRIVE_API_URL", &url);
    connect_connector("google-drive", Some("ya29.drive"))
        .await
        .expect("connect");
    let output = invoke_tool(
        "google-drive_list_files",
        &json!({ "q": "name contains 'Brief'" }),
    )
    .await;
    assert!(output.contains("Briefing"), "{output}");
}

#[tokio::test]
async fn calendar_connects_and_lists_events() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("cal-invoke");
    let url = spawn_http(vec![
        (
            "/calendar/v3/users/me/calendarList".into(),
            200,
            "{\"items\":[{\"id\":\"primary\"}]}".into(),
        ),
        (
            "/calendar/v3/calendars/primary/events".into(),
            200,
            "{\"items\":[{\"id\":\"evt1\",\"summary\":\"Design review\",\"start\":{\"dateTime\":\"2026-08-29T10:00:00Z\"},\"end\":{\"dateTime\":\"2026-08-29T10:45:00Z\"}}]}".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_CALENDAR_API_URL", &url);
    let connected = connect_connector("google-calendar", Some("ya29.cal"))
        .await
        .expect("connect");
    assert!(connected
        .tools
        .iter()
        .any(|name| name == "google-calendar_list_events"));
    let output = invoke_tool("google-calendar_list_events", &json!({})).await;
    assert!(output.contains("Design review"), "{output}");
}

#[tokio::test]
async fn github_connect_without_token_is_fail_closed() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gh-missing");
    let error = connect_connector("github", None).await.unwrap_err();
    assert!(error.contains("CONNECTOR_CREDENTIAL_MISSING"));
    assert!(!list_connectors()
        .unwrap()
        .iter()
        .any(|row| row.id == "github" && row.connected));
}

#[test]
fn github_token_strips_scheme_and_quotes() {
    assert_eq!(github::normalize_token("  Bearer ghp_abc  "), "ghp_abc");
    assert_eq!(github::normalize_token("\"github_pat_1\""), "github_pat_1");
    assert_eq!(github::normalize_token("token gho_x"), "gho_x");
}

#[tokio::test]
async fn github_fine_grained_token_without_user_scope_still_connects() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gh-pat");
    let url = spawn_http(vec![
        (
            "/user".into(),
            403,
            "{\"message\":\"Resource not accessible by personal access token\"}".into(),
        ),
        (
            "/rate_limit".into(),
            200,
            "{\"rate\":{\"limit\":5000}}".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_GITHUB_API_URL", &url);
    let connected = connect_connector("github", Some("github_pat_test"))
        .await
        .expect("connect");
    assert!(connected.connected);
    assert!(connected
        .tools
        .contains(&"github_list_pull_requests".into()));
}

#[tokio::test]
async fn github_401_is_auth_failed_not_unreachable() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gh-401");
    let url = spawn_http(vec![(
        "/user".into(),
        401,
        "{\"message\":\"Bad credentials\"}".into(),
    )]);
    std::env::set_var("SUNSETZ_GITHUB_API_URL", &url);
    let error = connect_connector("github", Some("ghp_bad"))
        .await
        .unwrap_err();
    assert!(error.contains("CONNECTOR_AUTH_FAILED"), "{error}");
    assert!(error.contains("Bad credentials"), "{error}");
    assert!(!list_connectors()
        .unwrap()
        .iter()
        .any(|row| row.id == "github" && row.connected));
}

#[tokio::test]
async fn github_connect_roundtrip_with_mock_api() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gh-ok");
    let url = spawn_http(vec![("/user".into(), 200, "{\"login\":\"octo\"}".into())]);
    std::env::set_var("SUNSETZ_GITHUB_API_URL", &url);
    let connected = connect_connector("github", Some("ghp_test"))
        .await
        .expect("connect");
    assert!(connected.connected);
    assert!(connected
        .tools
        .contains(&"github_list_pull_requests".into()));
    assert_eq!(
        verify_connector_selections(&["github".into()], Some("[[connector:github]] thanks"))
            .unwrap(),
        vec!["github".to_string()]
    );
    let disconnected = disconnect_connector("github").await.expect("disconnect");
    assert!(!disconnected.connected);
    let error =
        verify_connector_selections(&["github".into()], Some("[[connector:github]]")).unwrap_err();
    assert!(error.contains("CONNECTOR_NOT_CONNECTED"));
}

#[tokio::test]
async fn github_invoke_lists_pull_requests() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _guard = EnvGuard::new("gh-invoke");
    let url = spawn_http(vec![
        ("/user".into(), 200, "{\"login\":\"octo\"}".into()),
        (
            "/repos/acme/app/pulls".into(),
            200,
            "[{\"number\":1,\"title\":\"Fix\",\"state\":\"open\",\"html_url\":\"https://github.com/acme/app/pull/1\",\"user\":{\"login\":\"dev\"}}]".into(),
        ),
    ]);
    std::env::set_var("SUNSETZ_GITHUB_API_URL", &url);
    connect_connector("github", Some("ghp_test"))
        .await
        .expect("connect");
    let output = invoke_tool(
        "github_list_pull_requests",
        &json!({ "owner": "acme", "repo": "app" }),
    )
    .await;
    assert!(output.contains("Fix"), "{output}");
    assert!(output.contains("\"number\":1"), "{output}");
}

#[test]
fn unverified_gmail_marker_is_rejected() {
    let error = verify_connector_selections(&[], Some("[[connector:gmail]]")).unwrap_err();
    assert!(error.contains("CONNECTOR_USE_UNVERIFIED"));
}

#[test]
fn drive_and_calendar_create_tools_are_connector_writes() {
    assert!(crate::permission::is_connector_tool(
        "google-drive_create_file"
    ));
    assert!(crate::permission::is_connector_write_tool(
        "google-drive_create_file"
    ));
    assert!(crate::permission::is_connector_write_tool(
        "google-calendar_create_event"
    ));
    assert!(!crate::permission::is_connector_write_tool(
        "google-drive_list_files"
    ));
    assert!(!crate::permission::is_edit_tool("google-drive_create_file"));
}
