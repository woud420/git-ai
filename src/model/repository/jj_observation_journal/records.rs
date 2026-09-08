use super::{JournalError, MAX_RECORD_BYTES, decode_operation_row, invalid, sql_error};
use crate::model::jj_observation::{JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, is_root};
use rusqlite::{Connection, params};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn load_records(
    conn: &Connection,
    source: &str,
    ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, JjOperationEvidence>, JournalError> {
    let mut statement = conn
        .prepare(
            "SELECT operation_id, length(payload),
         CASE WHEN length(payload) <= ?3 THEN payload ELSE NULL END, substr(checksum, 1, 65)
         FROM jj_operations WHERE source_id = ?1 AND operation_id = ?2",
        )
        .map_err(|error| sql_error("prepare evidence read", error))?;
    let mut remaining = MAX_JJ_OBSERVATION_BATCH_BYTES;
    let mut records = BTreeMap::new();
    for id in ids {
        if is_root(id) {
            continue;
        }
        let limit = remaining.min(MAX_RECORD_BYTES);
        let mut rows = statement
            .query(params![source, id, limit])
            .map_err(|error| sql_error("read evidence", error))?;
        if let Some(row) = rows
            .next()
            .map_err(|error| sql_error("read evidence row", error))?
        {
            let length: u64 = row
                .get(1)
                .map_err(|error| sql_error("read evidence length", error))?;
            if length > limit as u64 {
                return Err(invalid("stored evidence read byte limit exceeded"));
            }
            let record = decode_operation_row(row, source)?;
            remaining -= length as usize;
            records.insert(id.clone(), record);
        }
    }
    Ok(records)
}

pub(super) fn view_representatives(
    conn: &Connection,
    source: &str,
    view_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, JournalError> {
    let mut statement = conn.prepare(
        "SELECT substr(operation_id, 1, 129) FROM jj_views WHERE source_id = ?1 AND view_id = ?2",
    ).map_err(|error| sql_error("prepare view identity lookup", error))?;
    let mut result = BTreeMap::new();
    for view in view_ids {
        let mut rows = statement
            .query(params![source, view])
            .map_err(|error| sql_error("read view identity", error))?;
        if let Some(row) = rows
            .next()
            .map_err(|error| sql_error("read view identity row", error))?
        {
            let operation: String = row
                .get(0)
                .map_err(|error| sql_error("read view operation identity", error))?;
            crate::model::jj_observation::validate_operation_id(&operation)?;
            result.insert(view.clone(), operation);
        }
    }
    Ok(result)
}

pub(super) fn pending_ids(
    conn: &Connection,
    source: &str,
    limit: usize,
    expected_count: usize,
) -> Result<Vec<String>, JournalError> {
    let mut statement = conn
        .prepare(
            "SELECT substr(operation_id, 1, 129), length(payload), sequence FROM jj_operations
         WHERE source_id = ?1 ORDER BY sequence LIMIT ?2",
        )
        .map_err(|error| sql_error("prepare pending read", error))?;
    let mut rows = statement
        .query(params![source, limit])
        .map_err(|error| sql_error("read pending identities", error))?;
    let mut remaining = MAX_JJ_OBSERVATION_BATCH_BYTES;
    let mut ids = Vec::with_capacity(limit);
    while let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read pending identity row", error))?
    {
        let id: String = row
            .get(0)
            .map_err(|error| sql_error("read pending identity", error))?;
        let length: u64 = row
            .get(1)
            .map_err(|error| sql_error("read pending length", error))?;
        let sequence: u64 = row
            .get(2)
            .map_err(|error| sql_error("read pending sequence", error))?;
        if sequence != ids.len() as u64 + 1 {
            return Err(invalid("pending operation sequence gap"));
        }
        if length > remaining.min(MAX_RECORD_BYTES) as u64 {
            return Err(invalid("pending read byte limit exceeded"));
        }
        crate::model::jj_observation::validate_operation_id(&id)?;
        remaining -= length as usize;
        ids.push(id);
    }
    if ids.len() != expected_count {
        return Err(invalid("pending operation count gap"));
    }
    Ok(ids)
}
