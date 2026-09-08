use super::batch_write::notes_add_blob_batch;
use super::ref_queries::ref_exists;
use super::{note_blob_oids_for_commits, note_blob_oids_for_commits_from_ref};
use crate::error::GitAiError;
use crate::operations::git::oid::is_full_oid;
use crate::operations::git::repository::Repository;

#[doc(hidden)]
pub fn parse_batch_check_blob_oid(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let oid = parts.first().copied().unwrap_or_default();
    if parts.len() >= 2 && parts[1] == "blob" && is_full_oid(oid) {
        Some(oid.to_string())
    } else {
        None
    }
}

/// Copy missing notes for a bounded commit set from `source_ref` into `refs/notes/ai`.
///
/// This deliberately does not merge `source_ref` wholesale. The source ref may
/// contain untrusted notes from a fork, so callers must pass the exact commits
/// whose notes are allowed to enter the local authorship ref. Existing local
/// notes win on conflicts, matching the `git notes merge -s ours` behavior used
/// for trusted tracking refs.
pub fn copy_missing_notes_for_commits_from_ref(
    repo: &Repository,
    source_ref: &str,
    commit_shas: &[String],
) -> Result<usize, GitAiError> {
    if commit_shas.is_empty() || !ref_exists(repo, source_ref) {
        return Ok(0);
    }

    let source_note_oids = note_blob_oids_for_commits_from_ref(repo, source_ref, commit_shas)?;
    if source_note_oids.is_empty() {
        return Ok(0);
    }

    let local_note_oids = note_blob_oids_for_commits(repo, commit_shas)?;
    let entries: Vec<(String, String)> = commit_shas
        .iter()
        .filter(|commit_sha| !local_note_oids.contains_key(*commit_sha))
        .filter_map(|commit_sha| {
            source_note_oids
                .get(commit_sha)
                .map(|blob_oid| (commit_sha.clone(), blob_oid.clone()))
        })
        .collect();

    let copied = entries.len();
    notes_add_blob_batch(repo, &entries)?;
    Ok(copied)
}
