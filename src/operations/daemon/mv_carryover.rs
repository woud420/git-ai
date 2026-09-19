use super::side_effect_helpers::{
    parsed_invocation_for_normalized_command, proven_literal_worktree_path,
};
use crate::error::GitAiError;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand, SemanticEvent};
use crate::operations::git::find_repository_in_path;
use crate::operations::git::oid::is_non_zero_oid;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn event(
    cmd: &NormalizedCommand,
    refs: &HashMap<String, String>,
) -> Option<SemanticEvent> {
    if cmd.exit_code != 0
        || cmd.raw_argv.is_empty()
        || !matches!(cmd.index_write, IndexWriteEvidence::Exact(_))
    {
        return None;
    }
    let worktree = cmd.worktree.as_deref()?;
    let head = refs.get("HEAD").filter(|head| is_non_zero_oid(head))?;
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if parsed.command.as_deref() != Some("mv") {
        return None;
    }
    let [separator, source, destination] = parsed.command_args.as_slice() else {
        return None;
    };
    // Git treats "." as an existing directory. Other destinations depend on
    // operation-time filesystem shape, even when written with a trailing slash.
    // A relative source forces the shared helper to prove the root via -C.
    if separator != "--" || destination != "." || Path::new(source).is_absolute() {
        return None;
    }
    let source = proven_literal_worktree_path(&parsed.global_args, worktree, source)?;
    let (_, destination) = source.rsplit_once('/')?;
    Some(SemanticEvent::WorkingLogPathMoved {
        base_commit: head.clone(),
        destination: destination.to_string(),
        source,
    })
}

pub(super) fn apply(
    worktree: &str,
    head: &str,
    source: &str,
    destination: &str,
    index_write: &IndexWriteEvidence,
) -> Result<(), GitAiError> {
    let repo = find_repository_in_path(worktree)?;
    if !repo.is_collection_allowed(&crate::config::Config::fresh()) {
        return Ok(());
    }
    // An alternate-index move can leave the original path staged in the
    // default index. Its pending evidence must stay available there.
    if !matches!(index_write, IndexWriteEvidence::Exact(path) if path == &repo.path().join("index.lock"))
        || !repo.storage.has_working_log(head)
    {
        return Ok(());
    }
    let log = repo.storage.working_log_for_base_commit(head)?;
    // INITIAL and checkpoints are separate durable files. Restrict migration
    // to a single atomic journal replacement instead of risking a partial move.
    if log.initial_file.try_exists()? {
        return Ok(());
    }
    let mut checkpoints = log.read_all_checkpoints()?;
    if !checkpoints
        .iter()
        .flat_map(|checkpoint| &checkpoint.entries)
        .any(|entry| is_within(&entry.file, source))
    {
        return Ok(());
    }
    let paths = checkpoints
        .iter()
        .flat_map(|checkpoint| checkpoint.entries.iter().map(|entry| &entry.file));
    let mut count = 0usize;
    let mut bytes = 0usize;
    for path in paths {
        count = count.saturating_add(1);
        bytes = bytes.saturating_add(path.len());
        if count > 4096 || bytes > 1024 * 1024 {
            return Ok(());
        }
    }

    for checkpoint in &mut checkpoints {
        checkpoint.entries.retain_mut(|entry| {
            if let Some(path) = moved_path(&entry.file, source, destination) {
                entry.file = path;
                true
            } else {
                false
            }
        });
    }
    // Keep checkpoint identities and immutable content snapshots. Existing
    // commit reconciliation validates their bytes against the committed tree.
    log.write_all_checkpoints(&checkpoints)
}

#[cfg(test)]
mod tests;

fn is_within(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn moved_path(path: &str, source: &str, destination: &str) -> Option<String> {
    if path == source {
        Some(destination.to_string())
    } else if let Some(suffix) = path
        .strip_prefix(source)
        .filter(|suffix| suffix.starts_with('/'))
    {
        Some(format!("{destination}{suffix}"))
    } else if path == destination
        || path
            .strip_prefix(destination)
            .is_some_and(|suffix| suffix.starts_with('/'))
    {
        // A successful non-force move proves this destination did not exist;
        // any older checkpoint under it cannot describe the moved content.
        None
    } else {
        Some(path.to_string())
    }
}
