use super::{DB_LABEL, JournalError, PersistenceError, sql_error};
use rusqlite::{Connection, TransactionBehavior};

mod native;

const SCHEMA: &str = "
CREATE TABLE schema_metadata (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
INSERT INTO schema_metadata (key, value) VALUES ('version', '1');
CREATE TABLE jj_sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL
);
CREATE TABLE jj_operations (
    source_id TEXT NOT NULL REFERENCES jj_sources(source_id),
    operation_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    payload BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, operation_id),
    UNIQUE (source_id, sequence)
);
CREATE TABLE jj_batches (
    source_id TEXT NOT NULL REFERENCES jj_sources(source_id),
    digest TEXT NOT NULL,
    receipt BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, digest)
);
CREATE TABLE jj_views (
    source_id TEXT NOT NULL,
    view_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    PRIMARY KEY (source_id, view_id),
    FOREIGN KEY (source_id, operation_id) REFERENCES jj_operations(source_id, operation_id)
);";

pub(super) fn initialize(conn: &mut Connection) -> Result<(), JournalError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| sql_error("begin schema initialization", error))?;
    let table_count: u64 = tx
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| sql_error("inspect schema", error))?;
    if table_count == 0 {
        tx.execute_batch(SCHEMA)
            .map_err(|error| sql_error("create schema", error))?;
    }
    let version = read_version(&tx)?;
    verify_opaque_schema(&tx)?;
    if version == 1 {
        // Unconditional creation rejects even correctly shaped partial native
        // tables. The version and all DDL remain in this same transaction.
        native::create(&tx)?;
        let updated = tx
            .execute(
                "UPDATE schema_metadata SET value = '2' WHERE key = 'version'",
                [],
            )
            .map_err(|error| sql_error("record native schema version", error))?;
        if updated != 1 || read_version(&tx)? != 2 {
            return Err(unsupported_schema());
        }
    }
    native::verify(&tx)?;
    tx.commit()
        .map_err(|error| sql_error("commit schema initialization", error))
}

fn read_version(conn: &Connection) -> Result<u8, JournalError> {
    // Compare bytes so a permissive column collation cannot accept another
    // spelling; only the bounded numeric result is materialized.
    let mut statement = conn
        .prepare(
            "SELECT CASE WHEN typeof(value) = 'text' THEN
                 CASE CAST(value AS BLOB) WHEN X'31' THEN 1 WHEN X'32' THEN 2 END
             END FROM schema_metadata WHERE key = 'version' LIMIT 2",
        )
        .map_err(|error| sql_error("read schema version", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| sql_error("read schema version", error))?;
    let row = rows
        .next()
        .map_err(|error| sql_error("read schema version", error))?
        .ok_or_else(unsupported_schema)?;
    let version = row
        .get::<_, Option<u8>>(0)
        .map_err(|error| sql_error("read schema version", error))?
        .ok_or_else(unsupported_schema)?;
    if rows
        .next()
        .map_err(|error| sql_error("read schema version", error))?
        .is_some()
    {
        return Err(unsupported_schema());
    }
    Ok(version)
}

fn verify_opaque_schema(conn: &Connection) -> Result<(), JournalError> {
    conn.prepare("SELECT source_id, state, checksum FROM jj_sources LIMIT 0")
        .map_err(|error| sql_error("verify source schema", error))?;
    conn.prepare(
        "SELECT source_id, operation_id, sequence, payload, checksum FROM jj_operations LIMIT 0",
    )
    .map_err(|error| sql_error("verify operation schema", error))?;
    conn.prepare("SELECT source_id, digest, receipt, checksum FROM jj_batches LIMIT 0")
        .map_err(|error| sql_error("verify receipt schema", error))?;
    conn.prepare("SELECT source_id, view_id, operation_id FROM jj_views LIMIT 0")
        .map_err(|error| sql_error("verify view schema", error))?;
    Ok(())
}

fn unsupported_schema() -> JournalError {
    PersistenceError::Migration {
        db: DB_LABEL,
        found: "unsupported or malformed".to_owned(),
        supported: "2".to_owned(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use crate::model::repository::jj_observation_journal::JjObservationJournal;

    #[test]
    fn connection_uses_full_synchronous_wal_with_foreign_keys_and_memory_limit() {
        let directory = tempfile::tempdir().unwrap();
        let journal =
            JjObservationJournal::open_at_path(&directory.path().join("journal.db")).unwrap();
        let synchronous: i64 = journal
            .conn
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        let mode: String = journal
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        let foreign_keys: bool = journal
            .conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        let cache: i64 = journal
            .conn
            .pragma_query_value(None, "cache_size", |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 2);
        assert_eq!(mode, "wal");
        assert!(foreign_keys);
        assert_eq!(
            cache,
            i64::from(crate::model::repository::sqlite::MEMORY_LIMIT_CACHE_SIZE_KIB)
        );
    }
}
