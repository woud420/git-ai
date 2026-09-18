use std::collections::{HashMap, HashSet};

use crate::error::GitAiError;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::model::hunk_shift::apply_hunk_shifts_to_file_attestation;
use crate::model::repository::error::PersistenceError;
use crate::operations::git::notes_api;
use crate::operations::git::repository::Repository;

use super::diff_tree::compute_diff_trees_streaming;

struct PendingShift<'a> {
    new_sha: String,
    raw_note: &'a str,
}

fn serialization_error(context: &str, error: impl std::fmt::Display) -> GitAiError {
    PersistenceError::Io {
        // Preserve diagnostics stored in FamilyStatus.last_error.
        operation: "Generic error",
        path: String::new(),
        kind: std::io::ErrorKind::InvalidData,
        message: format!("failed to serialize {context} authorship log: {error}"),
    }
    .into()
}

/// Serialize `log` to a note, write it for `commit_sha`, and return the
/// serialized string (used by the metrics path to attach the note text).
pub(super) fn write_authorship_log(
    repo: &Repository,
    commit_sha: &str,
    log: &AuthorshipLog,
) -> Result<String, GitAiError> {
    let serialized = log
        .serialize_to_string()
        .map_err(|e| serialization_error("rewrite", e))?;
    let entries = vec![(commit_sha.to_string(), serialized)];
    notes_api::write_notes_batch(repo, &entries)?;
    Ok(entries
        .into_iter()
        .next()
        .map(|(_, note)| note)
        .unwrap_or_default())
}

/// Shift authorship notes from source commits to their rewritten counterparts.
/// Pre-existing non-empty notes on target commits are skipped (not replaced).
pub fn shift_authorship_notes(
    repo: &Repository,
    mappings: &[(String, String)],
) -> Result<(), GitAiError> {
    shift_authorship_notes_with_existing_mode(repo, mappings, false).map(|_| ())
}

/// Like [`shift_authorship_notes`] but merges shifted content into any
/// pre-existing non-empty note on the target commit rather than skipping it.
pub fn shift_authorship_notes_merging_existing(
    repo: &Repository,
    mappings: &[(String, String)],
) -> Result<(), GitAiError> {
    shift_authorship_notes_with_existing_mode(repo, mappings, true).map(|_| ())
}

/// Like [`shift_authorship_notes_merging_existing`] but also returns the
/// written `(commit_sha, serialized_note)` pairs for the metrics path.
pub(crate) fn shift_authorship_notes_merging_existing_with_notes(
    repo: &Repository,
    mappings: &[(String, String)],
) -> Result<Vec<(String, String)>, GitAiError> {
    shift_authorship_notes_with_existing_mode(repo, mappings, true)
}

