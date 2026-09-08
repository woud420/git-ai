use super::{DB_LABEL, JournalError, PersistenceError, sql_error};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

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
    } else {
        let version = tx.query_row(
            "SELECT length(value), substr(value, 1, 16) FROM schema_metadata WHERE key = 'version'",
            [], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        ).optional().map_err(|error| sql_error("read schema version", error))?;
        if version != Some((1, "1".to_owned())) {
            return Err(PersistenceError::Migration {
                db: DB_LABEL,
                found: "unsupported or malformed".to_owned(),
                supported: "1".to_owned(),
            }
            .into());
        }
        tx.prepare("SELECT source_id, state, checksum FROM jj_sources LIMIT 0")
            .map_err(|error| sql_error("verify source schema", error))?;
        tx.prepare("SELECT source_id, operation_id, sequence, payload, checksum FROM jj_operations LIMIT 0")
            .map_err(|error| sql_error("verify operation schema", error))?;
        tx.prepare("SELECT source_id, digest, receipt, checksum FROM jj_batches LIMIT 0")
            .map_err(|error| sql_error("verify receipt schema", error))?;
        tx.prepare("SELECT source_id, view_id, operation_id FROM jj_views LIMIT 0")
            .map_err(|error| sql_error("verify view schema", error))?;
    }
    tx.commit()
        .map_err(|error| sql_error("commit schema initialization", error))
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
