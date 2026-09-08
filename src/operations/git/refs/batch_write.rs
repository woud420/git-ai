use super::constants::AI_AUTHORSHIP_FULL_REF;
use super::note_fanout::{write_blob_stanza, write_note_entry, write_notes_commit_header};
use crate::clients::git_cli::{exec_git, exec_git_with_stdin_writer};
use crate::error::GitAiError;
use crate::operations::git::cat_file::batch_read_blob_contents;
use crate::operations::git::repository::Repository;
use std::collections::HashSet;

pub(in crate::operations::git) fn notes_add(
    repo: &Repository,
    commit_sha: &str,
    note_content: &str,
) -> Result<(), GitAiError> {
    // Route through notes_add_batch to ensure consistent fanout tree format.
    // Using git's native `notes add` can produce flat entries for small trees,
    // leading to mixed-fanout trees that trigger assertion failures in
    // `git notes merge` (notes-merge.c diff_tree_remote).
    notes_add_batch(repo, &[(commit_sha.to_string(), note_content.to_string())])
}

/// Shared prologue for `notes_add_batch`/`notes_add_blob_batch`: resolve the
/// current `refs/notes/ai` tip (if any), collapse `entries` to one
/// `(commit_sha, value)` pair per commit (last write wins, original relative
/// order preserved), and stamp the fast-import commit time.
///
/// Returns `None` when `entries` is empty — callers should return `Ok(())`.
/// `(existing_notes_tip, deduped_entries, commit_timestamp)` from
/// [`prepare_notes_batch_write`]; `None` when there is nothing to write.
/// Entries are borrowed so large note batches are never copied.
type PreparedNotesBatch<'a> = Option<(Option<String>, Vec<&'a (String, String)>, u64)>;

fn prepare_notes_batch_write<'a>(
    repo: &Repository,
    entries: &'a [(String, String)],
) -> Result<PreparedNotesBatch<'a>, GitAiError> {
    if entries.is_empty() {
        return Ok(None);
    }

    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push("--verify".to_string());
    args.push("refs/notes/ai".to_string());
    let existing_notes_tip = match exec_git(&args) {
        Ok(output) => Some(String::from_utf8(output.stdout)?.trim().to_string()),
        Err(GitAiError::GitCliError {
            code: Some(128), ..
        })
        | Err(GitAiError::GitCliError { code: Some(1), .. }) => None,
        Err(e) => return Err(e),
    };

    let mut deduped_entries: Vec<&(String, String)> = Vec::with_capacity(entries.len());
    let mut seen = HashSet::with_capacity(entries.len());
    for entry in entries.iter().rev() {
        if seen.insert(entry.0.as_str()) {
            deduped_entries.push(entry);
        }
    }
    deduped_entries.reverse();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| GitAiError::Generic(format!("System clock before epoch: {}", e)))?
        .as_secs();

    Ok(Some((existing_notes_tip, deduped_entries, now)))
}

pub(in crate::operations::git) fn fast_import_args(repo: &Repository) -> Vec<String> {
    let mut args = repo.global_args_for_exec();
    args.push("fast-import".to_string());
    args.push("--quiet".to_string());
    args
}

pub(in crate::operations::git) fn notes_add_batch(
    repo: &Repository,
    entries: &[(String, String)],
) -> Result<(), GitAiError> {
    let Some((existing_notes_tip, deduped_entries, now)) =
        prepare_notes_batch_write(repo, entries)?
    else {
        return Ok(());
    };

    exec_git_with_stdin_writer(&fast_import_args(repo), |writer| {
        for (idx, (_commit_sha, note_content)) in deduped_entries.iter().copied().enumerate() {
            write_blob_stanza(writer, idx + 1, note_content)?;
        }

        write_notes_commit_header(
            writer,
            AI_AUTHORSHIP_FULL_REF,
            format_args!("git-ai <git-ai@local> {now} +0000"),
            existing_notes_tip.as_deref(),
        )?;

        for (idx, (commit_sha, _note_content)) in deduped_entries.iter().copied().enumerate() {
            write_note_entry(writer, commit_sha, format_args!(":{}", idx + 1))?;
        }
        writer.write_all(b"\n")
    })?;
    crate::operations::authorship::git_ai_hooks::post_notes_updated_refs(
        repo,
        deduped_entries
            .iter()
            .map(|(commit_sha, note_content)| (commit_sha.as_str(), note_content.as_str())),
    );

    Ok(())
}

/// Batch-attach existing note blobs to commits without rewriting blob contents.
///
/// Each entry is (commit_sha, existing_note_blob_oid).
pub(in crate::operations::git) fn notes_add_blob_batch(
    repo: &Repository,
    entries: &[(String, String)],
) -> Result<(), GitAiError> {
    let Some((existing_notes_tip, deduped_entries, now)) =
        prepare_notes_batch_write(repo, entries)?
    else {
        return Ok(());
    };

    exec_git_with_stdin_writer(&fast_import_args(repo), |writer| {
        write_notes_commit_header(
            writer,
            AI_AUTHORSHIP_FULL_REF,
            format_args!("git-ai <git-ai@local> {now} +0000"),
            existing_notes_tip.as_deref(),
        )?;

        for (commit_sha, blob_oid) in deduped_entries.iter().copied() {
            write_note_entry(writer, commit_sha, format_args!("{blob_oid}"))?;
        }
        writer.write_all(b"\n")
    })?;

    let has_post_notes_updated_hooks = crate::config::Config::get()
        .git_ai_hook_commands("post_notes_updated")
        .is_some_and(|commands| !commands.is_empty());
    if has_post_notes_updated_hooks {
        let hook_entries = (|| -> Result<Vec<(String, String)>, GitAiError> {
            let mut unique_blob_oids: Vec<String> = deduped_entries
                .iter()
                .map(|(_commit_sha, blob_oid)| blob_oid.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            unique_blob_oids.sort();
            let blob_contents = batch_read_blob_contents(repo, &unique_blob_oids)?;

            Ok(deduped_entries
                .iter()
                .filter_map(|(commit_sha, blob_oid)| {
                    blob_contents
                        .get(blob_oid)
                        .map(|note_content| (commit_sha.clone(), note_content.clone()))
                })
                .collect())
        })();
        match hook_entries {
            Ok(entries) if !entries.is_empty() => {
                crate::operations::authorship::git_ai_hooks::post_notes_updated(repo, &entries)
            }
            Ok(_) => {}
            Err(e) => tracing::debug!(target: "git_ai::operations::git::refs",
                "Failed to prepare post_notes_updated payload for notes_add_blob_batch: {}",
                e
            ),
        }
    }

    Ok(())
}
