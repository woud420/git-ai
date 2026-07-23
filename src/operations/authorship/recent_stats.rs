//! Bounded waiting for authorship produced asynchronously after a recent commit.

use crate::clients::git_cli::exec_git;
use crate::config::Config;
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::git::notes_api::{read_authorship, read_authorship_from_primary_backend};
use crate::operations::git::repository::Repository;
use crate::operations::mdm::spinner::Spinner;
use crate::utils::is_interactive_terminal;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::stats::{CommitStats, stats_for_commit_stats_with_authorship};

const RECENT_COMMIT_WINDOW: Duration = Duration::from_secs(60);
const AUTHORSHIP_NOTE_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const AUTHORSHIP_NOTE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const WAIT_MESSAGE: &str = "Waiting for git-ai to process this commit";

pub(super) fn stats_for_commit(
    repo: &Repository,
    commit_sha: &str,
    ignore_patterns: &[String],
) -> Result<CommitStats, GitAiError> {
    let authorship_log = wait_for_recent_authorship(repo, commit_sha)?;
    stats_for_commit_stats_with_authorship(
        repo,
        commit_sha,
        ignore_patterns,
        authorship_log.as_ref(),
    )
}

fn wait_for_recent_authorship(
    repo: &Repository,
    commit_sha: &str,
) -> Result<Option<AuthorshipLog>, GitAiError> {
    if let Some(authorship_log) = read_authorship(repo, commit_sha) {
        return Ok(Some(authorship_log));
    }

    let config = Config::fresh();
    if !repo.is_collection_allowed(&config) {
        return Ok(None);
    }
    let notes_backend_kind = config.notes_backend_kind();

    let commit_timestamp = commit_timestamp(repo, commit_sha)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            GitAiError::Generic(format!("System clock is before Unix epoch: {error}"))
        })?
        .as_secs();
    if now.abs_diff(commit_timestamp) > RECENT_COMMIT_WINDOW.as_secs() {
        return Ok(None);
    }

    let force_tty = std::env::var_os("GIT_AI_TEST_FORCE_TTY").is_some();
    let spinner = if force_tty {
        // Indicatif intentionally hides itself when stderr is captured. Keep the
        // existing test-only TTY override useful for subprocess assertions.
        eprintln!("{WAIT_MESSAGE}");
        None
    } else {
        is_interactive_terminal().then(|| Spinner::new(WAIT_MESSAGE))
    };
    let started = Instant::now();
    let authorship_log = loop {
        let remaining = AUTHORSHIP_NOTE_WAIT_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break None;
        }
        std::thread::sleep(AUTHORSHIP_NOTE_POLL_INTERVAL.min(remaining));
        if let Some(authorship_log) =
            read_authorship_from_primary_backend(repo, commit_sha, notes_backend_kind)
        {
            break Some(authorship_log);
        }
    };
    if let Some(spinner) = spinner {
        spinner.finish_and_clear();
    }

    Ok(authorship_log)
}

fn commit_timestamp(repo: &Repository, commit_sha: &str) -> Result<u64, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.extend([
        "show".to_string(),
        "-s".to_string(),
        "--no-notes".to_string(),
        "--format=%ct".to_string(),
        commit_sha.to_string(),
    ]);
    let output = exec_git(&args)?;
    String::from_utf8(output.stdout)?
        .trim()
        .parse()
        .map_err(|error| {
            GitAiError::Generic(format!(
                "Invalid commit timestamp for {commit_sha}: {error}"
            ))
        })
}
