use super::{JournalError, metadata, sql_error};
use rusqlite::Connection;

const SCHEMA: &str = "
CREATE TABLE jj_native_baselines (
    source_id TEXT NOT NULL,
    baseline_id TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, baseline_id)
);
CREATE TABLE jj_native_sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);";

pub(super) fn create(conn: &Connection) -> Result<(), JournalError> {
    conn.execute_batch(SCHEMA)
        .map_err(|error| sql_error("create native baseline schema", error))
}

pub(super) fn verify(conn: &Connection) -> Result<(), JournalError> {
    metadata::verify_columns(
        conn,
        "jj_native_baselines",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 2),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    metadata::verify_columns(
        conn,
        "jj_native_sources",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 0),
            ("state", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    metadata::verify_foreign_key(conn, "jj_native_baselines", "jj_native_baselines", &[])?;
    metadata::verify_foreign_key(
        conn,
        "jj_native_sources",
        "jj_native_baselines",
        &[("source_id", "source_id"), ("baseline_id", "baseline_id")],
    )
}
