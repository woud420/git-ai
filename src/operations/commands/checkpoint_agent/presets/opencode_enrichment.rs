use super::{StreamFormat, StreamSource};
use crate::model::authorship_log_serialization::generate_session_id;
use std::path::Path;

pub(super) fn stream_source_and_model(session_id: &str) -> Option<(StreamSource, String)> {
    let db_path = crate::operations::streams::agents::opencode::opencode_database_path()?;
    let parent_id = lookup_parent_session(&db_path, session_id);
    let model = crate::operations::streams::model_extraction::extract_model(
        &db_path,
        crate::operations::streams::sweep::StreamFormat::OpenCodeSqlite,
        Some(session_id),
    )
    .ok()
    .flatten()
    .unwrap_or_else(|| "unknown".to_string());

    Some((
        StreamSource {
            path: db_path,
            format: StreamFormat::OpenCodeSqlite,
            session_id: generate_session_id(session_id, "opencode"),
            external_session_id: session_id.to_string(),
            external_parent_session_id: parent_id,
        },
        model,
    ))
}

fn lookup_parent_session(db_path: &Path, session_id: &str) -> Option<String> {
    let connection =
        crate::operations::streams::agents::opencode::open_sqlite_readonly(db_path).ok()?;
    connection
        .query_row(
            "SELECT parent_id FROM session WHERE id = ?",
            [session_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
}
