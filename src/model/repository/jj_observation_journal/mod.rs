//! Durable capture of opaque jj evidence; this journal never applies attribution.

use crate::model::jj_observation::{
    JJ_OBSERVATION_READER_PROFILE, JJ_OBSERVATION_SCHEMA_VERSION, JjObservationError,
    JjOperationEvidence, MAX_JJ_OBSERVATION_HEADS, MAX_JJ_OBSERVATION_OPERATION_BYTES,
    validate_ids, validate_profile, validate_source,
};
use crate::model::repository::{error::PersistenceError, sqlite};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

mod capture;
mod codec;
mod graph;
mod lookup;
pub(crate) mod native_baseline;
mod read_budget;
mod records;
mod registration_records;
mod schema;

pub const MAX_JJ_OBSERVATION_PENDING_LIMIT: usize = 128;
pub const MAX_JJ_OBSERVATION_LOOKUP_LIMIT: usize = 128;
pub use lookup::ObservedEvidence;
pub use read_budget::ReadBudget;
const MAX_RECORD_BYTES: usize = 2 * MAX_JJ_OBSERVATION_OPERATION_BYTES + 64 * 1024;
const MAX_METADATA_BYTES: usize = 128 * 1024;
const DB_LABEL: &str = "jj observations";

#[derive(Debug)]
pub enum JournalError {
    Validation(JjObservationError),
    Persistence(PersistenceError),
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(f),
            Self::Persistence(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<JjObservationError> for JournalError {
    fn from(error: JjObservationError) -> Self {
        Self::Validation(error)
    }
}

impl From<PersistenceError> for JournalError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

fn invalid(message: &'static str) -> JournalError {
    JjObservationError(message).into()
}

fn sql_error(operation: &'static str, error: rusqlite::Error) -> JournalError {
    PersistenceError::sqlite(DB_LABEL, operation, &error).into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureOutcome {
    Captured { inserted_operations: usize },
    AlreadyCaptured,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservationStatus {
    pub generation: u64,
    pub observed_heads: Vec<String>,
    pub applied_heads: Vec<String>,
    pub pending_operations: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredState {
    schema_version: u16,
    reader_profile: String,
    source_id: String,
    generation: u64,
    observed_heads: Vec<String>,
    applied_heads: Vec<String>,
    pending_operations: u64,
}

impl StoredState {
    fn fresh(source: &str) -> Self {
        Self {
            schema_version: JJ_OBSERVATION_SCHEMA_VERSION,
            reader_profile: JJ_OBSERVATION_READER_PROFILE.to_owned(),
            source_id: source.to_owned(),
            generation: 0,
            observed_heads: Vec::new(),
            applied_heads: Vec::new(),
            pending_operations: 0,
        }
    }

    fn into_status(self) -> ObservationStatus {
        ObservationStatus {
            generation: self.generation,
            observed_heads: self.observed_heads,
            applied_heads: self.applied_heads,
            pending_operations: self.pending_operations,
        }
    }

    fn validate(&self, source: &str) -> Result<(), JournalError> {
        validate_profile(self.schema_version, &self.reader_profile)?;
        if self.source_id != source {
            return Err(invalid("stored source identity mismatch"));
        }
        validate_ids(&self.observed_heads, MAX_JJ_OBSERVATION_HEADS)?;
        if self.generation == 0 || self.observed_heads.is_empty() || !self.applied_heads.is_empty()
        {
            return Err(invalid("unsupported stored progress state"));
        }
        if self.pending_operations > i64::MAX as u64 {
            return Err(invalid("stored operation count limit exceeded"));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredOperation {
    schema_version: u16,
    reader_profile: String,
    source_id: String,
    evidence: JjOperationEvidence,
}

pub struct JjObservationJournal {
    conn: Connection,
}

impl JjObservationJournal {
    pub fn open_at_path(path: &Path) -> Result<Self, JournalError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| PersistenceError::Io {
                operation: "create jj journal directory",
                path: parent.display().to_string(),
                kind: error.kind(),
                message: error.to_string(),
            })?;
        }
        let mut conn = sqlite::open_writable_with_memory_limits(path)
            .map_err(|error| sql_error("open", error))?;
        conn.busy_timeout(std::time::Duration::from_millis(250))
            .map_err(|error| sql_error("configure busy timeout", error))?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(|error| sql_error("configure durability", error))?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|error| sql_error("configure foreign keys", error))?;
        schema::initialize(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn status(&self, source: &str) -> Result<ObservationStatus, JournalError> {
        validate_source(source)?;
        Ok(load_state(&self.conn, source)?.into_status())
    }

    pub fn pending(
        &self,
        source: &str,
        limit: usize,
    ) -> Result<Vec<JjOperationEvidence>, JournalError> {
        validate_source(source)?;
        if limit == 0 || limit > MAX_JJ_OBSERVATION_PENDING_LIMIT {
            return Err(invalid("pending read limit exceeded"));
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin pending read", error))?;
        let state = load_state(&tx, source)?;
        let expected_count = state.pending_operations.min(limit as u64) as usize;
        let ids = records::pending_ids(&tx, source, limit, expected_count)?;
        let mut records = records::load_records(&tx, source, &ids.iter().cloned().collect())?;
        ids.into_iter()
            .map(|id| {
                records
                    .remove(&id)
                    .ok_or_else(|| invalid("pending operation gap"))
            })
            .collect()
    }
}

fn load_state(conn: &Connection, source: &str) -> Result<StoredState, JournalError> {
    load_state_with_budget(conn, source, &mut ReadBudget::new(MAX_METADATA_BYTES))
}

fn load_state_with_budget(
    conn: &Connection,
    source: &str,
    budget: &mut ReadBudget,
) -> Result<StoredState, JournalError> {
    let limit = MAX_METADATA_BYTES.min(budget.remaining());
    let mut statement = conn
        .prepare(
            "SELECT CASE WHEN typeof(state) = 'blob' THEN length(state) ELSE NULL END,
         CASE WHEN typeof(state) != 'blob' THEN NULL
              WHEN length(state) <= ?2 THEN state ELSE NULL END,
         substr(checksum, 1, 65) FROM jj_sources WHERE source_id = ?1",
        )
        .map_err(|error| sql_error("prepare progress read", error))?;
    let mut rows = statement
        .query(params![source, limit])
        .map_err(|error| sql_error("read progress", error))?;
    let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read progress row", error))?
    else {
        return Ok(StoredState::fresh(source));
    };
    let length: u64 = row
        .get::<_, Option<u64>>(0)
        .map_err(|error| sql_error("read progress length", error))?
        .ok_or_else(|| invalid("stored progress payload is not a blob"))?;
    if length > limit as u64 {
        return Err(invalid("stored payload read byte limit exceeded"));
    }
    budget.charge(length as usize)?;
    let bytes: Vec<u8> = row
        .get(1)
        .map_err(|error| sql_error("read progress payload", error))?;
    let checksum: String = row
        .get(2)
        .map_err(|error| sql_error("read progress checksum", error))?;
    let state: StoredState = codec::decode(&bytes, length, &checksum, MAX_METADATA_BYTES)?;
    state.validate(source)?;
    Ok(state)
}

fn decode_operation_row(
    row: &rusqlite::Row<'_>,
    source: &str,
) -> Result<JjOperationEvidence, JournalError> {
    let operation_id: String = row
        .get(0)
        .map_err(|error| sql_error("read operation identity", error))?;
    let length: u64 = row
        .get(1)
        .map_err(|error| sql_error("read operation length", error))?;
    let bytes: Vec<u8> = row
        .get(2)
        .map_err(|error| sql_error("read operation payload", error))?;
    let checksum: String = row
        .get(3)
        .map_err(|error| sql_error("read operation checksum", error))?;
    let record: StoredOperation = codec::decode(&bytes, length, &checksum, MAX_RECORD_BYTES)?;
    validate_profile(record.schema_version, &record.reader_profile)?;
    if record.source_id != source || record.evidence.operation_id != operation_id {
        return Err(invalid("stored operation identity mismatch"));
    }
    record.evidence.validate()?;
    Ok(record.evidence)
}
