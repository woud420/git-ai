use super::super::registration::{self, StoredRegistrationSnapshot};
use super::super::registration_records::read::has_one;
use super::super::{JournalError, ReadBudget, invalid, sql_error};
use super::payload;
use super::snapshot::{
    StoredAdmissionSnapshot, require_packet_scope, require_scope, require_state_packet,
};
use super::types::{NativeAdmissionCursor, StoredNativeAdmission};
use crate::model::jj_observation::validate_source;
use rusqlite::{Connection, params};

const STATE_COUNT_SQL: &str = "SELECT 1 FROM jj_native_admission_states
    WHERE source_id COLLATE BINARY = ?1 LIMIT 2";
const PACKET_COUNT_SQL: &str = "SELECT 1 FROM jj_native_admissions
    WHERE source_id COLLATE BINARY = ?1 AND admission_id COLLATE BINARY = ?2 LIMIT 2";
const PACKET_EXISTS_SQL: &str = "SELECT 1 FROM jj_native_admissions
    WHERE source_id COLLATE BINARY = ?1 LIMIT 1";

pub(super) const GENERATION_IDS_SQL: &str = "SELECT
    CASE WHEN typeof(admission_id) = 'text' AND length(CAST(admission_id AS BLOB)) = 64 THEN admission_id ELSE NULL END
    FROM jj_native_admissions WHERE source_id COLLATE BINARY = ?1
    AND generation COLLATE BINARY = ?2 LIMIT 2";
pub(super) const HIGHER_GENERATION_SQL: &str = "SELECT 1 FROM jj_native_admissions
    WHERE source_id COLLATE BINARY = ?1 AND generation COLLATE BINARY > ?2 LIMIT 1";

pub(super) fn snapshot(
    conn: &Connection,
    source: &str,
    workspace: Option<&str>,
    requested: Option<&str>,
    budget: &mut ReadBudget,
) -> Result<StoredAdmissionSnapshot, JournalError> {
    let registration = registration::read::snapshot_selected(conn, source, workspace, budget)?
        .ok_or_else(|| invalid("native admission requires complete registration"))?;
    if !has_one(conn, STATE_COUNT_SQL, [source])? {
        if has_one(conn, PACKET_EXISTS_SQL, [source])? {
            return Err(invalid("native admission source state gap"));
        }
        let cursor = NativeAdmissionCursor {
            generation: 0,
            admitted_head_ids: registration.native.state.captured_head_ids.clone(),
        };
        return Ok(StoredAdmissionSnapshot {
            registration,
            cursor,
            latest: None,
            distinct_requested: None,
            requested_is_latest: false,
        });
    }
    let state = payload::state(conn, source, budget)?;
    require_scope(
        &registration,
        &state.source_id,
        &state.reader_profile,
        &state.initialization_receipt_id,
        &state.baseline_id,
        state.baseline_generation,
    )?;
    let latest = packet(conn, &registration, &state.admission_id, budget)?
        .ok_or_else(|| invalid("native admission current packet gap"))?;
    require_state_packet(&state, &latest)?;
    if has_one(
        conn,
        HIGHER_GENERATION_SQL,
        params![source, state.generation],
    )? {
        return Err(invalid("native admission state has a later packet"));
    }
    let requested_is_latest = requested == Some(latest.admission_id.as_str());
    let distinct_requested = match requested {
        Some(id) if !requested_is_latest => {
            let selected = packet(conn, &registration, id, budget)?;
            if selected
                .as_ref()
                .is_some_and(|packet| packet.generation > state.generation)
            {
                return Err(invalid(
                    "native admission requested generation is ahead of state",
                ));
            }
            selected
        }
        _ => None,
    };
    Ok(StoredAdmissionSnapshot {
        registration,
        cursor: NativeAdmissionCursor {
            generation: state.generation,
            admitted_head_ids: state.admitted_head_ids,
        },
        latest: Some(latest),
        distinct_requested,
        requested_is_latest,
    })
}

fn packet(
    conn: &Connection,
    registration: &StoredRegistrationSnapshot,
    id: &str,
    budget: &mut ReadBudget,
) -> Result<Option<StoredNativeAdmission>, JournalError> {
    let source = &registration.registration.record.source_id;
    if !has_one(conn, PACKET_COUNT_SQL, params![source, id])? {
        return Ok(None);
    }
    let packet = payload::packet(conn, source, id, budget)?;
    require_generation_identity(conn, source, packet.generation, id)?;
    require_packet_scope(registration, &packet.record)?;
    Ok(Some(packet))
}

fn require_generation_identity(
    conn: &Connection,
    source: &str,
    generation: u64,
    id: &str,
) -> Result<(), JournalError> {
    let mut statement = conn
        .prepare(GENERATION_IDS_SQL)
        .map_err(|error| sql_error("prepare native admission generation identity read", error))?;
    let mut rows = statement
        .query(params![source, generation])
        .map_err(|error| sql_error("read native admission generation identities", error))?;
    let row = rows
        .next()
        .map_err(|error| sql_error("read native admission generation row", error))?
        .ok_or_else(|| invalid("native admission generation gap"))?;
    let selected = payload::text(row, 0)?;
    validate_source(&selected)?;
    if selected != id
        || rows
            .next()
            .map_err(|error| sql_error("read native admission generation cardinality", error))?
            .is_some()
    {
        return Err(invalid("native admission generation identity conflict"));
    }
    Ok(())
}
