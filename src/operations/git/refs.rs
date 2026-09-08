mod batch_read;
mod batch_write;
mod constants;
mod ref_queries;

pub use batch_read::{
    copy_missing_notes_for_commits_from_ref, note_blob_oids_for_commits_from_ref,
    parse_batch_check_blob_oid,
};
pub(in crate::operations::git) use batch_read::{note_blob_oids_for_commits, notes_for_commits};
#[cfg(feature = "test-support")]
pub(in crate::operations::git) use batch_write::notes_add_blob_batch;
pub(in crate::operations::git) use batch_write::{fast_import_args, notes_add, notes_add_batch};
pub use constants::{
    AI_AUTHORSHIP_FORK_TRACKING_REF, AI_AUTHORSHIP_FULL_REF, AI_AUTHORSHIP_PUSH_REFSPEC,
    AI_AUTHORSHIP_REFNAME,
};
use ref_queries::parse_output_error;
pub use ref_queries::ref_exists;

use crate::clients::git_cli::{exec_git, exec_git_allow_nonzero, exec_git_stdin};
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::{AUTHORSHIP_LOG_VERSION, AuthorshipLog};
use crate::model::working_log::Checkpoint;
use crate::operations::git::cat_file::batch_read_blob_contents;
use crate::operations::git::repository::Repository;
use serde_json;
use std::collections::{HashMap, HashSet};

mod note_fanout;

#[doc(hidden)]
pub use note_fanout::{
    fanout_note_pathspec_for_commit, fanout_note_pathspec_for_ref, flat_note_pathspec_for_commit,
    flat_note_pathspec_for_ref, notes_path_for_object,
};
pub(in crate::operations::git) use note_fanout::{write_blob_stanza, write_notes_commit_header};

