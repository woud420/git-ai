use super::{JournalError, indexes, metadata, sql_error};
use rusqlite::Connection;

const SCHEMA: &str = "
CREATE TABLE jj_native_registrations (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    source_root_key TEXT NOT NULL UNIQUE,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);
CREATE TABLE jj_native_workspaces (
    source_id TEXT NOT NULL REFERENCES jj_native_registrations(source_id),
    workspace_name TEXT NOT NULL,
    locator_key TEXT NOT NULL,
    workspace_root_key TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, workspace_name),
    UNIQUE (locator_key),
    UNIQUE (source_id, workspace_root_key)
);";

pub(super) fn create(conn: &Connection) -> Result<(), JournalError> {
    conn.execute_batch(SCHEMA)
        .map_err(|error| sql_error("create native registration schema", error))
}

pub(super) fn verify(conn: &Connection) -> Result<(), JournalError> {
    metadata::verify_columns(
        conn,
        "jj_native_registrations",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 0),
            ("source_root_key", "TEXT", 0),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    metadata::verify_columns(
        conn,
        "jj_native_workspaces",
        &[
            ("source_id", "TEXT", 1),
            ("workspace_name", "TEXT", 2),
            ("locator_key", "TEXT", 0),
            ("workspace_root_key", "TEXT", 0),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    metadata::verify_foreign_key(
        conn,
        "jj_native_registrations",
        "jj_native_baselines",
        &[("source_id", "source_id"), ("baseline_id", "baseline_id")],
    )?;
    metadata::verify_foreign_key(
        conn,
        "jj_native_workspaces",
        "jj_native_registrations",
        &[("source_id", "source_id")],
    )?;
    indexes::verify(
        conn,
        "jj_native_registrations",
        &[&["source_id"], &["source_root_key"]],
    )?;
    indexes::verify(
        conn,
        "jj_native_workspaces",
        &[
            &["source_id", "workspace_name"],
            &["locator_key"],
            &["source_id", "workspace_root_key"],
        ],
    )?;
    // Matching index pragmas do not guarantee that SQLite can bind a child's
    // foreign key to the parent's declared column collations. Prepare only:
    // no registration row or trigger program is executed during schema checks.
    conn.prepare(
        "INSERT INTO jj_native_registrations
         (source_id, baseline_id, source_root_key, record, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .map_err(|error| sql_error("verify registration foreign key binding", error))?;
    conn.prepare(
        "INSERT INTO jj_native_workspaces
         (source_id, workspace_name, locator_key, workspace_root_key, record, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .map_err(|error| sql_error("verify workspace foreign key binding", error))?;
    Ok(())
}
