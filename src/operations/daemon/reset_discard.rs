use super::git_op_side_effects::remove_working_log_attributions_for_pathspecs;
use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand};
use crate::operations::git::refs::parse_batch_check_blob_oid;
use crate::operations::git::repository::Repository;
use std::collections::BTreeSet;

const MAX_PATHS: usize = 4096;
const MAX_QUERY_BYTES: usize = 1024 * 1024;

pub(super) fn discard_same_head_tracked_paths(
    repo: &Repository,
    head: &str,
    cmd: &NormalizedCommand,
) -> Result<(), GitAiError> {
    if !repo.is_collection_allowed(&crate::config::Config::fresh()) {
        return Ok(());
    }
    // Version 2 cannot encode skip-worktree entries. Later index reads cannot
    // establish whether the reset actually discarded a sparse working edit.
    if !cmd.index_v2
        || !matches!(&cmd.index_write, IndexWriteEvidence::Exact(path) if path == &repo.path().join("index.lock"))
        || !repo.storage.has_working_log(head)
    {
        return Ok(());
    }
    let working_log = repo.storage.working_log_for_base_commit(head)?;
    let initial = working_log.read_initial_attributions();
    let checkpoints = working_log.read_all_checkpoints()?;
    let paths: BTreeSet<_> = initial
        .files
        .keys()
        .chain(
            checkpoints
                .iter()
                .flat_map(|checkpoint| checkpoint.entries.iter().map(|entry| &entry.file)),
        )
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() > MAX_PATHS
        || paths.iter().any(|path| path.contains(['\n', '\r', '\0']))
        || paths
            .iter()
            .map(|path| head.len() + path.len() + 2)
            .sum::<usize>()
            > MAX_QUERY_BYTES
    {
        tracing::warn!(
            head,
            "same-HEAD reset attribution skipped: path query budget or encoding"
        );
        return Ok(());
    }

    // The index/worktree may already contain later edits. Resolve only recorded
    // paths in the command's immutable tree, without reading blob contents.
    let mut args = repo.global_args_for_exec();
    args.extend([
        "--no-replace-objects".to_string(),
        "cat-file".to_string(),
        "--batch-check=%(objectname) %(objecttype)".to_string(),
    ]);
    let input = paths
        .iter()
        .map(|path| format!("{head}:{path}\n"))
        .collect::<String>();
    let output = exec_git_stdin(&args, input.as_bytes())?;
    let stdout = String::from_utf8(output.stdout)?;
    let records: Vec<_> = stdout.lines().collect();
    if records.len() != paths.len() {
        tracing::warn!(
            head,
            "same-HEAD reset attribution skipped: incomplete path evidence"
        );
        return Ok(());
    }
    let tracked: Vec<_> = paths
        .into_iter()
        .zip(records)
        .filter(|(_, record)| parse_batch_check_blob_oid(record).is_some())
        .map(|(path, _)| path.clone())
        .collect();
    if !tracked.is_empty() {
        // The family sequencer applies this before later checkpoint writes;
        // retaining other paths preserves untracked files that Git left intact.
        remove_working_log_attributions_for_pathspecs(repo, head, &tracked)?;
    }
    Ok(())
}
