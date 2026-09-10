use super::{Error, output, shared};
use crate::config::Config;
use crate::model::repository::jj_observation_journal::ReadBudget;
use crate::operations::jj::admission::{NativeAdmissionExpectation, admit_registered_history};
use crate::operations::jj::registration::register_current_state;
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

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
    let mut budget = ReadBudget::new(shared::WRITE_SQL_READ_BYTES);
    let config = Config::get();
    let action = if expected.is_some() {
        "capture"
    } else {
        "initialize"
    };
    let context = shared::current_context(config, action)?;
    let mut journal = shared::open_writable_journal(path, deadline)?;
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
