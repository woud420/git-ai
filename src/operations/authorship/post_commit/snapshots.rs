use crate::error::GitAiError;
use crate::operations::authorship::diff_base::single_commit_diff_base;
use crate::operations::git::repository::{Repository, batch_read_paths_at_treeishes};
use std::collections::{HashMap, HashSet};

pub(super) fn commit_tree_snapshot_for_files(
    repo: &Repository,
    commit_sha: &str,
    file_paths: &HashSet<String>,
) -> Result<HashMap<String, String>, GitAiError> {
    let requests = file_paths
        .iter()
        .map(|file_path| (commit_sha.to_string(), file_path.clone()))
        .collect::<Vec<_>>();
    let contents = batch_read_paths_at_treeishes(repo, &requests)?;
    let mut snapshot = HashMap::with_capacity(file_paths.len());
    for file_path in file_paths {
        snapshot.insert(
            file_path.clone(),
            contents
                .get(&(commit_sha.to_string(), file_path.clone()))
                .cloned()
                .unwrap_or_default(),
        );
    }

    Ok(snapshot)
}

pub(super) fn recovery_committed_hunks(
    repo: &Repository,
    parent_sha: &str,
    commit_sha: &str,
    precomputed_parent_diff: Option<&crate::operations::authorship::rewrite::DiffTreeResult>,
) -> Result<HashMap<String, Vec<crate::model::authorship_log::LineRange>>, GitAiError> {
    if let Some(diff) = precomputed_parent_diff {
        return Ok(
            crate::operations::authorship::virtual_attribution::committed_hunks_from_diff_result(
                diff, None,
            ),
        );
    }

    // Recovery only attributes lines added by the commit being finalized, so the
    // diff must be bounded to that single commit (see `single_commit_diff_base`).
    let diff_base = single_commit_diff_base(parent_sha, commit_sha);
    let added_lines = repo.diff_added_lines(&diff_base, commit_sha, None)?;
    Ok(added_lines
        .into_iter()
        .filter(|(_, lines)| !lines.is_empty())
        .map(|(path, lines)| {
            (
                path,
                crate::model::authorship_log::LineRange::compress_lines(&lines),
            )
        })
        .collect())
}
