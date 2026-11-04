//! Request history (FR-HIS): every sent request with its outcome, newest first, in SQLite (D11).
//!
//! The request is stored *as typed* (with `{{variables}}` unresolved), so secret values from
//! environments never end up in the history file. Entries are capped by count, and stored
//! response bodies are capped by size.
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use at_core::Request;
use rusqlite::{params, Connection, OptionalExtension};

/// Schema version written to `PRAGMA user_version`. Newer files are refused, never altered.
pub const SCHEMA_VERSION: i64 = 1;
/// Default number of entries kept; older ones are deleted when new ones arrive.
pub const DEFAULT_MAX_ENTRIES: usize = 1000;
/// Default number of response-body bytes kept per entry.
pub const DEFAULT_MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("history database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("history entry is unreadable: {0}")]
    Json(#[from] serde_json::Error),
    #[error("history file was written by a newer version of the app (schema {0}); it was left untouched")]
    TooNew(i64),
}

/// What happened to one sent request; this is what gets recorded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    /// The request as typed (unresolved variables).
    pub request: Request,
    /// `None` when no response arrived (network error, timeout, cancelled).
    pub status: Option<u16>,
    pub status_text: String,
    pub elapsed_ms: u64,
    /// Size of the full response body in bytes (even if only part of it is kept).
    pub size: u64,
    pub error: Option<String>,
    pub resp_headers: Vec<(String, String)>,
    /// The response body if it was in memory; `None` if it was spilled to disk or absent.
    pub resp_body: Option<Vec<u8>>,
}

/// One row of the history list (no bodies, cheap to load).
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub id: i64,
    pub ts_ms: i64,
    pub method: String,
    pub url: String,
    pub name: String,
    pub status: Option<u16>,
    pub elapsed_ms: u64,
    pub size: u64,
    pub error: Option<String>,
}

/// A full entry, for opening it again.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub summary: Summary,
    pub request: Request,
    pub status_text: String,
    pub resp_headers: Vec<(String, String)>,
    pub resp_body: Option<Vec<u8>>,
    /// The stored body is only the first part of the response.
    pub body_truncated: bool,
}

pub struct History {
    conn: Connection,
    max_entries: usize,
    max_body: usize,
}

impl History {
    /// Open (or create) the history file.
    pub fn open(path: &Path) -> Result<History, HistoryError> {
        let conn = Connection::open(path)?;
        // WAL: crash-safe and readers don't block the writer.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    /// A throw-away history (tests).
    pub fn open_in_memory() -> Result<History, HistoryError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<History, HistoryError> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(HistoryError::TooNew(version));
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                ts_ms         INTEGER NOT NULL,
                method        TEXT NOT NULL,
                url           TEXT NOT NULL,
                name          TEXT NOT NULL,
                request_json  TEXT NOT NULL,
                status        INTEGER,
                status_text   TEXT NOT NULL,
                elapsed_ms    INTEGER NOT NULL,
                size          INTEGER NOT NULL,
                error         TEXT,
                resp_headers  TEXT NOT NULL,
                resp_body     BLOB,
                body_truncated INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        if version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(History {
            conn,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_body: DEFAULT_MAX_BODY,
        })
    }

    /// Change the caps (mainly for tests). `max_entries` is at least 1.
    pub fn with_limits(mut self, max_entries: usize, max_body: usize) -> History {
        self.max_entries = max_entries.max(1);
        self.max_body = max_body;
        self
    }