// Check which commits from the given list have authorship notes.
// Uses git cat-file --batch-check to efficiently check multiple commits in one invocation.
// Returns a Vec of CommitAuthorship for each commit.
#[derive(Debug, Clone)]
pub enum CommitAuthorship {
    NoLog {
        sha: String,
        git_author: String,
    },
    Log {
        sha: String,
        git_author: String,
        authorship_log: AuthorshipLog,
    },
}
pub(in crate::operations::git) fn get_commits_with_notes_from_list(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<Vec<CommitAuthorship>, GitAiError> {
    if commit_shas.is_empty() {
        return Ok(Vec::new());
    }

    // Get the git authors for all commits using git rev-list
    // This approach works in both bare and normal repositories
    let mut args = repo.global_args_for_exec();
    args.push("rev-list".to_string());
    args.push("--no-walk".to_string());
    args.push("--pretty=format:%H%n%an%n%ae".to_string());
    for sha in commit_shas {
        args.push(sha.clone());
    }

    let output = exec_git(&args)?;
    let stdout =
        String::from_utf8(output.stdout).map_err(|_| parse_output_error("git rev-list"))?;

    let mut commit_authors = HashMap::new();
    let lines: Vec<&str> = stdout.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Skip commit headers (start with "commit ")
        if line.starts_with("commit ") {
            i += 1;
            if i + 2 < lines.len() {
                let sha = lines[i].to_string();
                let name = lines[i + 1].to_string();
                let email = lines[i + 2].to_string();
                let author = format!("{} <{}>", name, email);
                commit_authors.insert(sha, author);
                i += 3;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }

    let note_blob_oids = note_blob_oids_for_commits(repo, commit_shas)?;
    let mut unique_blob_oids = Vec::new();
    let mut seen_blob_oids = HashSet::new();
    for blob_oid in note_blob_oids.values() {
        if seen_blob_oids.insert(blob_oid.clone()) {
            unique_blob_oids.push(blob_oid.clone());
        }
    }
    let note_blob_contents = batch_read_blob_contents(repo, &unique_blob_oids)?;

    // Build the result Vec
    let mut result = Vec::new();
    for sha in commit_shas {
        let git_author = commit_authors
            .get(sha)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());

        if let Some(blob_oid) = note_blob_oids.get(sha)
            && let Some(content) = note_blob_contents.get(blob_oid)
            && let Ok(mut authorship_log) = AuthorshipLog::deserialize_from_string(content)
        {
            authorship_log.metadata.base_commit_sha = sha.clone();
            result.push(CommitAuthorship::Log {
                sha: sha.clone(),
                git_author,
                authorship_log,
            });
        } else {
            result.push(CommitAuthorship::NoLog {
                sha: sha.clone(),
                git_author,
            });
        }
    }

    Ok(result)
}

// Show an authorship note and return its JSON content if found, or None if it doesn't exist.
pub(in crate::operations::git) fn show_authorship_note(
    repo: &Repository,
    commit_sha: &str,
) -> Option<String> {
    let mut args = repo.global_args_for_exec();
    args.push("notes".to_string());
    args.push("--ref=ai".to_string());
    args.push("show".to_string());
    args.push(commit_sha.to_string());

    match exec_git(&args) {
        Ok(output) => String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        Err(GitAiError::GitCliError { code: Some(1), .. }) => None,
        Err(_) => None,
    }
}

/// Return the subset of `commit_shas` that currently has an authorship note.
///
/// This uses a single `git notes --ref=ai list` invocation instead of one
/// `git notes show` call per commit.
pub(in crate::operations::git) fn commits_with_authorship_notes(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashSet<String>, GitAiError> {
    Ok(note_blob_oids_for_commits(repo, commit_shas)?
        .into_keys()
        .collect())
}

// Show an authorship note and return its JSON content if found, or None if it doesn't exist.
pub(in crate::operations::git) fn get_authorship(
    repo: &Repository,
    commit_sha: &str,
) -> Option<AuthorshipLog> {
    let content = show_authorship_note(repo, commit_sha)?;
    let mut authorship_log = AuthorshipLog::deserialize_from_string(&content).ok()?;
    // Keep metadata aligned with the commit where this note is attached.
    authorship_log.metadata.base_commit_sha = commit_sha.to_string();
    Some(authorship_log)
}

#[allow(dead_code)]
pub fn get_reference_as_working_log(
    repo: &Repository,
    commit_sha: &str,
) -> Result<Vec<Checkpoint>, GitAiError> {
    let content = show_authorship_note(repo, commit_sha)
        .ok_or_else(|| GitAiError::Generic("No authorship note found".to_string()))?;
    let working_log = serde_json::from_str(&content)?;
    Ok(working_log)
}

pub(in crate::operations::git) fn get_reference_as_authorship_log_v3(
    repo: &Repository,
    commit_sha: &str,
) -> Result<AuthorshipLog, GitAiError> {
    let content = show_authorship_note(repo, commit_sha)
        .ok_or_else(|| GitAiError::Generic("No authorship note found".to_string()))?;

    // Try to deserialize as AuthorshipLog
    let mut authorship_log = match AuthorshipLog::deserialize_from_string(&content) {
        Ok(log) => log,
        Err(_) => {
            return Err(GitAiError::Generic(
                "Failed to parse authorship log".to_string(),
            ));
        }
    };

    // Check version compatibility
    if authorship_log.metadata.schema_version != AUTHORSHIP_LOG_VERSION {
        return Err(GitAiError::Generic(format!(
            "Unsupported authorship log version: {} (expected: {})",
            authorship_log.metadata.schema_version, AUTHORSHIP_LOG_VERSION
        )));
    }

    // Keep metadata aligned with the commit where this note is attached.
    authorship_log.metadata.base_commit_sha = commit_sha.to_string();

    Ok(authorship_log)
}

/// Sanitize a remote name to create a safe ref name
/// Replaces special characters with underscores to ensure valid ref names
#[doc(hidden)]
pub fn sanitize_remote_name(remote: &str) -> String {
    remote
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Generate a tracking ref name for notes from a specific remote
/// Returns a ref like "refs/notes/ai-remote/origin"
///
/// SAFETY: These tracking refs are stored under refs/notes/ai-remote/* which:
/// - Won't be pushed by `git push` (only pushes refs/heads/* by default)
/// - Won't be pushed by `git push --all` (only pushes refs/heads/*)
/// - Won't be pushed by `git push --tags` (only pushes refs/tags/*)
/// - **WILL** be pushed by `git push --mirror` (usually only used for backups, etc.)
/// - **WILL** be pushed if user explicitly specifies refs/notes/ai-remote/* (extremely rare)
pub fn tracking_ref_for_remote(remote_name: &str) -> String {
    format!("refs/notes/ai-remote/{}", sanitize_remote_name(remote_name))
}

/// Merge notes from a source ref into refs/notes/ai
/// Uses the 'ours' strategy to combine notes without data loss
pub fn merge_notes_from_ref(repo: &Repository, source_ref: &str) -> Result<(), GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("notes".to_string());
    args.push(format!("--ref={}", AI_AUTHORSHIP_REFNAME));
    args.push("merge".to_string());
    args.push("-s".to_string());
    args.push("ours".to_string());
    args.push("--quiet".to_string());
    args.push(source_ref.to_string());

    tracing::debug!("Merging notes from {} into refs/notes/ai", source_ref);
    exec_git(&args)?;
    Ok(())
}

/// Fallback merge when `git notes merge -s ours` fails (e.g., due to git assertion
/// failures on corrupted/mixed-fanout notes trees). Implements the "ours" strategy
/// using a single `git fast-import` invocation that:
///   1. Creates a merge commit with both local and source as parents
///   2. Emits all notes via `N <blob> <object>` commands (source first, then local —
///      last writer wins, so local takes precedence on conflicts = "ours" strategy)
///   3. Produces a clean notes tree with correct fanout regardless of input tree format
///
/// This is O(1) git process invocations regardless of note count, which matters on
/// large monorepos with thousands of notes.
pub fn fallback_merge_notes_ours(repo: &Repository, source_ref: &str) -> Result<(), GitAiError> {
    let local_ref = format!("refs/notes/{}", AI_AUTHORSHIP_REFNAME);

    // 1. List notes from both refs
    let source_notes = list_all_notes(repo, source_ref)?;
    let local_notes = list_all_notes(repo, &local_ref)?;

    // 2. Resolve parent commit SHAs for the merge commit
    let local_commit = rev_parse(repo, &local_ref)?;
    let source_commit = rev_parse(repo, source_ref)?;

    // Nothing to merge if both refs point to the same commit.
    if local_commit == source_commit {
        tracing::debug!("notes refs already at same commit, nothing to merge");
        return Ok(());
    }

    // 3. Build the fast-import stream.
    //    Use explicit `M` (filemodify) commands instead of `N` (notemodify) because
    //    `N` validates that the annotated object exists locally, which fails when
    //    merging notes from a remote that annotates commits not yet fetched to this
    //    repo (e.g., notes from another developer's push on a monorepo).
    //
    //    Emit source (remote) notes first, then local notes. fast-import uses
    //    last-writer-wins for duplicate paths, so local notes take precedence —
    //    this implements the "ours" merge strategy.
    let mut stream = String::new();
    stream.push_str(&format!("commit {}\n", local_ref));
    stream.push_str("committer git-ai <git-ai@noreply> 0 +0000\n");
    stream.push_str("data 23\nMerge notes (fallback)\n");
    stream.push_str(&format!("from {}\n", local_commit));
    stream.push_str(&format!("merge {}\n", source_commit));
    // Start with a clean tree to avoid mixed-fanout issues
    stream.push_str("deleteall\n");

    // Source notes first (will be overwritten by local on conflict)
    for (blob, object) in &source_notes {
        let path = notes_path_for_object(object);
        stream.push_str(&format!("M 100644 {} {}\n", blob, path));
    }
    // Local notes second (wins on conflict)
    for (blob, object) in &local_notes {
        let path = notes_path_for_object(object);
        stream.push_str(&format!("M 100644 {} {}\n", blob, path));
    }
    stream.push_str("done\n");

    // 4. Run fast-import
    let mut args = repo.global_args_for_exec();
    args.extend_from_slice(&[
        "fast-import".to_string(),
        "--quiet".to_string(),
        "--done".to_string(),
    ]);
    exec_git_stdin(&args, stream.as_bytes())?;

    tracing::debug!("fallback merge via fast-import completed successfully");
    Ok(())
}

/// List all notes on a given ref. Returns Vec<(note_blob_sha, annotated_object_sha)>.
fn list_all_notes(repo: &Repository, notes_ref: &str) -> Result<Vec<(String, String)>, GitAiError> {
    // `git notes list` uses --ref to specify which notes ref.
    // The --ref option prepends "refs/notes/" automatically, so for full refs
    // like "refs/notes/ai-remote/origin" we need to strip the prefix.
    let ref_arg = notes_ref.strip_prefix("refs/notes/").unwrap_or(notes_ref);

    let mut args = repo.global_args_for_exec();
    args.extend_from_slice(&[
        "notes".to_string(),
        format!("--ref={}", ref_arg),
        "list".to_string(),
    ]);

    let output = exec_git(&args)?;
    let stdout = String::from_utf8(output.stdout).map_err(|_| parse_output_error("notes list"))?;

    Ok(stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                None
            }
        })
        .collect())
}

