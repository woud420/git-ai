use super::working_log_discard::{
    batch_check_objects, path_queries, recorded_paths, remove_working_log_attributions_for_files,
};
use crate::error::GitAiError;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::refs::parse_batch_check_blob_oid;
use crate::operations::git::repository::Repository;

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
    let paths = recorded_paths(repo, head)?;
    if paths.is_empty() {
        return Ok(Some(Vec::new()));
    }
    // Resolve only recorded paths in the ordered old HEAD, never the later
    // index/worktree. One metadata-only batch also bounds process overhead.
    let Some(queries) = path_queries(format!("{head}\n"), head, &paths) else {
        return Ok(None);
    };
    let stdout = batch_check_objects(repo, &queries)?;
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
            tracked.push(path);
        } else if record != format!("{head}:{path} missing") {
            return Ok(None);
        }
    }
    Ok(Some(tracked))
}
