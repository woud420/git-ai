use super::{
    JjObservationJournal, JournalError, MAX_JJ_OBSERVATION_LOOKUP_LIMIT, ObservationStatus,
    invalid, load_state, records, sql_error,
};
use crate::model::jj_observation::{JjOperationEvidence, is_root, validate_ids, validate_source};
use rusqlite::{Connection, params};
use std::collections::{BTreeMap, BTreeSet};

/// Captured record integrity is checked; native jj content addresses remain opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedEvidence {
    pub status: ObservationStatus,
    pub operations: BTreeMap<String, JjOperationEvidence>,
}

impl JjObservationJournal {
    pub fn lookup_observed(
        &self,
        source: &str,
        operation_ids: &[String],
    ) -> Result<ObservedEvidence, JournalError> {
        validate_source(source)?;
        validate_ids(operation_ids, MAX_JJ_OBSERVATION_LOOKUP_LIMIT)?;
        if operation_ids.iter().any(|id| is_root(id)) {
            return Err(invalid("root sentinel is not stored observation evidence"));
        }
        let ids: BTreeSet<_> = operation_ids.iter().cloned().collect();
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin observed evidence read", error))?;
        let state = load_state(&tx, source)?;
        if state.generation == 0 && has_source_records(&tx, source)? {
            return Err(invalid("observed source state gap"));
        }
        let operations =
            records::load_observed_records(&tx, source, &ids, state.pending_operations)?;
        if state
            .observed_heads
            .iter()
            .any(|head| ids.contains(head) && !operations.contains_key(head))
        {
            return Err(invalid("requested observed head evidence gap"));
        }
        Ok(ObservedEvidence {
            status: state.into_status(),
            operations,
        })
    }
}

fn has_source_records(conn: &Connection, source: &str) -> Result<bool, JournalError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM jj_operations WHERE source_id = ?1)
             OR EXISTS(SELECT 1 FROM jj_batches WHERE source_id = ?1)
             OR EXISTS(SELECT 1 FROM jj_views WHERE source_id = ?1)",
        params![source],
        |row| row.get(0),
    )
    .map_err(|error| sql_error("check observed source records", error))
}
