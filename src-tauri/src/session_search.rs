//! Rebuildable SQLite FTS5 cache for visible session messages.
//!
//! JSON journals remain the source of truth. This database contains no model
//! memory, thoughts, raw tool payloads, secrets, or attachment bytes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection};
use serde::Serialize;

use crate::store::ChatMessageStored;

const INDEX_VERSION: u32 = 1;
const DEFAULT_LIMIT: usize = 40;
const MAX_LIMIT: usize = 200;

static INDEX_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn index_lock() -> std::sync::MutexGuard<'static, ()> {
    INDEX_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchResultV1 {
    pub version: u8,
    pub session_id: String,
    pub session_title: String,
    pub message_id: String,
    pub role: String,
    pub snippet: String,
    pub rank: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchRebuildResultV1 {
    pub version: u8,
    pub sessions: usize,
    pub messages: usize,
}

fn index_path() -> PathBuf {
    crate::paths::app_data_root().join("session-search.v1.sqlite3")
}

fn open_at(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("search index dir: {e}"))?;
    }
    let connection = Connection::open(path).map_err(|e| format!("search index open: {e}"))?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|e| format!("search index timeout: {e}"))?;
    connection
        .execute_batch(
            r#"
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS search_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS indexed_sessions (
  session_id TEXT PRIMARY KEY,
  journal_mtime_ms INTEGER NOT NULL,
  journal_size INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
  session_id UNINDEXED,
  message_id UNINDEXED,
  role UNINDEXED,
  content,
  tokenize='unicode61'
);
"#,
        )
        .map_err(|e| format!("search index schema: {e}"))?;
    connection
        .execute(
            "INSERT OR REPLACE INTO search_meta(key, value) VALUES ('version', ?1)",
            [INDEX_VERSION.to_string()],
        )
        .map_err(|e| format!("search index version: {e}"))?;
    Ok(connection)
}

fn journal_fingerprint(session_id: &str) -> (i64, i64) {
    let path = crate::paths::session_dir(session_id).join("messages.json");
    let Ok(metadata) = fs::metadata(path) else {
        return (0, 0);
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0);
    (modified, metadata.len().min(i64::MAX as u64) as i64)
}

