use crate::model::jj_observation::{JJ_OBSERVATION_READER_PROFILE, validate_source};
use crate::model::jj_observer::{JjObserverError, JjObserverTarget, paths};
use crate::model::repository::jj_observation_journal::validate_workspace_name;
use crate::model::repository::{error::PersistenceError, sqlite};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

const DDL: &str = "CREATE TABLE jj_observer_intent (slot INTEGER PRIMARY KEY CHECK (slot = 1), revision INTEGER NOT NULL CHECK (revision > 0), payload BLOB NOT NULL CHECK (length(payload) <= 524288))";
const MAX_PAYLOAD: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredTarget {
    pub(crate) journal_path_hex: String,
    pub(crate) workspace_path_hex: String,
    pub(crate) metadata: JjObserverTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredIntent {
    pub(crate) schema_version: u16,
    pub(crate) revision: u64,
    pub(crate) target: StoredTarget,
    pub(crate) enabled: bool,
    pub(crate) blocked: Option<JjObserverError>,
}

#[derive(Debug)]
pub(crate) enum ObserverStoreError {
    Validation(&'static str),
    Persistence(PersistenceError),
}

impl std::fmt::Display for ObserverStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(message) => f.write_str(message),
            Self::Persistence(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ObserverStoreError {}

fn sql(error: rusqlite::Error) -> ObserverStoreError {
    ObserverStoreError::Persistence(PersistenceError::sqlite(
        "jj observer intent",
        "storage",
        &error,
    ))
}
fn io(error: std::io::Error) -> ObserverStoreError {
    ObserverStoreError::Persistence(PersistenceError::Io {
        operation: "jj observer intent storage",
        path: String::new(),
        kind: error.kind(),
        message: error.to_string(),
    })
}
fn invalid() -> ObserverStoreError {
    ObserverStoreError::Validation("Observer intent is invalid or unavailable.")
}

fn validate(value: &StoredIntent) -> Result<(), ObserverStoreError> {
    if value.schema_version != 1 || value.revision == 0 || value.revision > i64::MAX as u64 {
        return Err(invalid());
    }
    let target = &value.target;
    paths::decode(&target.journal_path_hex).map_err(|_| invalid())?;
    paths::decode(&target.workspace_path_hex).map_err(|_| invalid())?;
    let metadata = &target.metadata;
    for id in [
        &metadata.source_id,
        &metadata.initialization_receipt_id,
        &metadata.baseline_id,
        &metadata.attachment_id,
    ] {
        validate_source(id).map_err(|_| invalid())?;
    }
    if metadata.reader_profile != JJ_OBSERVATION_READER_PROFILE || metadata.baseline_generation != 1
    {
        return Err(invalid());
    }
    validate_workspace_name(&metadata.workspace_name).map_err(|_| invalid())?;
    if let Some(error) = &value.blocked
        && (!value.enabled
            || !error.persisted
            || error.code.is_empty()
            || error.code.len() > 64
            || !error
                .code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || error.message.len() > 4096
            || error.message.chars().count() > 1024)
    {
        return Err(invalid());
    }
    Ok(())
}

fn ordinary_path(path: &Path) -> Result<(), ObserverStoreError> {
    if path.as_os_str().is_empty()
        || path == Path::new(":memory:")
        || path.as_os_str().as_encoded_bytes().starts_with(b"file:")
    {
        return Err(invalid());
    }
    Ok(())
}

fn open(path: &Path, write: bool, initialize: bool) -> Result<Connection, ObserverStoreError> {
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let conn = sqlite::open_with_flags_and_memory_limits(path, flags).map_err(sql)?;
    if write
        && conn
            .is_readonly(rusqlite::DatabaseName::Main)
            .map_err(sql)?
    {
        return Err(invalid());
    }
    conn.busy_timeout(Duration::from_millis(250)).map_err(sql)?;
    let mode: String = conn
        .query_row(
            if initialize {
                "PRAGMA journal_mode=WAL"
            } else {
                "PRAGMA journal_mode"
            },
            [],
            |row| row.get(0),
        )
        .map_err(sql)?;
    if mode != "wal" {
        return Err(invalid());
    }
    conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;")
        .map_err(sql)?;
    Ok(conn)
}

fn verify_schema(conn: &Connection) -> Result<(), ObserverStoreError> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql)?;
    let exact: bool = conn.query_row(
        "SELECT COUNT(*)=1 AND COALESCE(MIN(type='table' AND name='jj_observer_intent' AND tbl_name='jj_observer_intent' AND sql=?1),0) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'",
        [DDL], |r| r.get(0)
    ).map_err(sql)?;
    if version != 1 || !exact {
        return Err(invalid());
    }
    Ok(())
}

fn read(conn: &Connection) -> Result<Option<StoredIntent>, ObserverStoreError> {
    let mut statement = conn.prepare(
        "SELECT typeof(slot),slot,typeof(revision),revision,typeof(payload),length(payload) FROM jj_observer_intent LIMIT 2"
    ).map_err(sql)?;
    let mut rows = statement.query([]).map_err(sql)?;
    let Some(row) = rows.next().map_err(sql)? else {
        return Ok(None);
    };
    if row.get::<_, String>(0).map_err(sql)? != "integer"
        || row.get::<_, i64>(1).map_err(sql)? != 1
        || row.get::<_, String>(2).map_err(sql)? != "integer"
        || row.get::<_, String>(4).map_err(sql)? != "blob"
    {
        return Err(invalid());
    }
    let revision: i64 = row.get(3).map_err(sql)?;
    let length: i64 = row.get(5).map_err(sql)?;
    if revision <= 0
        || length <= 0
        || length > MAX_PAYLOAD as i64
        || rows.next().map_err(sql)?.is_some()
    {
        return Err(invalid());
    }
    let bytes: Vec<u8> = conn
        .query_row(
            "SELECT payload FROM jj_observer_intent WHERE slot=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let value: StoredIntent = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    validate(&value)?;
    if value.revision != revision as u64 {
        return Err(invalid());
    }
    Ok(Some(value))
}

pub(crate) fn load(path: &Path) -> Result<Option<StoredIntent>, ObserverStoreError> {
    ordinary_path(path)?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error)),
        Ok(_) => {}
    }
    let mut conn = open(path, false, false)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(sql)?;
    verify_schema(&tx)?;
    read(&tx)
}

