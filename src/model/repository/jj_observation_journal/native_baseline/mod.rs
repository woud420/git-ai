//! Structural storage for a current-state cutoff; no native decoding occurs here.

use super::{JjObservationJournal, JournalError, ReadBudget, codec, invalid, sql_error};
use crate::model::jj_observation::{JjOperationEvidence, validate_source};
use rusqlite::{TransactionBehavior, params};

mod bounded;
mod read;
mod types;

pub(crate) use types::NativeBaselineState;
use types::{MAX_BASELINE_BYTES, MAX_STATE_BYTES, Request, StoredBaseline};

pub(crate) struct NativeBaselineSnapshot {
    pub state: NativeBaselineState,
    pub record: StoredBaseline,
}

pub(crate) enum NativeInstallOutcome {
    Installed(NativeBaselineState),
    AlreadyInstalled(NativeBaselineState),
}

impl JjObservationJournal {
    pub(crate) fn persist_native_baseline(
        &mut self,
        source: &str,
        expected_generation: u64,
        profile: &str,
        heads: &[String],
        anchors: &[&JjOperationEvidence],
    ) -> Result<NativeInstallOutcome, JournalError> {
        let request = Request::new(source, expected_generation, profile, heads, anchors)?;
        // Only bounded vectors of references are canonicalized; raw evidence is not cloned.
        let record_bytes = codec::encode(&request, MAX_BASELINE_BYTES)?;
        let baseline_id = codec::checksum(&record_bytes);
        let state = request.state(baseline_id);
        let state_bytes = codec::encode(&state, MAX_STATE_BYTES)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error("begin native baseline installation", error))?;
        let existing = read::snapshot(
            &tx,
            source,
            &mut ReadBudget::new(MAX_BASELINE_BYTES + MAX_STATE_BYTES),
        )?;
        if let Some(existing) = existing {
            if existing.state.baseline_id == state.baseline_id {
                if !request.matches(&existing.record) || existing.state != state {
                    return Err(invalid("native baseline receipt identity conflict"));
                }
                return Ok(NativeInstallOutcome::AlreadyInstalled(existing.state));
            }
            return Err(invalid("native baseline installation conflict"));
        }
        let inserted = tx.execute(
            "INSERT INTO jj_native_baselines(source_id, baseline_id, record, checksum) VALUES (?1, ?2, ?3, ?4)",
            params![source, state.baseline_id, record_bytes, state.baseline_id],
        ).map_err(|error| sql_error("persist native baseline record", error))?;
        if inserted != 1 {
            return Err(invalid(
                "native baseline record insert did not persist one row",
            ));
        }
        let inserted = tx.execute(
            "INSERT INTO jj_native_sources(source_id, baseline_id, state, checksum) VALUES (?1, ?2, ?3, ?4)",
            params![source, state.baseline_id, state_bytes, codec::checksum(&state_bytes)],
        ).map_err(|error| sql_error("persist native baseline state", error))?;
        if inserted != 1 {
            return Err(invalid(
                "native baseline state insert did not persist one row",
            ));
        }
        drop(record_bytes);
        drop(state_bytes);
        // AFTER triggers can alter rows despite a successful affected-row count.
        let installed = read::snapshot(
            &tx,
            source,
            &mut ReadBudget::new(MAX_BASELINE_BYTES + MAX_STATE_BYTES),
        )?
        .ok_or_else(|| invalid("native baseline installation state gap"))?;
        if installed.state != state || !request.matches(&installed.record) {
            return Err(invalid("native baseline installation readback conflict"));
        }
        tx.commit()
            .map_err(|error| sql_error("commit native baseline installation", error))?;
        Ok(NativeInstallOutcome::Installed(state))
    }

    pub(crate) fn read_native_baseline(
        &self,
        source: &str,
        budget: &mut ReadBudget,
    ) -> Result<Option<NativeBaselineSnapshot>, JournalError> {
        validate_source(source)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin native baseline read", error))?;
        read::snapshot(&tx, source, budget)
    }
}
