//! Inspection of individual stored records, without registration authority.

use super::{JjObservationJournal, JournalError, ReadBudget, sql_error};
use crate::model::jj_observation::validate_source;

pub(super) mod bounded;
pub(super) mod codec;
pub(super) mod read;
pub(super) mod types;

pub(crate) struct StoredRecord<T> {
    pub record: T,
    pub raw: Vec<u8>,
    pub checksum: String,
}

impl<T> StoredRecord<T> {
    fn into_raw(self) -> Vec<u8> {
        let Self {
            record,
            raw,
            checksum,
        } = self;
        drop(record);
        drop(checksum);
        raw
    }
}

impl JjObservationJournal {
    /// Returns one canonical structural record; absence is only a missing row.
    /// Neither result verifies a native baseline, source seal or complete registration.
    pub fn read_native_registration_record(
        &self,
        source_id: &str,
        budget: &mut ReadBudget,
    ) -> Result<Option<Vec<u8>>, JournalError> {
        validate_source(source_id)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin native registration record read", error))?;
        read::registration(&tx, source_id, budget)
    }

    /// Inspects only the exact source/name row, including individually valid orphans.
    /// Historical checkout bytes are not a current-readiness or native-evidence proof.
    pub fn read_native_workspace_record(
        &self,
        source_id: &str,
        workspace_name: &str,
        budget: &mut ReadBudget,
    ) -> Result<Option<Vec<u8>>, JournalError> {
        validate_source(source_id)?;
        codec::validate_name(workspace_name)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin native workspace record read", error))?;
        read::workspace(&tx, source_id, workspace_name, budget)
    }
}
