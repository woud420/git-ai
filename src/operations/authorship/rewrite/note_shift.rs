use std::collections::HashMap;

use crate::error::GitAiError;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::model::hunk_shift::apply_hunk_shifts_to_file_attestation;
use crate::operations::git::notes_api;
use crate::operations::git::repository::Repository;

use super::diff_tree::compute_diff_trees_batch;

/// Serialize `log` to a note, write it for `commit_sha`, and return the
/// serialized string (used by the metrics path to attach the note text).
pub(super) fn write_authorship_log(
    repo: &Repository,
    commit_sha: &str,
    log: &AuthorshipLog,
) -> Result<String, GitAiError> {
    let serialized = log.serialize_to_string().map_err(|e| {
        GitAiError::Generic(format!("failed to serialize rewrite authorship log: {}", e))
    })?;
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
    let all_shas: Vec<String> = mappings
        .iter()
        .flat_map(|(src, dst)| [src.clone(), dst.clone()])
        .collect();
    let notes_map = notes_api::read_notes_batch(repo, &all_shas)?;

    // Determine which mappings need processing
    struct PendingShift {
        new_sha: String,
        log: AuthorshipLog,
        diff_pair_idx: usize,
    }

    let mut pending: Vec<PendingShift> = Vec::new();
    let mut verbatim_writes: Vec<(String, String)> = Vec::new();
    let mut diff_pairs: Vec<(String, String)> = Vec::new();
    let mut existing_by_target: HashMap<String, AuthorshipLog> = HashMap::new();

    for (source_sha, new_sha) in mappings {
        if let Some(existing_raw) = notes_map.get(new_sha) {
            if let Ok(existing_log) = AuthorshipLog::deserialize_from_string(existing_raw) {
                if !existing_log.attestations.is_empty() {
                    if merge_existing_targets {
                        existing_by_target
                            .entry(new_sha.clone())
                            .or_insert(existing_log);
                    } else {
                        continue;
                    }
                }
            } else {
                continue;
            }
        }

        let Some(raw_note) = notes_map.get(source_sha) else {
            continue;
        };

        let Ok(log) = AuthorshipLog::deserialize_from_string(raw_note) else {
            if !merge_existing_targets {
                verbatim_writes.push((new_sha.clone(), raw_note.clone()));
            }
            continue;
        };

        let diff_pair_idx = diff_pairs.len();
        diff_pairs.push((source_sha.clone(), new_sha.clone()));
        pending.push(PendingShift {
            new_sha: new_sha.clone(),
            log,
            diff_pair_idx,
        });
    }

    if pending.is_empty() && verbatim_writes.is_empty() {
        return Ok(Vec::new());
    }

    // Single batched diff-tree call for all pairs
    let diff_results = if !diff_pairs.is_empty() {
        compute_diff_trees_batch(repo, &diff_pairs)?
    } else {
        Vec::new()
    };

    // Apply shifts and merge logs that share a target commit
    let mut merged_by_target = existing_by_target;

    for shift in pending {
        let diff_result = &diff_results[shift.diff_pair_idx];
        let mut log = shift.log;

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

        log.metadata.base_commit_sha = shift.new_sha.clone();

        match merged_by_target.get_mut(&shift.new_sha) {
            Some(existing) => merge_authorship_logs(existing, &log),
            None => {
                merged_by_target.insert(shift.new_sha, log);
            }
        }
    }

    let mut all_writes = verbatim_writes;
    for (sha, log) in merged_by_target {
        let serialized = log.serialize_to_string().map_err(|e| {
            GitAiError::Generic(format!("failed to serialize shifted authorship log: {}", e))
        })?;
        all_writes.push((sha, serialized));
    }

    // Single batched write for all notes
    notes_api::write_notes_batch(repo, &all_writes)?;

    Ok(all_writes)
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
    // Merge all metadata maps
    for (key, record) in &source.metadata.prompts {
        target
            .metadata
            .prompts
            .entry(key.clone())
            .or_insert_with(|| record.clone());
    }
    for (key, record) in &source.metadata.sessions {
        target
            .metadata
            .sessions
            .entry(key.clone())
            .or_insert_with(|| record.clone());
    }
    for (key, record) in &source.metadata.humans {
        target
            .metadata
            .humans
            .entry(key.clone())
            .or_insert_with(|| record.clone());
    }
}
