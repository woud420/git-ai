use super::*;
use rusqlite::{Transaction, TransactionBehavior, params};

pub(crate) struct StagedRegistration<'j> {
    transaction: Transaction<'j>,
    snapshot: StoredRegistrationSnapshot,
}

impl StagedRegistration<'_> {
    pub(crate) fn snapshot(&self) -> &StoredRegistrationSnapshot {
        &self.snapshot
    }

    pub(crate) fn commit(self) -> Result<StoredRegistrationSnapshot, JournalError> {
        self.transaction
            .commit()
            .map_err(|error| sql_error("commit native registration installation", error))?;
        Ok(self.snapshot)
    }
}

impl JjObservationJournal {
    pub(crate) fn stage_registration_install<'j>(
        &'j mut self,
        prepared: PreparedRegistrationInstall<'_>,
        budget: &mut ReadBudget,
    ) -> Result<super::StagedRegistration<'j>, JournalError> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error("begin native registration installation", error))?;
        read::require_absent(&transaction, &prepared.registration.source_id)?;
        if read::guards_occupied(
            &transaction,
            &prepared.source_root_key,
            &prepared.locator_key,
        )? {
            return Err(invalid("native registration installation guard conflict"));
        }
        let PreparedRegistrationInstall {
            native,
            registration,
            workspace,
            source_root_key,
            locator_key,
            workspace_root_key,
            registration_bytes,
            workspace_bytes,
            registration_checksum,
            workspace_checksum,
        } = prepared;
        let (native_request, native_state) = native.insert_into(&transaction)?;
        let inserted = transaction.execute(
            "INSERT INTO jj_native_registrations(source_id, baseline_id, source_root_key, record, checksum)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![registration.source_id, registration.baseline_id, source_root_key,
                registration_bytes, registration_checksum],
        ).map_err(|error| sql_error("persist native registration record", error))?;
        if inserted != 1 {
            return Err(invalid(
                "native registration record insert did not persist one row",
            ));
        }
        let inserted = transaction.execute(
            "INSERT INTO jj_native_workspaces(source_id, workspace_name, locator_key, workspace_root_key, record, checksum)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![workspace.source_id, workspace.workspace_name, locator_key,
                workspace_root_key, workspace_bytes, workspace_checksum],
        ).map_err(|error| sql_error("persist native workspace record", error))?;
        if inserted != 1 {
            return Err(invalid(
                "native workspace record insert did not persist one row",
            ));
        }
        drop(registration_bytes);
        drop(workspace_bytes);
        read::require_initial_workspace(&transaction, &registration.source_id)?;
        let snapshot = read::snapshot(
            &transaction,
            &registration.source_id,
            &workspace.workspace_name,
            budget,
        )?
        .ok_or_else(|| invalid("native registration installation state gap"))?;
        if snapshot.registration.record != registration
            || snapshot.registration.checksum != registration_checksum
            || snapshot.original_workspace.record != workspace
            || snapshot.original_workspace.checksum != workspace_checksum
            || snapshot.native.state != native_state
            || !native_request.matches(&snapshot.native.record)
        {
            return Err(invalid(
                "native registration installation readback conflict",
            ));
        }
        Ok(StagedRegistration {
            transaction,
            snapshot,
        })
    }
}
