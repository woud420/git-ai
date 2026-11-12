use crate::cli::fail::{print_stdout, resolve_repo_or_fail};
use crate::error::GitAiError;
use crate::operations::commands::revision::normalize_head_rev;
use crate::operations::git::notes_api::{CommitAuthorship, filter_commits_with_notes};
use crate::operations::git::repository::{CommitRange, Repository};

const NO_AUTHORSHIP_DATA_MESSAGE: &str = "No authorship data found for this revision";

pub fn handle_show(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: show requires a revision or range");
        std::process::exit(1);
    }

    if args.len() > 1 {
        eprintln!("Error: show accepts exactly one revision or range");
        std::process::exit(1);
    }

    let repo = resolve_repo_or_fail();

    if let Err(e) = show_authorship(&repo, &args[0]) {
        eprintln!("Failed to show authorship: {}", e);
        std::process::exit(1);
    }
}

fn show_authorship(repo: &Repository, spec: &str) -> Result<(), GitAiError> {
    let commits = resolve_commits(repo, spec)?;
    if commits.is_empty() {
        print_stdout(&format!("{NO_AUTHORSHIP_DATA_MESSAGE}\n"));
        return Ok(());
    }

    let entries = filter_commits_with_notes(repo, &commits)?;

    let multiple_commits = entries.len() > 1;
    let mut out = String::new();
    for (index, entry) in entries.iter().enumerate() {
        if multiple_commits && index > 0 {
            out.push('\n');
        }

        let (sha, body) = match entry {
            CommitAuthorship::Log {
                sha,
                authorship_log,
                ..
            } => (
                sha,
                authorship_log.serialize_to_string().map_err(|_| {
                    GitAiError::Generic("Failed to serialize authorship log".to_string())
                })?,
            ),
            CommitAuthorship::NoLog { sha, .. } => (sha, NO_AUTHORSHIP_DATA_MESSAGE.to_string()),
        };
        if multiple_commits {
            out.push_str(&format!("{sha}\n"));
        }
        out.push_str(&format!("{body}\n"));
    }
    print_stdout(&out);

    Ok(())
}

fn resolve_commits(repo: &Repository, spec: &str) -> Result<Vec<String>, GitAiError> {
    if let Some((start, end)) = spec.split_once("..") {
        if start.is_empty() || end.is_empty() {
            return Err(GitAiError::Generic(
                "Invalid commit range format. Expected <start>..<end>".to_string(),
            ));
        }

        let start = normalize_head_rev(start);
        let end = normalize_head_rev(end);
        let range = CommitRange::new_infer_refname(repo, start, end.clone(), None)?;

        let mut commits: Vec<String> = range.into_iter().map(|commit| commit.id()).collect();

        if commits.is_empty() {
            commits.push(resolve_commit(repo, &end)?);
        }

        Ok(commits)
    } else {
        Ok(vec![resolve_commit(repo, &normalize_head_rev(spec))?])
    }
}

/// Peel in the same rev-parse so an annotated tag yields its commit, which is
/// what notes are keyed by.
pub(crate) fn resolve_commit(repo: &Repository, rev: &str) -> Result<String, GitAiError> {
    Ok(repo.revparse_single(&format!("{rev}^{{commit}}"))?.id())
}