fn shift_authorship_notes_with_existing_mode(
    repo: &Repository,
    mappings: &[(String, String)],
    merge_existing_targets: bool,
) -> Result<Vec<(String, String)>, GitAiError> {
    tracing::debug!("shift_authorship_notes: {} mappings", mappings.len());

    if mappings.is_empty() {
        return Ok(Vec::new());
    }

    // Batch-read all notes for source and target commits in O(1) git calls
    let all_shas = super::diff_tree::unique_pair_shas(mappings);
    let notes_map = notes_api::read_notes_batch(repo, &all_shas)?;

    let mut pending: Vec<PendingShift> = Vec::new();
    let mut verbatim_writes: Vec<(String, String)> = Vec::new();
    let mut diff_pairs: Vec<(String, String)> = Vec::new();
    let mut existing_by_target: HashMap<String, AuthorshipLog> = HashMap::new();

    let mut inspected_targets = HashSet::new();
    let mut blocked_targets = HashSet::new();
    for (_, new_sha) in mappings {
        if !inspected_targets.insert(new_sha) {
            continue;
        }
        if let Some(existing_raw) = notes_map.get(new_sha) {
            match AuthorshipLog::deserialize_from_string(existing_raw) {
                Ok(log) if !log.attestations.is_empty() => {
                    if merge_existing_targets {
                        existing_by_target.insert(new_sha.clone(), log);
                    } else {
                        blocked_targets.insert(new_sha);
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    blocked_targets.insert(new_sha);
                }
            }
        }
    }
    for (source_sha, new_sha) in mappings {
        if blocked_targets.contains(new_sha) {
            continue;
        }

        let Some(raw_note) = notes_map.get(source_sha) else {
            continue;
        };

        // Validate one source at a time; retain only borrowed raw text while
        // diff-tree runs, rather than every expanded authorship log.
        if AuthorshipLog::deserialize_from_string(raw_note).is_err() {
            if !merge_existing_targets {
                verbatim_writes.push((new_sha.clone(), raw_note.clone()));
            }
            continue;
        }

        diff_pairs.push((source_sha.clone(), new_sha.clone()));
        pending.push(PendingShift {
            new_sha: new_sha.clone(),
            raw_note,
        });
    }

    if pending.is_empty() && verbatim_writes.is_empty() {
        return Ok(Vec::new());
    }

    // Stream all pairs through one diff-tree process. Parsed results are
    // applied and dropped in bounded chunks while stdout is paused, which
    // back-pressures Git without adding one process spawn per chunk.
    let mut merged_by_target = existing_by_target;
    let mut pending_iter = pending.into_iter();
    compute_diff_trees_streaming(repo, &diff_pairs, |chunk| {
        apply_chunk_shifts(&mut merged_by_target, chunk, &mut pending_iter);
    })?;

    let mut all_writes = verbatim_writes;
    for (sha, log) in merged_by_target {
        let serialized = log
            .serialize_to_string()
            .map_err(|e| serialization_error("shifted", e))?;
        all_writes.push((sha, serialized));
    }

    // Single batched write for all notes
    notes_api::write_notes_batch(repo, &all_writes)?;

    Ok(all_writes)
}

fn apply_chunk_shifts<'a>(
    merged_by_target: &mut HashMap<String, AuthorshipLog>,
    chunk: Vec<super::DiffTreeResult>,
    pending_iter: &mut impl Iterator<Item = PendingShift<'a>>,
) {
    for (diff_result, shift) in chunk.into_iter().zip(pending_iter) {
        let Ok(mut log) = AuthorshipLog::deserialize_from_string(shift.raw_note) else {
            continue;
        };

        shift_authorship_log(&mut log, &diff_result);

        log.metadata.base_commit_sha = shift.new_sha.clone();

        match merged_by_target.get_mut(&shift.new_sha) {
            Some(existing) => merge_authorship_logs(existing, &log),
            None => {
                merged_by_target.insert(shift.new_sha, log);
            }
        }
    }
}

pub(crate) fn shift_authorship_log(log: &mut AuthorshipLog, diff_result: &super::DiffTreeResult) {
    for (old_path, new_path) in &diff_result.renames {
        for attestation in &mut log.attestations {
            if attestation.file_path == *old_path {
                attestation.file_path = new_path.clone();
            }
        }
    }

    if !diff_result.hunks_by_file.is_empty() {
        log.attestations = log
            .attestations
            .iter()
            .filter_map(|fa| match diff_result.hunks_by_file.get(&fa.file_path) {
                Some(hunks) => apply_hunk_shifts_to_file_attestation(fa, hunks),
                None => Some(fa.clone()),
            })
            .collect();
    }
}

pub(super) fn merge_authorship_logs(target: &mut AuthorshipLog, source: &AuthorshipLog) {
    for src_fa in &source.attestations {
        if let Some(existing_fa) = target
            .attestations
            .iter_mut()
            .find(|a| a.file_path == src_fa.file_path)
        {
            // Merge entries into existing file attestation
            for src_entry in &src_fa.entries {
                if let Some(existing_entry) = existing_fa
                    .entries
                    .iter_mut()
                    .find(|e| e.hash == src_entry.hash)
                {
                    for range in &src_entry.line_ranges {
                        if !existing_entry.line_ranges.contains(range) {
                            existing_entry.line_ranges.push(range.clone());
                        }
                    }
                } else {
                    existing_fa.entries.push(src_entry.clone());
                }
            }
        } else {
            target.attestations.push(src_fa.clone());
        }
    }
    target.metadata.merge_missing_from(&source.metadata);
}

#[cfg(test)]
mod tests;
