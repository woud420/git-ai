/// Shared test utilities for metrics_db tests.
use crate::metrics::attrs::attr_pos;
use rusqlite::params;

use super::MetricsDatabase;

pub(super) fn create_test_db() -> (MetricsDatabase, tempfile::TempDir) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test-metrics.db");

    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();

    let mut db = MetricsDatabase { conn };
    db.initialize_schema().unwrap();

    (db, temp_dir)
}

pub(super) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn days_ago(days: u64) -> u32 {
    seconds_ago(days * 24 * 3600)
}

pub(super) fn seconds_ago(seconds: u64) -> u32 {
    unix_now().saturating_sub(seconds).min(u32::MAX as u64) as u32
}

pub(super) fn event_json(ts: u32) -> String {
    format!(r#"{{"t":{ts},"e":1,"v":{{}},"a":{{}}}}"#)
}

pub(super) fn event_json_with_repo(ts: u32, event_id: u16, repo: &str) -> String {
    format!(r#"{{"t":{ts},"e":{event_id},"v":{{}},"a":{{"1":"{repo}"}}}}"#)
}

pub(super) fn pending_event_jsons(db: &MetricsDatabase) -> Vec<String> {
    let mut stmt = db
        .conn
        .prepare("SELECT event_json FROM metrics WHERE delivered_ts IS NULL ORDER BY id DESC")
        .unwrap();
    let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap()
}

pub(super) fn assert_metric_index_exists(db: &MetricsDatabase, index: &str) {
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
            params![index],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "missing index {index}");
}

pub(super) fn assert_metric_index_missing(db: &MetricsDatabase, index: &str) {
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
            params![index],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "unexpected index {index}");
}

pub(super) fn metric_metadata_rows(db: &MetricsDatabase) -> Vec<(Option<i64>, Option<i64>)> {
    let mut stmt = db
        .conn
        .prepare("SELECT event_ts, event_kind FROM metrics ORDER BY id ASC")
        .unwrap();
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap()
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct MetricIdentifierRow {
    pub trace_id: Option<String>,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub tool: Option<String>,
    pub external_session_id: Option<String>,
    pub external_parent_session_id: Option<String>,
    pub external_event_id: Option<String>,
    pub external_parent_event_id: Option<String>,
    pub external_tool_use_id: Option<String>,
}

pub(super) fn metric_identifier_rows(db: &MetricsDatabase) -> Vec<MetricIdentifierRow> {
    let mut stmt = db
        .conn
        .prepare(
            "SELECT trace_id, session_id, parent_session_id, tool, \
                    external_session_id, external_parent_session_id, \
                    external_event_id, external_parent_event_id, external_tool_use_id \
             FROM metrics ORDER BY id ASC",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok(MetricIdentifierRow {
                trace_id: row.get(0)?,
                session_id: row.get(1)?,
                parent_session_id: row.get(2)?,
                tool: row.get(3)?,
                external_session_id: row.get(4)?,
                external_parent_session_id: row.get(5)?,
                external_event_id: row.get(6)?,
                external_parent_event_id: row.get(7)?,
                external_tool_use_id: row.get(8)?,
            })
        })
        .unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap()
}

pub(super) fn event_json_with_all_common_metadata(ts: u32, event_kind: u16) -> String {
    format!(
        r#"{{
            "t":{ts},
            "e":{event_kind},
            "v":{{}},
            "a":{{
                "20":"codex",
                "23":"external-session-1",
                "24":"session-1",
                "25":"trace-1",
                "26":"parent-session-1",
                "27":"external-parent-session-1"
            }}
        }}"#
    )
}

pub(super) fn session_event_json(
    ts: u32,
    session_id: &str,
    external_session_id: &str,
    tool: &str,
    repo_url: Option<&str>,
) -> String {
    let repo_attr = repo_url
        .map(|url| format!(r#","{}":"{}""#, attr_pos::REPO_URL, url))
        .unwrap_or_default();
    format!(
        r#"{{
            "t":{ts},
            "e":5,
            "v":{{"0":{{"type":"assistant"}},"1":"event-{session_id}","3":"tool-use-{session_id}"}},
            "a":{{
                "20":"{tool}",
                "21":"gpt-5",
                "23":"{external_session_id}",
                "24":"{session_id}",
                "25":"trace-{session_id}"
                {repo_attr}
            }}
        }}"#
    )
}
