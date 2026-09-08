use super::super::{JournalError, ReadBudget, invalid, sql_error};
use super::codec;
use super::types::{NativeAdmissionState, StoredNativeAdmission};
use super::validate::{MAX_PACKET_BYTES, MAX_STATE_BYTES};
use rusqlite::{Connection, Row, params};

const STATE_SQL: &str = "SELECT
    CASE WHEN typeof(state) = 'blob' THEN length(state) ELSE NULL END,
    CASE WHEN typeof(state) = 'blob' AND length(state) <= ?2 THEN state ELSE NULL END,
    CASE WHEN typeof(checksum) = 'text' AND length(CAST(checksum AS BLOB)) = 64 THEN checksum ELSE NULL END,
    CASE WHEN typeof(source_id) = 'text' AND length(CAST(source_id AS BLOB)) = 64 THEN source_id ELSE NULL END,
    CASE WHEN typeof(admission_id) = 'text' AND length(CAST(admission_id AS BLOB)) = 64 THEN admission_id ELSE NULL END
    FROM jj_native_admission_states WHERE source_id COLLATE BINARY = ?1 LIMIT 1";

const PACKET_SQL: &str = "SELECT
    CASE WHEN typeof(record) = 'blob' THEN length(record) ELSE NULL END,
    CASE WHEN typeof(record) = 'blob' AND length(record) <= ?3 THEN record ELSE NULL END,
    CASE WHEN typeof(checksum) = 'text' AND length(CAST(checksum AS BLOB)) = 64 THEN checksum ELSE NULL END,
    CASE WHEN typeof(source_id) = 'text' AND length(CAST(source_id AS BLOB)) = 64 THEN source_id ELSE NULL END,
    CASE WHEN typeof(admission_id) = 'text' AND length(CAST(admission_id AS BLOB)) = 64 THEN admission_id ELSE NULL END,
    CASE WHEN typeof(generation) = 'integer' THEN generation ELSE NULL END
    FROM jj_native_admissions WHERE source_id COLLATE BINARY = ?1 AND admission_id COLLATE BINARY = ?2 LIMIT 1";

pub(super) fn state(
    conn: &Connection,
    source: &str,
    budget: &mut ReadBudget,
) -> Result<NativeAdmissionState, JournalError> {
    let limit = MAX_STATE_BYTES.min(budget.remaining());
    let mut statement = conn
        .prepare(STATE_SQL)
        .map_err(|error| sql_error("prepare native admission state read", error))?;
    let mut rows = statement
        .query(params![source, limit])
        .map_err(|error| sql_error("read native admission state", error))?;
    let row = rows
        .next()
        .map_err(|error| sql_error("read native admission state row", error))?
        .ok_or_else(|| invalid("native admission state gap"))?;
    let raw = payload(row, source, limit, budget)?;
    codec::decode_state(&raw, source, &text(row, 4)?, &text(row, 2)?)
}

pub(super) fn packet(
    conn: &Connection,
    source: &str,
    id: &str,
    budget: &mut ReadBudget,
) -> Result<StoredNativeAdmission, JournalError> {
    let limit = MAX_PACKET_BYTES.min(budget.remaining());
    let mut statement = conn
        .prepare(PACKET_SQL)
        .map_err(|error| sql_error("prepare native admission packet read", error))?;
    let mut rows = statement
        .query(params![source, id, limit])
        .map_err(|error| sql_error("read native admission packet", error))?;
    let row = rows
        .next()
        .map_err(|error| sql_error("read native admission packet row", error))?
        .ok_or_else(|| invalid("native admission packet gap"))?;
    let raw = payload(row, source, limit, budget)?;
    if text(row, 4)? != id || text(row, 2)? != id {
        return Err(invalid(
            "native admission packet scalar identity or checksum mismatch",
        ));
    }
    let generation: u64 = row
        .get::<_, Option<u64>>(5)
        .map_err(|error| sql_error("read native admission generation", error))?
        .ok_or_else(|| invalid("native admission generation type invalid"))?;
    codec::decode_packet(&raw, source, id, generation)
}

fn payload(
    row: &Row<'_>,
    source: &str,
    limit: usize,
    budget: &mut ReadBudget,
) -> Result<Vec<u8>, JournalError> {
    let length = row
        .get::<_, Option<u64>>(0)
        .map_err(|error| sql_error("read native admission payload length", error))?
        .ok_or_else(|| invalid("native admission payload is not a blob"))?;
    if length > limit as u64 {
        return Err(invalid("native admission payload read byte limit exceeded"));
    }
    // The query already selected the BLOB. Metadata and decode failures cannot refund it.
    budget.charge(length as usize)?;
    let raw: Vec<u8> = row
        .get(1)
        .map_err(|error| sql_error("read native admission payload", error))?;
    if raw.len() as u64 != length || text(row, 3)? != source {
        return Err(invalid(
            "native admission payload length or source mismatch",
        ));
    }
    Ok(raw)
}

pub(super) fn text(row: &Row<'_>, column: usize) -> Result<String, JournalError> {
    row.get::<_, Option<String>>(column)
        .map_err(|error| sql_error("read native admission scalar", error))?
        .ok_or_else(|| invalid("native admission scalar type or length invalid"))
}
