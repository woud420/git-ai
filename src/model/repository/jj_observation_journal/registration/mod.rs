//! Structural registration composition; filesystem and native verification belong above storage.

use super::native_baseline::NativeBaselineSnapshot;
use super::{JjObservationJournal, JournalError, ReadBudget, invalid, sql_error};
use crate::model::jj_observation::validate_source;

pub(crate) use super::registration_records::StoredRecord;
pub(crate) use super::registration_records::bounded::ByteString;
pub(crate) use super::registration_records::codec::{
    source_root_guard, workspace_locator_guard, workspace_root_guard,
};
pub(crate) use super::registration_records::types::{
    BaselineRelation, DirectoryIdentity, Platform, RegistrationRecord, SelectedCheckout,
    SourceBinding, WorkspaceBinding, WorkspaceLocator, WorkspaceRecord,
};

mod prepared;
mod read;
mod staged;

pub(crate) use prepared::PreparedRegistrationInstall;
pub(crate) use staged::StagedRegistration;

pub(crate) struct RegistrationMetadata {
    pub seal_bytes: Vec<u8>,
    pub source_binding: SourceBinding,
    pub workspace: WorkspaceRegistrationMetadata,
}

pub(crate) struct WorkspaceRegistrationMetadata {
    pub workspace_name: String,
    pub attachment_id: String,
    pub locator: WorkspaceLocator,
    pub workspace_binding: WorkspaceBinding,
    pub selected_checkout: SelectedCheckout,
}

pub(crate) struct StoredRegistrationSnapshot {
    pub registration: StoredRecord<RegistrationRecord>,
    pub original_workspace: StoredRecord<WorkspaceRecord>,
    selected_workspace: Option<StoredRecord<WorkspaceRecord>>,
    pub native: NativeBaselineSnapshot,
}

impl StoredRegistrationSnapshot {
    pub(crate) fn selected_workspace(&self) -> &StoredRecord<WorkspaceRecord> {
        self.selected_workspace
            .as_ref()
            .unwrap_or(&self.original_workspace)
    }
}

impl JjObservationJournal {
    pub(crate) fn registration_guards_occupied(
        &self,
        source_root_key: &str,
        locator_key: &str,
    ) -> Result<bool, JournalError> {
        validate_source(source_root_key)?;
        validate_source(locator_key)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin native registration guard read", error))?;
        read::guards_occupied(&tx, source_root_key, locator_key)
    }

    pub(crate) fn read_registration_snapshot(
        &self,
        source: &str,
        workspace: &str,
        budget: &mut ReadBudget,
    ) -> Result<Option<StoredRegistrationSnapshot>, JournalError> {
        validate_source(source)?;
        super::registration_records::codec::validate_name(workspace)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin complete native registration read", error))?;
        read::snapshot(&tx, source, workspace, budget)
    }
}

#[cfg(test)]
mod tests;
