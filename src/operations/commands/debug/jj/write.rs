use super::{Error, output, shared};
use crate::config::Config;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::jj::admission::{NativeAdmissionExpectation, admit_registered_history};
use crate::operations::jj::registration::register_current_state;
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

const SQL_READ_BYTES: usize = 48 * 1024 * 1024;

pub(super) fn initialize(path: &Path) -> Result<Value, Error> {
    run(path, None)
}

pub(super) fn capture(
    path: &Path,
    expected: NativeAdmissionExpectation<'_>,
) -> Result<Value, Error> {
    run(path, Some(expected))
}

fn run(path: &Path, expected: Option<NativeAdmissionExpectation<'_>>) -> Result<Value, Error> {
    let deadline = Instant::now() + shared::COMMAND_DEADLINE;
    let mut budget = ReadBudget::new(SQL_READ_BYTES);
    let config = Config::get();
    let action = if expected.is_some() {
        "capture"
    } else {
        "initialize"
    };
    let context = shared::current_context(config, action)?;
    ordinary_journal_path(path)?;
    check_deadline(deadline)?;
    let mut journal = JjObservationJournal::open_at_path(path).map_err(shared::journal_error)?;
    check_deadline(deadline)?;
    match expected {
        None => register_current_state(&mut journal, &context, config, deadline, &mut budget)
            .map(|value| output::initialize(&value))
            .map_err(|error| Error::new("registration_unavailable", error)),
        Some(expected) => admit_registered_history(
            &mut journal,
            &context,
            config,
            expected,
            deadline,
            &mut budget,
        )
        .map(|value| output::capture(&value))
        .map_err(|error| Error::new("admission_unavailable", error)),
    }
}

fn ordinary_journal_path(path: &Path) -> Result<(), Error> {
    // Bundled SQLite interprets URI names even without SQLITE_OPEN_URI.
    if path.as_os_str().is_empty()
        || path == Path::new(":memory:")
        || path.as_os_str().as_encoded_bytes().starts_with(b"file:")
    {
        return Err(Error::new(
            "journal_unavailable",
            "Journal requires an ordinary filesystem path.",
        ));
    }
    Ok(())
}

fn check_deadline(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        return Err(Error::new(
            "journal_unavailable",
            "Journal opening deadline exceeded.",
        ));
    }
    Ok(())
}
