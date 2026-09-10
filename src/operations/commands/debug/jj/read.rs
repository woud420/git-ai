use super::{Error, output, shared};
use crate::config::Config;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::jj::admission::{read_native_admission, read_registered_admission_state};
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

const SQL_READ_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn status(path: &Path) -> Result<Value, Error> {
    let deadline = Instant::now() + shared::COMMAND_DEADLINE;
    let config = Config::get();
    let context = shared::current_context(config, "status")?;
    let journal = open_journal(path)?;
    let state = read_registered_admission_state(
        &journal,
        &context,
        config,
        deadline,
        &mut ReadBudget::new(SQL_READ_BYTES),
    )
    .map_err(|error| Error::new("admission_unavailable", error))?;
    Ok(output::status(&state))
}

pub(super) fn receipt(path: &Path, source: &str, admission: &str) -> Result<Value, Error> {
    let deadline = Instant::now() + shared::COMMAND_DEADLINE;
    let journal = open_journal(path)?;
    let value = read_native_admission(
        &journal,
        source,
        admission,
        deadline,
        &mut ReadBudget::new(SQL_READ_BYTES),
    )
    .map_err(|error| Error::new("admission_unavailable", error))?;
    Ok(output::admission(value.as_ref()))
}

fn open_journal(path: &Path) -> Result<JjObservationJournal, Error> {
    JjObservationJournal::open_read_only_at_path(path).map_err(shared::journal_error)
}
