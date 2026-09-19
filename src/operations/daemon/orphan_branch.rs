use super::working_log_discard::remove_working_log_attributions_for_files;
use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::refs::parse_batch_check_blob_oid;
use crate::operations::git::repository::Repository;
use std::collections::BTreeSet;

const MAX_PATHS: usize = 4096;
const MAX_QUERY_BYTES: usize = 1024 * 1024;

pub(super) fn migrate_working_log(
    worktree: &str,
    old_head: &str,
    discard_tracked: bool,
) -> Result<(), GitAiError> {
    let repo = find_repository_in_path(worktree)?;
    if discard_tracked {
        let Some(paths) = tracked_checkpoint_paths(&repo, old_head)? else {
            tracing::warn!(
                "orphan switch attribution skipped: incomplete or over-budget path evidence"
            );
            return Ok(());
        };
        remove_working_log_attributions_for_files(&repo, old_head, &paths)?;
    }
    repo.storage.rename_working_log(old_head, "initial")
}

fn tracked_checkpoint_paths(
    repo: &Repository,
    head: &str,
) -> Result<Option<Vec<String>>, GitAiError> {
    if !repo.storage.has_working_log(head) {
        return Ok(Some(Vec::new()));
    }
    let log = repo.storage.working_log_for_base_commit(head)?;
    let initial = log.read_initial_attributions();
    let checkpoints = log.read_all_checkpoints()?;
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
        return Ok(Some(Vec::new()));
    }
    if paths.len() > MAX_PATHS
        || paths.iter().any(|path| path.contains(['\n', '\r', '\0']))
        || head.len()
            + 1
            + paths
                .iter()
                .map(|path| head.len() + path.len() + 2)
                .sum::<usize>()
            > MAX_QUERY_BYTES
    {
        return Ok(None);
    }
    // Resolve only recorded paths in the ordered old HEAD, never the later
    // index/worktree. One metadata-only batch also bounds process overhead.
    let mut args = repo.global_args_for_exec();
    args.extend([
        "--no-replace-objects".into(),
        "cat-file".into(),
        "--batch-check=%(objectname) %(objecttype)".into(),
    ]);
    let mut input = format!("{head}\n");
    input.extend(paths.iter().map(|path| format!("{head}:{path}\n")));
    let output = exec_git_stdin(&args, input.as_bytes())?;
    let stdout = String::from_utf8(output.stdout)?;
    let mut lines = stdout.lines();
    let Some((oid, kind)) = lines.next().and_then(|line| line.split_once(' ')) else {
        return Ok(None);
    };
    // Missing paths alone cannot distinguish an absent file from a pruned
    // source commit; require the old commit in the same immutable-object batch.
    if !oid.eq_ignore_ascii_case(head) || kind != "commit" {
        return Ok(None);
    }
    let records: Vec<_> = lines.collect();
    if records.len() != paths.len() {
        return Ok(None);
    }
    let mut tracked = Vec::new();
    for (path, record) in paths.into_iter().zip(records) {
        if parse_batch_check_blob_oid(record).is_some() {
            tracked.push(path.clone());
        } else if record != format!("{head}:{path} missing") {
            return Ok(None);
        }
    }
    Ok(Some(tracked))
}
