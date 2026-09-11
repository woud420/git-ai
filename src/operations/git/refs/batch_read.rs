use super::AI_AUTHORSHIP_FULL_REF;
use super::batch_write::notes_add_blob_batch;
use super::note_fanout::normalize_note_path;
use super::ref_exists;
use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;
use crate::operations::git::cat_file::batch_read_blob_contents;
use crate::operations::git::oid::is_full_oid;
use crate::operations::git::repository::Repository;
use std::collections::{HashMap, HashSet};

fn unexpected_note_object_type(entry: &LsTreeNoteEntry, notes_ref: &str) -> GitAiError {
    GitAiError::Generic(format!(
        "authorship note path {} in {} is {}, expected blob",
        entry.path, notes_ref, entry.object_type
    ))
}

fn malformed_ls_tree_output_error(missing: &str) -> GitAiError {
    GitAiError::Generic(format!("Malformed ls-tree output: missing {}", missing))
}

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

/// Resolve authorship note blob OIDs for a set of commits using one batched cat-file call.
///
/// Returns a map of commit SHA -> note blob SHA for commits that currently have notes.
pub(in crate::operations::git) fn note_blob_oids_for_commits(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    note_blob_oids_for_commits_from_ref(repo, AI_AUTHORSHIP_FULL_REF, commit_shas)
}

/// Read authorship note contents for a set of commits in batch.
///
/// Returns a map of commit SHA -> raw note content for commits that currently
/// have notes in `refs/notes/ai`.
pub(in crate::operations::git) fn notes_for_commits(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    if commit_shas.is_empty() {
        return Ok(HashMap::new());
    }

    let note_blob_oids = note_blob_oids_for_commits(repo, commit_shas)?;
    if note_blob_oids.is_empty() {
        return Ok(HashMap::new());
    }

    let unique_blob_oids: Vec<String> = note_blob_oids
        .values()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let blob_contents = batch_read_blob_contents(repo, &unique_blob_oids)?;

    Ok(note_blob_oids
        .into_iter()
        .filter_map(|(commit_sha, blob_oid)| {
            blob_contents
                .get(&blob_oid)
                .map(|content| (commit_sha, content.clone()))
        })
        .collect())
}

/// Resolve authorship note blob OIDs for a set of commits from a specific notes ref.
///
/// Returns a map of commit SHA -> note blob SHA for commits that have notes on
/// `notes_ref`. The destination `refs/notes/ai` is not consulted.
pub fn note_blob_oids_for_commits_from_ref(
    repo: &Repository,
    notes_ref: &str,
    commit_shas: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    if commit_shas.is_empty() {
        return Ok(HashMap::new());
    }

    let mut notes_by_commit = HashMap::with_capacity(commit_shas.len());
    let mut fanout_prefixes = HashSet::with_capacity(commit_shas.len().min(256));
    for commit_sha in commit_shas {
        notes_by_commit.insert(commit_sha.as_str(), None);
        if commit_sha.len() > 2 {
            fanout_prefixes.insert(commit_sha[..2].to_string());
        }
    }

    let Some(root_entries) = ls_tree_note_entries(repo, notes_ref, false, &[])? else {
        return Ok(HashMap::new());
    };

    record_matching_note_entries(root_entries, notes_ref, &mut notes_by_commit)?;

    let mut prefixes = fanout_prefixes.into_iter().collect::<Vec<_>>();
    prefixes.sort();
    if let Some(entries) = ls_tree_note_entries(repo, notes_ref, true, &prefixes)? {
        record_matching_note_entries(entries, notes_ref, &mut notes_by_commit)?;
    }

    Ok(notes_by_commit
        .into_iter()
        .filter_map(|(commit_sha, note)| {
            note.map(|(_preference, blob_oid)| (commit_sha.to_string(), blob_oid))
        })
        .collect())
}

#[derive(Debug)]
struct LsTreeNoteEntry {
    object_type: String,
    oid: String,
    path: String,
}

fn record_matching_note_entries(
    entries: Vec<LsTreeNoteEntry>,
    notes_ref: &str,
    notes_by_commit: &mut HashMap<&str, Option<(usize, String)>>,
) -> Result<(), GitAiError> {
    for mut entry in entries {
        let Some(preference) = normalize_note_path(&mut entry.path) else {
            continue;
        };
        let Some(current_note) = notes_by_commit.get_mut(entry.path.as_str()) else {
            continue;
        };
        if entry.object_type != "blob" {
            return Err(unexpected_note_object_type(&entry, notes_ref));
        }
        if current_note
            .as_ref()
            .is_none_or(|(current_preference, _)| preference < *current_preference)
        {
            *current_note = Some((preference, entry.oid));
        }
    }
    Ok(())
}

fn ls_tree_note_entries(
    repo: &Repository,
    notes_ref: &str,
    recursive: bool,
    pathspecs: &[String],
) -> Result<Option<Vec<LsTreeNoteEntry>>, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("ls-tree".to_string());
    args.push("-z".to_string());
    args.push("--full-tree".to_string());
    if recursive {
        args.push("-r".to_string());
    }
    args.push(notes_ref.to_string());
    if !pathspecs.is_empty() {
        args.push("--".to_string());
        args.extend(pathspecs.iter().cloned());
    }

    let output = match exec_git(&args) {
        Ok(output) => output,
        Err(GitAiError::GitCliError {
            code: Some(128),
            stderr,
            ..
        }) if stderr.contains("Not a valid object name") && stderr.contains(notes_ref) => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    parse_ls_tree_note_entries(&output.stdout).map(Some)
}

fn parse_ls_tree_note_entries(data: &[u8]) -> Result<Vec<LsTreeNoteEntry>, GitAiError> {
    let mut entries = Vec::new();
    for raw in data.split(|byte| *byte == 0).filter(|raw| !raw.is_empty()) {
        let Some(tab_idx) = raw.iter().position(|byte| *byte == b'\t') else {
            return Err(malformed_ls_tree_output_error("path separator"));
        };
        let meta = std::str::from_utf8(&raw[..tab_idx])?;
        let path = std::str::from_utf8(&raw[tab_idx + 1..])?.to_string();
        let mut parts = meta.split_whitespace();
        let Some(_mode) = parts.next() else {
            return Err(malformed_ls_tree_output_error("mode"));
        };
        let Some(object_type) = parts.next() else {
            return Err(malformed_ls_tree_output_error("object type"));
        };
        let Some(oid) = parts.next() else {
            return Err(malformed_ls_tree_output_error("object id"));
        };
        entries.push(LsTreeNoteEntry {
            object_type: object_type.to_string(),
            oid: oid.to_string(),
            path,
        });
    }
    Ok(entries)
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