    /// Record a sent request; deletes the oldest entries beyond the cap. Returns the new id.
    pub fn add(&mut self, rec: &Record, ts_ms: i64) -> Result<i64, HistoryError> {
        let request_json = serde_json::to_string(&rec.request)?;
        let headers_json = serde_json::to_string(&rec.resp_headers)?;
        let (body, truncated) = match &rec.resp_body {
            Some(b) if b.len() > self.max_body => (Some(&b[..self.max_body]), true),
            Some(b) => (Some(&b[..]), false),
            None => (None, false),
        };
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO entries (ts_ms, method, url, name, request_json, status, status_text,
                                  elapsed_ms, size, error, resp_headers, resp_body, body_truncated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                ts_ms,
                rec.request.method.as_str(),
                rec.request.url,
                rec.request.name,
                request_json,
                rec.status,
                rec.status_text,
                rec.elapsed_ms as i64,
                rec.size as i64,
                rec.error,
                headers_json,
                body,
                truncated,
            ],
        )?;
        let id = tx.last_insert_rowid();
        // Keep the newest `max_entries` rows.
        tx.execute(
            "DELETE FROM entries WHERE id <=
               (SELECT id FROM entries ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            params![self.max_entries as i64],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Newest first. `filter` matches the URL or name (case-insensitive for ASCII); empty = all.
    pub fn list(&self, filter: &str, limit: usize) -> Result<Vec<Summary>, HistoryError> {
        let pattern = format!("%{}%", escape_like(filter.trim()));
        let mut stmt = self.conn.prepare(
            "SELECT id, ts_ms, method, url, name, status, elapsed_ms, size, error
             FROM entries
             WHERE url LIKE ?1 ESCAPE '\\' OR name LIKE ?1 ESCAPE '\\'
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], summary_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Entry>, HistoryError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, ts_ms, method, url, name, status, elapsed_ms, size, error,
                        request_json, status_text, resp_headers, resp_body, body_truncated
                 FROM entries WHERE id = ?1",
                params![id],
                |r| {
                    Ok((
                        summary_from_row(r)?,
                        r.get::<_, String>(9)?,
                        r.get::<_, String>(10)?,
                        r.get::<_, String>(11)?,
                        r.get::<_, Option<Vec<u8>>>(12)?,
                        r.get::<_, bool>(13)?,
                    ))
                },
            )
            .optional()?;
        let Some((summary, request_json, status_text, headers_json, resp_body, body_truncated)) =
            row
        else {
            return Ok(None);
        };
        Ok(Some(Entry {
            summary,
            request: serde_json::from_str(&request_json)?,
            status_text,
            resp_headers: serde_json::from_str(&headers_json)?,
            resp_body,
            body_truncated,
        }))
    }

    pub fn clear(&mut self) -> Result<(), HistoryError> {
        self.conn.execute("DELETE FROM entries", [])?;
        Ok(())
    }

    pub fn len(&self) -> Result<usize, HistoryError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn is_empty(&self) -> Result<bool, HistoryError> {
        Ok(self.len()? == 0)
    }
}

fn summary_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Summary> {
    Ok(Summary {
        id: r.get(0)?,
        ts_ms: r.get(1)?,
        method: r.get(2)?,
        url: r.get(3)?,
        name: r.get(4)?,
        status: r.get(5)?,
        elapsed_ms: r.get::<_, i64>(6)? as u64,
        size: r.get::<_, i64>(7)? as u64,
        error: r.get(8)?,
    })
}

/// Escape `%`, `_` and `\` so user text is matched literally by `LIKE ... ESCAPE '\'`.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Short relative time for the history list: "just now", "5 min ago", "3 h ago", "2 d ago".
pub fn ago(now_ms: i64, ts_ms: i64) -> String {
    let s = (now_ms - ts_ms).max(0) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use at_core::{KeyValue, Method};

    fn rec(url: &str, status: Option<u16>) -> Record {
        Record {
            request: Request {
                name: format!("req {url}"),
                method: Method::Post,
                url: url.into(),
                headers: vec![KeyValue::new("Authorization", "Bearer {{token}}")],
                ..Default::default()
            },
            status,
            status_text: if status.is_some() {
                "OK".into()
            } else {
                String::new()
            },
            elapsed_ms: 12,
            size: 5,
            error: status.is_none().then(|| "connection refused".to_string()),
            resp_headers: vec![("content-type".into(), "application/json".into())],
            resp_body: status.map(|_| b"{\"a\":1}".to_vec()),
        }
    }

    #[test]
    fn add_list_get_roundtrip_newest_first() {
        let mut h = History::open_in_memory().unwrap();
        let a = h.add(&rec("https://a.test", Some(200)), 1_000).unwrap();
        let b = h.add(&rec("https://b.test", None), 2_000).unwrap();
        let list = h.list("", 10).unwrap();
        assert_eq!(list.iter().map(|s| s.id).collect::<Vec<_>>(), vec![b, a]);
        assert_eq!(list[0].method, "POST");
        assert_eq!(list[0].status, None);
        assert_eq!(list[0].error.as_deref(), Some("connection refused"));

        let e = h.get(a).unwrap().unwrap();
        assert_eq!(e.request, rec("https://a.test", Some(200)).request);
        assert_eq!(e.summary.status, Some(200));
        assert_eq!(e.summary.ts_ms, 1_000);
        assert_eq!(e.resp_body.as_deref(), Some(&b"{\"a\":1}"[..]));
        assert_eq!(e.resp_headers[0].0, "content-type");
        assert!(!e.body_truncated);
        assert!(h.get(9999).unwrap().is_none());
    }

    #[test]
    fn variables_are_stored_unresolved() {
        let mut h = History::open_in_memory().unwrap();
        let id = h.add(&rec("https://{{host}}/x", Some(200)), 1).unwrap();
        let e = h.get(id).unwrap().unwrap();
        assert_eq!(e.summary.url, "https://{{host}}/x");
        assert_eq!(e.request.headers[0].value, "Bearer {{token}}");
    }

    #[test]
    fn oldest_entries_are_pruned_beyond_the_cap() {
        let mut h = History::open_in_memory().unwrap().with_limits(3, 1024);
        for i in 0..5 {
            h.add(&rec(&format!("https://{i}.test"), Some(200)), i)
                .unwrap();
        }
        assert_eq!(h.len().unwrap(), 3);
        let urls: Vec<_> = h.list("", 10).unwrap().into_iter().map(|s| s.url).collect();
        assert_eq!(
            urls,
            vec!["https://4.test", "https://3.test", "https://2.test"]
        );
    }

    #[test]
    fn large_bodies_are_truncated_and_flagged() {
        let mut h = History::open_in_memory().unwrap().with_limits(10, 4);
        let mut r = rec("https://big.test", Some(200));
        r.resp_body = Some(b"0123456789".to_vec());
        r.size = 10;
        let id = h.add(&r, 1).unwrap();
        let e = h.get(id).unwrap().unwrap();
        assert_eq!(e.resp_body.as_deref(), Some(&b"0123"[..]));
        assert!(e.body_truncated);
        assert_eq!(e.summary.size, 10, "size is the real response size");
    }

    #[test]
    fn spilled_or_missing_body_is_none() {
        let mut h = History::open_in_memory().unwrap();
        let mut r = rec("https://spill.test", Some(200));
        r.resp_body = None;
        let id = h.add(&r, 1).unwrap();
        let e = h.get(id).unwrap().unwrap();
        assert!(e.resp_body.is_none());
        assert!(!e.body_truncated);
    }

    #[test]
    fn filter_matches_url_or_name_literally() {
        let mut h = History::open_in_memory().unwrap();
        h.add(&rec("https://api.test/users", Some(200)), 1).unwrap();
        h.add(&rec("https://api.test/orders", Some(200)), 2)
            .unwrap();
        h.add(&rec("https://x.test/100%_done", Some(200)), 3)
            .unwrap();
        assert_eq!(h.list("users", 10).unwrap().len(), 1);
        assert_eq!(h.list("USERS", 10).unwrap().len(), 1, "case-insensitive");
        assert_eq!(h.list("api.test", 10).unwrap().len(), 2);
        assert_eq!(h.list("  orders ", 10).unwrap().len(), 1, "trimmed");
        // % and _ are literal, not wildcards
        assert_eq!(h.list("%_", 10).unwrap().len(), 1);
        assert_eq!(h.list("_", 10).unwrap().len(), 1);
        assert_eq!(h.list("nothing", 10).unwrap().len(), 0);
        // name matches too ("req <url>")
        assert_eq!(h.list("req https://api", 10).unwrap().len(), 2);
        // limit
        assert_eq!(h.list("", 2).unwrap().len(), 2);
    }

    #[test]
    fn clear_empties() {
        let mut h = History::open_in_memory().unwrap();
        h.add(&rec("https://a.test", Some(200)), 1).unwrap();
        assert!(!h.is_empty().unwrap());
        h.clear().unwrap();
        assert!(h.is_empty().unwrap());
        // ids keep increasing after a clear (AUTOINCREMENT), so stale ids never point elsewhere
        let id = h.add(&rec("https://b.test", Some(200)), 2).unwrap();
        assert!(id > 1);
    }

    #[test]
    fn survives_reopen_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        {
            let mut h = History::open(&path).unwrap();
            h.add(&rec("https://keep.test", Some(201)), 7).unwrap();
        }
        let h = History::open(&path).unwrap();
        let list = h.list("", 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, Some(201));
    }

    #[test]
    fn newer_schema_is_refused_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        assert!(matches!(
            History::open(&path),
            Err(HistoryError::TooNew(99))
        ));
        let c = Connection::open(&path).unwrap();
        let v: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 99);
    }

    #[test]
    fn garbage_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        std::fs::write(
            &path,
            b"this is not a database, just text that is long enough",
        )
        .unwrap();
        assert!(History::open(&path).is_err());
    }

    #[test]
    fn relative_times() {
        let now = 10_000_000_000;
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now, now - 59_000), "just now");
        assert_eq!(ago(now, now - 60_000), "1 min ago");
        assert_eq!(ago(now, now - 3_599_000), "59 min ago");
        assert_eq!(ago(now, now - 3_600_000), "1 h ago");
        assert_eq!(ago(now, now - 86_400_000 * 3), "3 d ago");
        // clock went backwards: never negative
        assert_eq!(ago(now, now + 5_000), "just now");
    }
}
