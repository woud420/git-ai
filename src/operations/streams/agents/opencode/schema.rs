use crate::model::stream_types::StreamError;
use rusqlite::{Connection, OptionalExtension};

pub(crate) fn is_v2_session(conn: &Connection, id: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_v2 WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )
    .unwrap_or(false)
}
pub(super) fn has_table(conn: &Connection, name: &str) -> Result<bool, StreamError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |row| row.get(0),
    )
    .map_err(|e| StreamError::Fatal {
        message: format!("Failed to detect OpenCode schema: {e}"),
    })
}
pub(crate) fn parent(conn: &Connection, id: &str) -> Option<Option<String>> {
    let table = if is_v2_session(conn, id) {
        "session_v2"
    } else {
        "session"
    };
    conn.query_row(
        &format!("SELECT parent_id FROM {table} WHERE id = ?1"),
        [id],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}
