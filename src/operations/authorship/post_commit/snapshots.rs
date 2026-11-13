use crate::error::GitAiError;
use crate::operations::authorship::diff_base::single_commit_diff_base;
use crate::operations::commands::diff_parsing::get_diff_text;
use crate::operations::git::repository::{
    Repository, batch_read_paths_at_treeishes, parse_diff_added_lines,
};
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

/// `git diff -U0` of one commit against its single-commit base, run at most once
/// so recovery, background-agent fill and stats share one Git process.
pub(super) struct CommitDiffText<'a> {
    repo: &'a Repository,
    base: String,
    commit_sha: &'a str,
    text: Option<String>,
}

impl<'a> CommitDiffText<'a> {
    pub(super) fn new(repo: &'a Repository, parent_sha: &str, commit_sha: &'a str) -> Self {
        // Bounded to that single commit (see `single_commit_diff_base`): on the daemon
        // fast-forward path `parent_sha` can be a far-behind old branch tip.
        let base = single_commit_diff_base(parent_sha, commit_sha);
        Self {
            repo,
            base,
            commit_sha,
            text: None,
        }
    }

    pub(super) fn text(&mut self) -> Result<&str, GitAiError> {
        let text = match self.text.take() {
            Some(text) => text,
            None => get_diff_text(self.repo, &self.base, self.commit_sha, true)?,
        };
        Ok(self.text.insert(text))
    }
}

pub(super) fn recovery_committed_hunks(
    precomputed_parent_diff: Option<&crate::operations::authorship::rewrite::DiffTreeResult>,
    commit_diff: &mut CommitDiffText<'_>,
) -> Result<HashMap<String, Vec<crate::model::authorship_log::LineRange>>, GitAiError> {
    if let Some(diff) = precomputed_parent_diff {
        return Ok(
            crate::operations::authorship::virtual_attribution::committed_hunks_from_diff_result(
                diff, None,
            ),
        );
    }

    let (added_lines, _deleted_count) = parse_diff_added_lines(commit_diff.text()?)?;
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
