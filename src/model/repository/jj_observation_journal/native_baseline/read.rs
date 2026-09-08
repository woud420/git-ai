use super::super::{JournalError, ReadBudget, codec, invalid, sql_error};
use super::NativeBaselineSnapshot;
use super::types::{MAX_BASELINE_BYTES, MAX_STATE_BYTES, NativeBaselineState, StoredBaseline};
use rusqlite::{Connection, Row, params};
use serde::de::DeserializeOwned;

pub(super) fn snapshot(
    conn: &Connection,
    source: &str,
    budget: &mut ReadBudget,
) -> Result<Option<NativeBaselineSnapshot>, JournalError> {
    // This first read establishes the snapshot used by the orphan/receipt checks.
    let state = state(conn, source, budget)?;
    let ids = record_ids(conn, source)?;
    let Some(state) = state else {
        if !ids.is_empty() {
            return Err(invalid("native baseline source state gap"));
        }
        return Ok(None);
    };
    if ids.len() != 1 || ids[0] != state.baseline_id {
        return Err(invalid("native baseline record count or identity gap"));
    }
    let record = record(conn, source, &state.baseline_id, budget)?;
    if !state.matches(&record) {
        return Err(invalid("native baseline state and record mismatch"));
    }
    Ok(Some(NativeBaselineSnapshot { state, record }))
}

fn state(
    conn: &Connection,
    source: &str,
    budget: &mut ReadBudget,
) -> Result<Option<NativeBaselineState>, JournalError> {
    let limit = MAX_STATE_BYTES.min(budget.remaining());
    let mut statement = conn.prepare(
        "SELECT CASE WHEN typeof(state) = 'blob' THEN length(state) ELSE NULL END,
         CASE WHEN typeof(state) != 'blob' THEN NULL WHEN length(state) <= ?2 THEN state ELSE NULL END,
         CASE WHEN typeof(checksum) = 'text' AND length(CAST(checksum AS BLOB)) = 64 THEN checksum ELSE NULL END,
         CASE WHEN typeof(source_id) = 'text' AND length(CAST(source_id AS BLOB)) = 64 THEN source_id ELSE NULL END,
         CASE WHEN typeof(baseline_id) = 'text' AND length(CAST(baseline_id AS BLOB)) = 64 THEN baseline_id ELSE NULL END
         FROM jj_native_sources WHERE source_id = ?1"
    ).map_err(|error| sql_error("prepare native baseline state read", error))?;
    let mut rows = statement
        .query(params![source, limit])
        .map_err(|error| sql_error("read native baseline state", error))?;
    let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read native baseline state row", error))?
    else {
        return Ok(None);
    };
    let (state, raw): (NativeBaselineState, _) =
        payload(row, source, limit, MAX_STATE_BYTES, budget)?;
    let id = text(row, 4)?;
    state.validate(source, &id)?;
    if codec::encode(&state, MAX_STATE_BYTES)? != raw {
        return Err(invalid("native baseline state encoding is not canonical"));
    }
    Ok(Some(state))
}

fn record_ids(conn: &Connection, source: &str) -> Result<Vec<String>, JournalError> {
    let mut statement = conn.prepare(
        "SELECT CASE WHEN typeof(baseline_id) = 'text' AND length(CAST(baseline_id AS BLOB)) = 64 THEN baseline_id ELSE NULL END
         FROM jj_native_baselines WHERE source_id = ?1 LIMIT 2"
    ).map_err(|error| sql_error("prepare native baseline identity read", error))?;
    let mut rows = statement
        .query([source])
        .map_err(|error| sql_error("read native baseline identities", error))?;
    let mut ids = Vec::with_capacity(2);
    while let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read native baseline identity row", error))?
    {
        let id = text(row, 0)?;
        crate::model::jj_observation::validate_source(&id)
            .map_err(|_| invalid("native baseline digest identity invalid"))?;
        ids.push(id);
    }
    if ids.len() > 1 {
        return Err(invalid("multiple native baseline records in one source"));
    }
    Ok(ids)
}

fn record(
    conn: &Connection,
    source: &str,
    id: &str,
    budget: &mut ReadBudget,
) -> Result<StoredBaseline, JournalError> {
    let limit = MAX_BASELINE_BYTES.min(budget.remaining());
    let mut statement = conn.prepare(
        "SELECT CASE WHEN typeof(record) = 'blob' THEN length(record) ELSE NULL END,
         CASE WHEN typeof(record) != 'blob' THEN NULL WHEN length(record) <= ?3 THEN record ELSE NULL END,
         CASE WHEN typeof(checksum) = 'text' AND length(CAST(checksum AS BLOB)) = 64 THEN checksum ELSE NULL END,
         CASE WHEN typeof(source_id) = 'text' AND length(CAST(source_id AS BLOB)) = 64 THEN source_id ELSE NULL END,
         CASE WHEN typeof(baseline_id) = 'text' AND length(CAST(baseline_id AS BLOB)) = 64 THEN baseline_id ELSE NULL END
         FROM jj_native_baselines WHERE source_id = ?1 AND baseline_id = ?2"
    ).map_err(|error| sql_error("prepare native baseline record read", error))?;
    let mut rows = statement
        .query(params![source, id, limit])
        .map_err(|error| sql_error("read native baseline record", error))?;
    let row = rows
        .next()
        .map_err(|error| sql_error("read native baseline record row", error))?
        .ok_or_else(|| invalid("native baseline record gap"))?;
    let (record, raw): (StoredBaseline, _) =
        payload(row, source, limit, MAX_BASELINE_BYTES, budget)?;
    if text(row, 4)? != id || codec::checksum(&raw) != id {
        return Err(invalid("native baseline record digest mismatch"));
    }
    record.validate(source)?;
    if codec::encode(&record, MAX_BASELINE_BYTES)? != raw {
        return Err(invalid("native baseline record encoding is not canonical"));
    }
    Ok(record)
}

fn payload<T: DeserializeOwned>(
    row: &Row<'_>,
    source: &str,
    selected_limit: usize,
    record_limit: usize,
    budget: &mut ReadBudget,
) -> Result<(T, Vec<u8>), JournalError> {
    let length: u64 = row
        .get::<_, Option<u64>>(0)
        .map_err(|error| sql_error("read native baseline payload length", error))?
        .ok_or_else(|| invalid("native baseline payload is not a blob"))?;
    if length > selected_limit as u64 {
        return Err(invalid("native baseline payload read byte limit exceeded"));
    }
    // SQLite selected this complete BLOB, so later metadata/decode errors cannot refund it.
    budget.charge(length as usize)?;
    let raw: Vec<u8> = row
        .get(1)
        .map_err(|error| sql_error("read native baseline payload", error))?;
    let checksum = text(row, 2)?;
    if text(row, 3)? != source {
        return Err(invalid("native baseline scalar source mismatch"));
    }
    let value = codec::decode(&raw, length, &checksum, record_limit)?;
    Ok((value, raw))
}

fn text(row: &Row<'_>, column: usize) -> Result<String, JournalError> {
    row.get::<_, Option<String>>(column)
        .map_err(|error| sql_error("read native baseline scalar", error))?
        .ok_or_else(|| invalid("native baseline scalar type or length invalid"))
}