/// Parse a revision to its SHA
fn rev_parse(repo: &Repository, rev: &str) -> Result<String, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.extend_from_slice(&["rev-parse".to_string(), rev.to_string()]);
    let output = exec_git(&args)?;
    String::from_utf8(output.stdout)
        .map_err(|_| parse_output_error("rev-parse"))
        .map(|s| s.trim().to_string())
}

/// Copy a ref to another location (used for initial setup of local notes from tracking ref)
pub fn copy_ref(repo: &Repository, source_ref: &str, dest_ref: &str) -> Result<(), GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("update-ref".to_string());
    args.push(dest_ref.to_string());
    args.push(source_ref.to_string());

    tracing::debug!("Copying ref {} to {}", source_ref, dest_ref);
    exec_git(&args)?;
    Ok(())
}

/// Search AI notes for a pattern and return matching commit SHAs ordered by commit date (newest first)
/// Uses git grep to search through refs/notes/ai
pub(in crate::operations::git) fn grep_ai_notes(
    repo: &Repository,
    pattern: &str,
) -> Result<Vec<String>, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("--no-pager".to_string());
    args.push("grep".to_string());
    args.push("-nI".to_string());
    args.push(pattern.to_string());
    args.push("refs/notes/ai".to_string());

    let output = exec_git(&args)?;
    let stdout = String::from_utf8(output.stdout).map_err(|_| parse_output_error("git grep"))?;

    // Parse output format: refs/notes/ai:ab/cdef123...:line_number:matched_content
    // Extract the commit SHA from the path
    let mut shas = HashSet::new();
    for line in stdout.lines() {
        if let Some(path_and_rest) = line.strip_prefix("refs/notes/ai:")
            && let Some(path_end) = path_and_rest.find(':')
        {
            let path = &path_and_rest[..path_end];
            // Path is in format "ab/cdef123..." - combine to get full SHA
            let sha = path.replace('/', "");
            shas.insert(sha);
        }
    }

    // If we have multiple results, sort by commit date (newest first)
    sort_commit_shas_by_date_desc(repo, shas)
}

