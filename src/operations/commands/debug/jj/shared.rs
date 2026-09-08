use super::Error;
use crate::config::Config;
use crate::model::repository::error::PersistenceError;
use crate::model::repository::jj_observation_journal::JournalError;
use crate::operations::workspace_context::{WorkspaceContext, discover};
use std::time::Duration;

pub(super) const COMMAND_DEADLINE: Duration = Duration::from_secs(5);

pub(super) fn current_context(config: &Config, action: &str) -> Result<WorkspaceContext, Error> {
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
            format!("Native observation {action} requires a jj workspace."),
        ));
    }
    Ok(context)
}

pub(super) fn journal_error(error: JournalError) -> Error {
    let message = match error {
        // Opening errors may embed filenames in their free-form cause.
        JournalError::Persistence(PersistenceError::Sqlite {
            operation, code, ..
        }) => {
            let code = code
                .map(|code| format!("{code:?}"))
                .unwrap_or_else(|| "SQLite error".to_owned());
            format!("Journal {operation} failed ({code}).")
        }
        JournalError::Persistence(PersistenceError::Io {
            operation, kind, ..
        }) => {
            format!("Journal {operation} failed ({kind:?}).")
        }
        other => other.to_string(),
    };
    Error::new("journal_unavailable", message)
}
