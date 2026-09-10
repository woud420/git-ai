use super::{JournalError, indexes, metadata, sql_error};
use rusqlite::Connection;

const SCHEMA: &str = "
CREATE TABLE jj_native_admissions (
    source_id TEXT NOT NULL,
    admission_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, admission_id),
    UNIQUE (source_id, generation),
    FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id)
);
CREATE TABLE jj_native_admission_states (
    source_id TEXT PRIMARY KEY NOT NULL,
    admission_id TEXT NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, admission_id)
        REFERENCES jj_native_admissions(source_id, admission_id)
);";

pub(super) fn create(conn: &Connection) -> Result<(), JournalError> {
    conn.execute_batch(SCHEMA)
        .map_err(|error| sql_error("create native admission schema", error))
}

pub(super) fn verify(conn: &Connection) -> Result<(), JournalError> {
    metadata::verify_columns(
        conn,
        "jj_native_admissions",
        &[
            ("source_id", "TEXT", 1),
            ("admission_id", "TEXT", 2),
            ("generation", "INTEGER", 0),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    metadata::verify_columns(
        conn,
        "jj_native_admission_states",
        &[
            ("source_id", "TEXT", 1),
            ("admission_id", "TEXT", 0),
            ("state", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    metadata::verify_foreign_key(
        conn,
        "jj_native_admissions",
        "jj_native_registrations",
        &[("source_id", "source_id")],
    )?;
    metadata::verify_foreign_key(
        conn,
        "jj_native_admission_states",
        "jj_native_admissions",
        &[("source_id", "source_id"), ("admission_id", "admission_id")],
    )?;
    indexes::verify(
        conn,
        "jj_native_admissions",
        &[&["source_id", "admission_id"], &["source_id", "generation"]],
    )?;
    indexes::verify(conn, "jj_native_admission_states", &[&["source_id"]])?;
    // Index metadata alone cannot establish that SQLite accepts the parent's
    // declared collation for a foreign key. Preparing never executes these writes.
    conn.prepare(
        "INSERT INTO jj_native_admissions
         (source_id, admission_id, generation, record, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .map_err(|error| sql_error("verify admission foreign key binding", error))?;
    conn.prepare(
        "INSERT INTO jj_native_admission_states
         (source_id, admission_id, state, checksum)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .map_err(|error| sql_error("verify admission state foreign key binding", error))?;
    Ok(())
}
