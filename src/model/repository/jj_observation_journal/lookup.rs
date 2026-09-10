use super::{
    JjObservationJournal, JournalError, MAX_JJ_OBSERVATION_LOOKUP_LIMIT, MAX_METADATA_BYTES,
    ObservationStatus, ReadBudget, invalid, load_state_with_budget, records, sql_error,
};
use crate::model::jj_observation::{
    JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, is_root, validate_ids, validate_source,
};
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
        let mut budget = ReadBudget::new(MAX_METADATA_BYTES + MAX_JJ_OBSERVATION_BATCH_BYTES);
        self.lookup_observed_with_budget(source, operation_ids, &mut budget)
    }

    /// Reuses a caller's BLOB budget without refunding bytes on validation errors.
    /// Status and requested records still share one source-scoped read snapshot.
    pub fn lookup_observed_with_budget(
        &self,
        source: &str,
        operation_ids: &[String],
        budget: &mut ReadBudget,
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
        let state = load_state_with_budget(&tx, source, budget)?;
        if state.generation == 0 && has_source_records(&tx, source)? {
            return Err(invalid("observed source state gap"));
        }
        let operations =
            records::load_observed_records(&tx, source, &ids, state.pending_operations, budget)?;
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
