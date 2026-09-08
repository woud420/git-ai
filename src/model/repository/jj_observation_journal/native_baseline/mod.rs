//! Structural storage for a current-state cutoff; no native decoding occurs here.

use super::{JjObservationJournal, JournalError, ReadBudget, invalid, sql_error};
use crate::model::jj_observation::{JjOperationEvidence, validate_source};
use rusqlite::TransactionBehavior;

mod bounded;
mod prepared;
mod read;
mod types;

use prepared::PreparedBaseline;
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
        let prepared = PreparedBaseline::new(request)?;
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
            if existing.state.baseline_id == prepared.state.baseline_id {
                if !prepared.request.matches(&existing.record) || existing.state != prepared.state {
                    return Err(invalid("native baseline receipt identity conflict"));
                }
                return Ok(NativeInstallOutcome::AlreadyInstalled(existing.state));
            }
            return Err(invalid("native baseline installation conflict"));
        }
        let (request, state) = prepared.insert_into(&tx)?;
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
