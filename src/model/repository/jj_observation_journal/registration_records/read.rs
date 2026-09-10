use super::super::{JournalError, ReadBudget, invalid, sql_error};
use super::StoredRecord;
use super::codec;
use super::types::{MAX_RECORD_BYTES, RegistrationRecord, WorkspaceRecord};
use rusqlite::{Connection, Params, Row, params};

pub(in crate::model::repository::jj_observation_journal) const REGISTRATION_COUNT_SQL: &str =
    "SELECT 1 FROM jj_native_registrations WHERE source_id COLLATE BINARY = ?1 LIMIT 2";
const WORKSPACE_COUNT_SQL: &str =
    "SELECT 1 FROM jj_native_workspaces WHERE source_id COLLATE BINARY = ?1
     AND workspace_name COLLATE BINARY = ?2 LIMIT 2";

const REGISTRATION_PAYLOAD_SQL: &str = "
SELECT CASE WHEN typeof(record) = 'blob' THEN length(record) ELSE NULL END,
 CASE WHEN typeof(record) != 'blob' THEN NULL WHEN length(record) <= ?2 THEN record ELSE NULL END,
 CASE WHEN typeof(checksum) = 'text' AND length(CAST(checksum AS BLOB)) = 64 THEN checksum ELSE NULL END,
 CASE WHEN typeof(source_id) = 'text' AND length(CAST(source_id AS BLOB)) = 64 THEN source_id ELSE NULL END,
 CASE WHEN typeof(baseline_id) = 'text' AND length(CAST(baseline_id AS BLOB)) = 64 THEN baseline_id ELSE NULL END,
 CASE WHEN typeof(source_root_key) = 'text' AND length(CAST(source_root_key AS BLOB)) = 64 THEN source_root_key ELSE NULL END
 FROM jj_native_registrations WHERE source_id COLLATE BINARY = ?1 LIMIT 1";

const WORKSPACE_PAYLOAD_SQL: &str = "
SELECT CASE WHEN typeof(record) = 'blob' THEN length(record) ELSE NULL END,
 CASE WHEN typeof(record) != 'blob' THEN NULL WHEN length(record) <= ?3 THEN record ELSE NULL END,
 CASE WHEN typeof(checksum) = 'text' AND length(CAST(checksum AS BLOB)) = 64 THEN checksum ELSE NULL END,
 CASE WHEN typeof(source_id) = 'text' AND length(CAST(source_id AS BLOB)) = 64 THEN source_id ELSE NULL END,
 CASE WHEN typeof(workspace_name) = 'text' AND length(CAST(workspace_name AS BLOB)) BETWEEN 1 AND 16384 THEN workspace_name ELSE NULL END,
 CASE WHEN typeof(locator_key) = 'text' AND length(CAST(locator_key AS BLOB)) = 64 THEN locator_key ELSE NULL END,
 CASE WHEN typeof(workspace_root_key) = 'text' AND length(CAST(workspace_root_key AS BLOB)) = 64 THEN workspace_root_key ELSE NULL END
 FROM jj_native_workspaces WHERE source_id COLLATE BINARY = ?1
 AND workspace_name COLLATE BINARY = ?2 LIMIT 1";