pub(crate) fn replace(
    path: &Path,
    expected: &Option<StoredIntent>,
    next: &StoredIntent,
) -> Result<(), ObserverStoreError> {
    ordinary_path(path)?;
    validate(next)?;
    if let Some(value) = expected {
        validate(value)?;
    }
    let revision = expected.as_ref().map_or(0, |value| value.revision);
    if revision.checked_add(1) != Some(next.revision) {
        return Err(invalid());
    }
    let bytes = serde_json::to_vec(next).map_err(|_| invalid())?;
    if bytes.len() > MAX_PAYLOAD {
        return Err(invalid());
    }
    let created = if expected.is_none() {
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            // Close before SQLite acquires POSIX locks: another fd close could release them.
            Ok(file) => {
                drop(file);
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(io(error)),
        }
    } else {
        false
    };
    let mut conn = open(path, true, created)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql)?;
    if created {
        tx.execute_batch(DDL).map_err(sql)?;
        tx.pragma_update(None, "user_version", 1).map_err(sql)?;
    }
    verify_schema(&tx)?;
    if read(&tx)?.as_ref() != expected.as_ref() {
        return Err(invalid());
    }
    let changed = if expected.is_some() {
        tx.execute(
            "UPDATE jj_observer_intent SET revision=?1,payload=?2 WHERE slot=1 AND revision=?3",
            params![next.revision, bytes, revision],
        )
        .map_err(sql)?
    } else {
        tx.execute(
            "INSERT INTO jj_observer_intent(slot,revision,payload) VALUES(1,?1,?2)",
            params![next.revision, bytes],
        )
        .map_err(sql)?
    };
    if changed != 1 || read(&tx)?.as_ref() != Some(next) {
        return Err(invalid());
    }
    tx.commit().map_err(sql)?;
    if created {
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
