use super::super::{JjObservationJournal, JournalError, ReadBudget, codec, invalid, sql_error};
use super::read;
use super::request::PreparedNativeAdmission;
use super::snapshot::{RegistrationIdentity, StoredAdmissionSnapshot};
use crate::model::jj_observation::validate_source;
use rusqlite::{Transaction, TransactionBehavior, params};

pub(crate) struct NativeAdmissionTransaction<'j> {
    transaction: Transaction<'j>,
    snapshot: StoredAdmissionSnapshot,
    source: String,
    workspace: Option<String>,
    requested_id: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NativeAdmissionOutcome {
    Admitted,
    AlreadyAdmitted,
}

pub(crate) struct StagedNativeAdmission<'j> {
    transaction: Transaction<'j>,
    outcome: NativeAdmissionOutcome,
    snapshot: StoredAdmissionSnapshot,
}

pub(crate) struct NativeAdmissionCommit<'j> {
    transaction: Transaction<'j>,
}

impl JjObservationJournal {
    pub(crate) fn read_native_admission_snapshot(
        &self,
        source: &str,
        workspace: Option<&str>,
        requested_id: Option<&str>,
        reads: &mut ReadBudget,
    ) -> Result<StoredAdmissionSnapshot, JournalError> {
        validate_selection(source, workspace, requested_id)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin native admission snapshot read", error))?;
        read::snapshot(&tx, source, workspace, requested_id, reads)
    }

    pub(crate) fn begin_native_admission<'j>(
        &'j mut self,
        source: &str,
        workspace: Option<&str>,
        requested_id: Option<&str>,
        reads: &mut ReadBudget,
    ) -> Result<NativeAdmissionTransaction<'j>, JournalError> {
        validate_selection(source, workspace, requested_id)?;
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error("begin native admission transaction", error))?;
        let snapshot = read::snapshot(&transaction, source, workspace, requested_id, reads)?;
        Ok(NativeAdmissionTransaction {
            transaction,
            snapshot,
            source: source.to_owned(),
            workspace: workspace.map(str::to_owned),
            requested_id: requested_id.map(str::to_owned),
        })
    }
}

fn validate_selection(
    source: &str,
    workspace: Option<&str>,
    requested: Option<&str>,
) -> Result<(), JournalError> {
    validate_source(source)?;
    if let Some(workspace) = workspace {
        super::super::registration_records::codec::validate_name(workspace)?;
    }
    if let Some(requested) = requested {
        validate_source(requested)?;
    }
    Ok(())
}

impl<'j> NativeAdmissionTransaction<'j> {
    pub(crate) fn snapshot(&self) -> &StoredAdmissionSnapshot {
        &self.snapshot
    }

    pub(crate) fn stage(
        self,
        prepared: PreparedNativeAdmission<'_>,
        reads: &mut ReadBudget,
    ) -> Result<StagedNativeAdmission<'j>, JournalError> {
        if self.requested_id.as_deref() != Some(prepared.admission_id())
            || self.source != prepared.request.source_id
        {
            return Err(invalid("native admission staged selection mismatch"));
        }
        prepared
            .request
            .require_registration(&self.snapshot.registration)?;
        if let Some(existing) = self.snapshot.requested() {
            if !prepared.request.matches(&existing.record) {
                return Err(invalid("native admission receipt identity conflict"));
            }
            return Ok(StagedNativeAdmission {
                transaction: self.transaction,
                outcome: NativeAdmissionOutcome::AlreadyAdmitted,
                snapshot: self.snapshot,
            });
        }
        if self.snapshot.cursor.generation != prepared.request.expected_admission_generation
            || !prepared
                .request
                .expected_admitted_head_ids
                .iter()
                .copied()
                .eq(self.snapshot.cursor.admitted_head_ids.iter())
        {
            return Err(invalid("native admission cursor conflict"));
        }
        let previous_id = self
            .snapshot
            .latest
            .as_ref()
            .map(|packet| packet.admission_id.clone());
        let registration_identity = RegistrationIdentity::from(&self.snapshot.registration);
        let Self {
            transaction,
            snapshot,
            source,
            workspace,
            requested_id,
        } = self;
        // Preflight was exposed for native verification. Release its large packets before readback.
        drop(snapshot);
        let PreparedNativeAdmission {
            request,
            state,
            record_bytes,
            state_bytes,
        } = prepared;
        let changed = transaction.execute(
            "INSERT INTO jj_native_admissions(source_id,admission_id,generation,record,checksum) VALUES (?1,?2,?3,?4,?5)",
            params![source, state.admission_id, state.generation, record_bytes, state.admission_id],
        ).map_err(|error| sql_error("persist native admission packet", error))?;
        require_one(changed)?;
        let state_checksum = codec::checksum(&state_bytes);
        let changed = if let Some(previous) = previous_id {
            transaction.execute(
                "UPDATE jj_native_admission_states SET admission_id=?2,state=?3,checksum=?4
                 WHERE source_id COLLATE BINARY=?1 AND admission_id COLLATE BINARY=?5",
                params![source, state.admission_id, state_bytes, state_checksum, previous],
            )
        } else {
            transaction.execute(
                "INSERT INTO jj_native_admission_states(source_id,admission_id,state,checksum) VALUES (?1,?2,?3,?4)",
                params![source, state.admission_id, state_bytes, state_checksum],
            )
        }.map_err(|error| sql_error("persist native admission state", error))?;
        require_one(changed)?;
        drop(record_bytes);
        drop(state_bytes);
        let snapshot = read::snapshot(
            &transaction,
            &source,
            workspace.as_deref(),
            requested_id.as_deref(),
            reads,
        )?;
        let selected = snapshot
            .requested()
            .ok_or_else(|| invalid("native admission staged request gap"))?;
        if !registration_identity.matches(&snapshot.registration)
            || selected.admission_id != state.admission_id
            || !request.matches(&selected.record)
            || snapshot.cursor.generation != state.generation
            || snapshot.cursor.admitted_head_ids != state.admitted_head_ids
            || snapshot
                .latest
                .as_ref()
                .is_none_or(|latest| latest.admission_id != state.admission_id)
        {
            return Err(invalid("native admission staged readback conflict"));
        }
        Ok(StagedNativeAdmission {
            transaction,
            outcome: NativeAdmissionOutcome::Admitted,
            snapshot,
        })
    }
}

fn require_one(changed: usize) -> Result<(), JournalError> {
    if changed != 1 {
        return Err(invalid("native admission write did not affect one row"));
    }
    Ok(())
}

impl<'j> StagedNativeAdmission<'j> {
    pub(crate) fn into_parts(
        self,
    ) -> (
        NativeAdmissionCommit<'j>,
        NativeAdmissionOutcome,
        StoredAdmissionSnapshot,
    ) {
        (
            NativeAdmissionCommit {
                transaction: self.transaction,
            },
            self.outcome,
            self.snapshot,
        )
    }
}

impl NativeAdmissionCommit<'_> {
    pub(crate) fn commit(self) -> Result<(), JournalError> {
        self.transaction
            .commit()
            .map_err(|error| sql_error("commit native admission", error))
    }
}
