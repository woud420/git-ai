use super::Error;
use crate::config::Config;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, JournalError};
use crate::operations::workspace_context::{WorkspaceContext, discover};
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) const COMMAND_DEADLINE: Duration = Duration::from_secs(5);
pub(super) const WRITE_SQL_READ_BYTES: usize = 48 * 1024 * 1024;

pub(super) fn require_opt_in(config: &Config) -> Result<(), Error> {
    if !config.has_allowed_repositories() {
        return Err(Error::new(
            "collection_disabled",
            "Repository collection is not allowed.",
        ));
    }
    Ok(())
}

pub(super) fn current_context(config: &Config, action: &str) -> Result<WorkspaceContext, Error> {
    require_opt_in(config)?;
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
    let message = error.diagnostic();
    Error::new("journal_unavailable", message)
}

pub(super) fn open_writable_journal(
    path: &Path,
    deadline: Instant,
) -> Result<JjObservationJournal, Error> {
    ordinary_journal_path(path)?;
    check_deadline(deadline)?;
    let journal = JjObservationJournal::open_at_path(path).map_err(journal_error)?;
    check_deadline(deadline)?;
    Ok(journal)
}

pub(super) fn ordinary_journal_path(path: &Path) -> Result<(), Error> {
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