/// Sort commit SHAs by commit date, newest first, using a single `git log
/// --no-walk` call. Best-effort: if git cannot resolve every SHA (e.g. a
/// cache-only note references a commit that was never fetched locally), the
/// input is returned in lexicographic order instead so callers still get a
/// deterministic result.
pub(crate) fn sort_commit_shas_by_date_desc(
    repo: &Repository,
    shas: HashSet<String>,
) -> Result<Vec<String>, GitAiError> {
    if shas.len() <= 1 {
        return Ok(shas.into_iter().collect());
    }

    let mut sha_vec: Vec<String> = shas.into_iter().collect();
    let mut args = repo.global_args_for_exec();
    args.push("log".to_string());
    args.push("--format=%H".to_string());
    args.push("--date-order".to_string());
    args.push("--no-walk".to_string());
    for sha in &sha_vec {
        args.push(sha.clone());
    }

    if let Ok(output) = exec_git_allow_nonzero(&args)
        && output.status.success()
        && let Ok(stdout) = String::from_utf8(output.stdout)
    {
        let sorted: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
        if sorted.len() == sha_vec.len() {
            return Ok(sorted);
        }
    }

    sha_vec.sort();
    Ok(sha_vec)
}

/// Direct access to the raw git-notes backend, for TESTS ONLY.
///
/// Production code must go through `crate::operations::git::notes_api`, which dispatches on
/// the configured notes backend — calling these directly silently ignores the
/// HTTP backend (notes live in the notes-db cache there, not refs/notes/ai).
/// The underlying functions are `pub(in crate::operations::git)` to enforce that; these
/// wrappers exist so backend unit tests can exercise the raw git
/// implementation regardless of backend configuration.
#[cfg(feature = "test-support")]
pub mod git_backend_for_tests {
    use super::*;

    pub fn notes_add(
        repo: &Repository,
        commit_sha: &str,
        note_content: &str,
    ) -> Result<(), GitAiError> {
        super::notes_add(repo, commit_sha, note_content)
    }

    pub fn show_authorship_note(repo: &Repository, commit_sha: &str) -> Option<String> {
        super::show_authorship_note(repo, commit_sha)
    }

    pub fn commits_with_authorship_notes(
        repo: &Repository,
        commit_shas: &[String],
    ) -> Result<HashSet<String>, GitAiError> {
        super::commits_with_authorship_notes(repo, commit_shas)
    }

    pub fn get_commits_with_notes_from_list(
        repo: &Repository,
        commit_shas: &[String],
    ) -> Result<Vec<CommitAuthorship>, GitAiError> {
        super::get_commits_with_notes_from_list(repo, commit_shas)
    }

    pub fn grep_ai_notes(repo: &Repository, pattern: &str) -> Result<Vec<String>, GitAiError> {
        super::grep_ai_notes(repo, pattern)
    }

    pub fn note_blob_oids_for_commits(
        repo: &Repository,
        commit_shas: &[String],
    ) -> Result<HashMap<String, String>, GitAiError> {
        super::note_blob_oids_for_commits(repo, commit_shas)
    }

    pub fn notes_add_batch(
        repo: &Repository,
        entries: &[(String, String)],
    ) -> Result<(), GitAiError> {
        super::notes_add_batch(repo, entries)
    }

    pub fn notes_add_blob_batch(
        repo: &Repository,
        entries: &[(String, String)],
    ) -> Result<(), GitAiError> {
        super::notes_add_blob_batch(repo, entries)
    }
}

#[path = "refs_tests.rs"]
#[cfg(test)]
mod tests;