pub(super) fn registration(
    conn: &Connection,
    source: &str,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, JournalError> {
    Ok(registration_stored(conn, source, budget)?.map(StoredRecord::into_raw))
}

pub(in crate::model::repository::jj_observation_journal) fn registration_stored(
    conn: &Connection,
    source: &str,
    budget: &mut ReadBudget,
) -> Result<Option<StoredRecord<RegistrationRecord>>, JournalError> {
    if !has_one(conn, REGISTRATION_COUNT_SQL, [source])? {
        return Ok(None);
    }
    let limit = MAX_RECORD_BYTES.min(budget.remaining());
    let mut statement = conn
        .prepare(REGISTRATION_PAYLOAD_SQL)
        .map_err(|error| sql_error("prepare native registration record read", error))?;
    let mut rows = statement
        .query(params![source, limit])
        .map_err(|error| sql_error("read native registration record", error))?;
    let row = rows
        .next()
        .map_err(|error| sql_error("read native registration record row", error))?
        .ok_or_else(|| invalid("native registration selected record disappeared"))?;
    let (raw, checksum) = payload(row, limit, budget)?;
    let scalar_source = text(row, 3)?;
    let baseline = text(row, 4)?;
    let root_key = text(row, 5)?;
    let record = codec::decode_registration(&raw, raw.len() as u64, &checksum)?;
    if scalar_source != source
        || record.source_id != source
        || record.baseline_id != baseline
        || codec::source_root_key(&record)? != root_key
    {
        return Err(invalid(
            "native registration record scalar identity mismatch",
        ));
    }
    Ok(Some(StoredRecord {
        record,
        raw,
        checksum,
    }))
}

pub(super) fn workspace(
    conn: &Connection,
    source: &str,
    name: &str,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, JournalError> {
    Ok(workspace_stored(conn, source, name, budget)?.map(StoredRecord::into_raw))
}

pub(in crate::model::repository::jj_observation_journal) fn workspace_stored(
    conn: &Connection,
    source: &str,
    name: &str,
    budget: &mut ReadBudget,
) -> Result<Option<StoredRecord<WorkspaceRecord>>, JournalError> {
    if !has_one(conn, WORKSPACE_COUNT_SQL, [source, name])? {
        return Ok(None);
    }
    let limit = MAX_RECORD_BYTES.min(budget.remaining());
    let mut statement = conn
        .prepare(WORKSPACE_PAYLOAD_SQL)
        .map_err(|error| sql_error("prepare native workspace record read", error))?;
    let mut rows = statement
        .query(params![source, name, limit])
        .map_err(|error| sql_error("read native workspace record", error))?;
    let row = rows
        .next()
        .map_err(|error| sql_error("read native workspace record row", error))?
        .ok_or_else(|| invalid("native workspace selected record disappeared"))?;
    let (raw, checksum) = payload(row, limit, budget)?;
    let scalar_source = text(row, 3)?;
    let scalar_name = text(row, 4)?;
    let locator_key = text(row, 5)?;
    let root_key = text(row, 6)?;
    let record = codec::decode_workspace(&raw, raw.len() as u64, &checksum)?;
    if scalar_source != source
        || record.source_id != source
        || scalar_name != name
        || record.workspace_name != name
        || codec::locator_key(&record)? != locator_key
        || codec::workspace_root_key(&record)? != root_key
    {
        return Err(invalid("native workspace record scalar identity mismatch"));
    }
    Ok(Some(StoredRecord {
        record,
        raw,
        checksum,
    }))
}

pub(in crate::model::repository::jj_observation_journal) fn has_one(
    conn: &Connection,
    sql: &str,
    parameters: impl Params,
) -> Result<bool, JournalError> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| sql_error("prepare native registration cardinality read", error))?;
    let mut rows = statement
        .query(parameters)
        .map_err(|error| sql_error("read native registration cardinality", error))?;
    let present = rows
        .next()
        .map_err(|error| sql_error("read native registration cardinality row", error))?
        .is_some();
    if rows
        .next()
        .map_err(|error| sql_error("read native registration cardinality row", error))?
        .is_some()
    {
        return Err(invalid("native registration record has duplicate keys"));
    }
    Ok(present)
}

fn payload(
    row: &Row<'_>,
    limit: usize,
    budget: &mut ReadBudget,
) -> Result<(Vec<u8>, String), JournalError> {
    let length = row
        .get::<_, Option<u64>>(0)
        .map_err(|error| sql_error("read native registration payload length", error))?
        .ok_or_else(|| invalid("native registration payload is not a blob"))?;
    if length > limit as u64 {
        return Err(invalid(
            "native registration payload read byte limit exceeded",
        ));
    }
    // The complete BLOB was selected; every later failure retains this charge.
    budget.charge(length as usize)?;
    let raw: Vec<u8> = row
        .get(1)
        .map_err(|error| sql_error("read native registration payload", error))?;
    if raw.len() as u64 != length {
        return Err(invalid("native registration payload length mismatch"));
    }
    Ok((raw, text(row, 2)?))
}

fn text(row: &Row<'_>, column: usize) -> Result<String, JournalError> {
    row.get::<_, Option<String>>(column)
        .map_err(|error| sql_error("read native registration scalar", error))?
        .ok_or_else(|| invalid("native registration scalar type or length invalid"))
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
