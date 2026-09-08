use crate::error::GitAiError;
use crate::operations::authorship::diff_base::single_commit_diff_base;
use crate::operations::git::repository::Repository;
use std::collections::HashMap;

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
