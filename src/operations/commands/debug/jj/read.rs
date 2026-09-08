use super::{Error, output};
use crate::config::Config;
use crate::model::repository::error::PersistenceError;
use crate::model::repository::jj_observation_journal::{
    JjObservationJournal, JournalError, ReadBudget,
};
use crate::operations::jj::admission::{read_native_admission, read_registered_admission_state};
use crate::operations::workspace_context::discover;
use serde_json::Value;
use std::path::Path;
use std::time::{Duration, Instant};

const COMMAND_DEADLINE: Duration = Duration::from_secs(5);
const SQL_READ_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn status(path: &Path) -> Result<Value, Error> {
    let deadline = Instant::now() + COMMAND_DEADLINE;
    let config = Config::get();
    if !config.has_allowed_repositories() {
        return Err(Error::new(
            "collection_disabled",
            "Repository collection is not allowed.",
        ));
    }
    let cwd = std::env::current_dir().map_err(|error| Error::new("context_unavailable", error))?;
    let context = discover(&cwd).map_err(|error| Error::new("context_unavailable", error.code))?;
    if context.vcs != "jj" || context.jj.is_none() {
        return Err(Error::new(
            "not_jj_workspace",
            "Native observation status requires a jj workspace.",
        ));
    }
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
    let deadline = Instant::now() + COMMAND_DEADLINE;
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
    JjObservationJournal::open_read_only_at_path(path).map_err(|error| {
        let message = match error {
            // SQLite open failures include filenames in their free-form message.
            JournalError::Persistence(PersistenceError::Sqlite {
                operation, code, ..
            }) => {
                let code = code
                    .map(|code| format!("{code:?}"))
                    .unwrap_or_else(|| "SQLite error".to_owned());
                format!("Journal {operation} failed ({code}).")
            }
            other => other.to_string(),
        };
        Error::new("journal_unavailable", message)
    })
}