fn replace_session_in(
    connection: &mut Connection,
    session_id: &str,
    messages: &[ChatMessageStored],
    fingerprint: (i64, i64),
) -> Result<usize, String> {
    let tx = connection
        .transaction()
        .map_err(|e| format!("search transaction: {e}"))?;
    tx.execute(
        "DELETE FROM messages_fts WHERE session_id = ?1",
        [session_id],
    )
    .map_err(|e| format!("search remove session: {e}"))?;
    let mut inserted = 0usize;
    {
        let mut statement = tx
            .prepare(
                "INSERT INTO messages_fts(session_id, message_id, role, content) VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| format!("search prepare insert: {e}"))?;
        for message in messages {
            let content = message.content.trim();
            if content.is_empty() || !matches!(message.role.as_str(), "user" | "assistant" | "tool")
            {
                continue;
            }
            statement
                .execute(params![session_id, message.id, message.role, content])
                .map_err(|e| format!("search insert: {e}"))?;
            inserted += 1;
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO indexed_sessions(session_id, journal_mtime_ms, journal_size) VALUES (?1, ?2, ?3)",
        params![session_id, fingerprint.0, fingerprint.1],
    )
    .map_err(|e| format!("search fingerprint: {e}"))?;
    tx.commit().map_err(|e| format!("search commit: {e}"))?;
    Ok(inserted)
}

pub fn reindex_session(session_id: &str) -> Result<usize, String> {
    let _guard = index_lock();
    let mut connection = open_at(&index_path())?;
    let messages = crate::store::load_messages(session_id);
    replace_session_in(
        &mut connection,
        session_id,
        &messages,
        journal_fingerprint(session_id),
    )
}

pub fn remove_session(session_id: &str) -> Result<(), String> {
    let _guard = index_lock();
    let mut connection = open_at(&index_path())?;
    let tx = connection
        .transaction()
        .map_err(|e| format!("search transaction: {e}"))?;
    tx.execute(
        "DELETE FROM messages_fts WHERE session_id = ?1",
        [session_id],
    )
    .map_err(|e| format!("search remove messages: {e}"))?;
    tx.execute(
        "DELETE FROM indexed_sessions WHERE session_id = ?1",
        [session_id],
    )
    .map_err(|e| format!("search remove fingerprint: {e}"))?;
    tx.commit().map_err(|e| format!("search commit: {e}"))
}

fn sync_stale_sessions(connection: &mut Connection) -> Result<(), String> {
    let sessions = crate::store::load_sessions_index();
    let live_ids: std::collections::HashSet<&str> =
        sessions.iter().map(|session| session.id.as_str()).collect();
    let indexed_ids = {
        let mut statement = connection
            .prepare("SELECT session_id FROM indexed_sessions")
            .map_err(|e| format!("search stale prepare: {e}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("search stale query: {e}"))?;
        rows.filter_map(Result::ok).collect::<Vec<_>>()
    };
    for stale_id in indexed_ids
        .into_iter()
        .filter(|session_id| !live_ids.contains(session_id.as_str()))
    {
        let tx = connection
            .transaction()
            .map_err(|e| format!("search stale transaction: {e}"))?;
        tx.execute(
            "DELETE FROM messages_fts WHERE session_id = ?1",
            [&stale_id],
        )
        .map_err(|e| format!("search stale messages: {e}"))?;
        tx.execute(
            "DELETE FROM indexed_sessions WHERE session_id = ?1",
            [&stale_id],
        )
        .map_err(|e| format!("search stale fingerprint: {e}"))?;
        tx.commit()
            .map_err(|e| format!("search stale commit: {e}"))?;
    }
    for session in sessions {
        let fingerprint = journal_fingerprint(&session.id);
        let indexed = connection
            .query_row(
                "SELECT journal_mtime_ms, journal_size FROM indexed_sessions WHERE session_id = ?1",
                [&session.id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .ok();
        if indexed == Some(fingerprint) {
            continue;
        }
        let messages = crate::store::load_messages(&session.id);
        replace_session_in(connection, &session.id, &messages, fingerprint)?;
    }
    Ok(())
}

fn fts_query(raw: &str) -> Result<String, String> {
    let terms: Vec<String> = raw
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .take(16)
        .map(|term| term.chars().take(200).collect::<String>())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        return Err("search query is empty".into());
    }
    Ok(terms.join(" AND "))
}

pub fn search(query: &str, limit: Option<usize>) -> Result<Vec<SessionSearchResultV1>, String> {
    let _guard = index_lock();
    let mut connection = open_at(&index_path())?;
    sync_stale_sessions(&mut connection)?;
    search_in(&connection, query, limit)
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(|character| {
        matches!(
            character,
            '\u{3400}'..='\u{4dbf}'
                | '\u{4e00}'..='\u{9fff}'
                | '\u{f900}'..='\u{faff}'
        )
    })
}

fn search_in(
    connection: &Connection,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<SessionSearchResultV1>, String> {
    if contains_cjk(query) {
        return search_cjk_in(connection, query, limit);
    }
    let query = fts_query(query)?;
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as i64;
    let titles: std::collections::HashMap<String, String> = crate::store::load_sessions_index()
        .into_iter()
        .map(|session| (session.id, session.title))
        .collect();
    let mut statement = connection
        .prepare(
            "SELECT session_id, message_id, role, snippet(messages_fts, 3, '', '', '…', 28), bm25(messages_fts) FROM messages_fts WHERE messages_fts MATCH ?1 ORDER BY bm25(messages_fts) LIMIT ?2",
        )
        .map_err(|e| format!("search prepare: {e}"))?;
    let rows = statement
        .query_map(params![query, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })
        .map_err(|e| format!("search query: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        let (session_id, message_id, role, snippet, rank) =
            row.map_err(|e| format!("search row: {e}"))?;
        out.push(SessionSearchResultV1 {
            version: 1,
            session_title: titles.get(&session_id).cloned().unwrap_or_default(),
            session_id,
            message_id,
            role,
            snippet,
            rank,
        });
    }
    Ok(out)
}

fn search_cjk_in(
    connection: &Connection,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<SessionSearchResultV1>, String> {
    let terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .take(16)
        .map(|term| term.chars().take(200).collect::<String>())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Err("search query is empty".into());
    }
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as i64;
    let predicates = (1..=terms.len())
        .map(|index| format!("instr(lower(content), lower(?{index})) > 0"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let limit_parameter = terms.len() + 1;
    let sql = format!(
        "SELECT session_id, message_id, role, substr(content, 1, 240) \
         FROM messages_fts WHERE {predicates} ORDER BY rowid DESC LIMIT ?{limit_parameter}"
    );
    let mut parameters = terms.into_iter().map(SqlValue::Text).collect::<Vec<_>>();
    parameters.push(SqlValue::Integer(limit));
    let titles: std::collections::HashMap<String, String> = crate::store::load_sessions_index()
        .into_iter()
        .map(|session| (session.id, session.title))
        .collect();
    let mut statement = connection
        .prepare(&sql)
        .map_err(|e| format!("search CJK prepare: {e}"))?;
    let rows = statement
        .query_map(params_from_iter(parameters.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| format!("search CJK query: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        let (session_id, message_id, role, snippet) =
            row.map_err(|e| format!("search CJK row: {e}"))?;
        out.push(SessionSearchResultV1 {
            version: 1,
            session_title: titles.get(&session_id).cloned().unwrap_or_default(),
            session_id,
            message_id,
            role,
            snippet,
            rank: 0.0,
        });
    }
    Ok(out)
}

pub fn rebuild() -> Result<SessionSearchRebuildResultV1, String> {
    let _guard = index_lock();
    let mut connection = open_at(&index_path())?;
    connection
        .execute_batch("DELETE FROM messages_fts; DELETE FROM indexed_sessions;")
        .map_err(|e| format!("search reset: {e}"))?;
    let sessions = crate::store::load_sessions_index();
    let mut messages = 0usize;
    for session in &sessions {
        let rows = crate::store::load_messages(&session.id);
        messages += replace_session_in(
            &mut connection,
            &session.id,
            &rows,
            journal_fingerprint(&session.id),
        )?;
    }
    Ok(SessionSearchRebuildResultV1 {
        version: 1,
        sessions: sessions.len(),
        messages,
    })
}

pub fn delete_index() -> Result<(), String> {
    let _guard = index_lock();
    let path = index_path();
    for candidate in [
        path.clone(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        match fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "delete search index {}: {error}",
                    candidate.display()
                ))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn message(id: &str, role: &str, content: &str) -> ChatMessageStored {
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

    #[test]
    fn fts_cache_indexes_visible_content() {
        let path =
            std::env::temp_dir().join(format!("sunsetz-fts-{}.sqlite3", uuid::Uuid::new_v4()));
        let mut connection = open_at(&path).unwrap();
        let rows = vec![
            message("u1", "user", "alpha migration"),
            message("a1", "assistant", "beta implementation"),
        ];
        replace_session_in(&mut connection, "s1", &rows, (1, 1)).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH 'alpha'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(connection);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn query_is_quoted_and_bounded() {
        assert_eq!(fts_query("hello world").unwrap(), "\"hello\" AND \"world\"");
        assert!(fts_query("   ").is_err());
    }

    #[test]
    fn cjk_search_matches_visible_substrings() {
        let path =
            std::env::temp_dir().join(format!("sunsetz-fts-cjk-{}.sqlite3", uuid::Uuid::new_v4()));
        let mut connection = open_at(&path).unwrap();
        replace_session_in(
            &mut connection,
            "s1",
            &[message("a1", "assistant", "迁移计划已经完成")],
            (1, 1),
        )
        .unwrap();
        let rows = search_cjk_in(&connection, "迁移计划", Some(10)).unwrap();
        assert_eq!(rows.len(), 1);
        drop(connection);
        let _ = fs::remove_file(path);
    }
}
