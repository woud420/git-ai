//! Per-commit "Git AI stats" for `git-ai log`.

use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::authorship::diff_base::empty_tree_for;
use crate::operations::authorship::ignore::{
    build_ignore_matcher, should_ignore_file_with_matcher,
};
use crate::operations::authorship::post_commit::{
    StatsCostEstimate, count_line_ranges, should_skip_expensive_post_commit_stats,
};
use crate::operations::authorship::stats::{
    stats_for_commit_stats_from_hunks_with_merge_flag, write_stats_to_terminal,
};
use crate::operations::commands::diff::{DiffHunk, get_diff_with_line_numbers};
use crate::operations::git::repository::Repository;
use std::collections::HashMap;

pub(super) fn render_stats(
    repo: &Repository,
    commit_sha: &str,
    parents: &[String],
    authorship_log: Option<&AuthorshipLog>,
    ignore_patterns: &[String],
) -> Result<String, String> {
    if parents.len() > 1 {
        return Err("stats skipped for merge commit".to_string());
    }

    let parent_sha = parents
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| empty_tree_for(commit_sha));

    // One diff per commit feeds both the size guard and the stats.
    let hunks = get_diff_with_line_numbers(repo, parent_sha, commit_sha)
        .map_err(|e| format!("stats unavailable: {}", e))?;
    if should_skip_expensive_post_commit_stats(&stats_cost_estimate(&hunks, ignore_patterns)) {
        return Err(format!(
            "stats skipped for large commit; run `git-ai stats {}` to compute on demand",
            commit_sha
        ));
    }

    let stats = stats_for_commit_stats_from_hunks_with_merge_flag(
        ignore_patterns,
        &hunks,
        authorship_log,
        false,
    );
    Ok(write_stats_to_terminal(&stats, false))
}

/// The post-commit cost estimate, computed from already-parsed hunks: added
/// lines of non-ignored files, and deleted lines of every file.
fn stats_cost_estimate(hunks: &[DiffHunk], ignore_patterns: &[String]) -> StatsCostEstimate {
    let matcher = build_ignore_matcher(ignore_patterns);
    let mut added_by_file: HashMap<&str, Vec<u32>> = HashMap::new();
    for hunk in hunks {
        if !hunk.added_lines.is_empty()
            && !should_ignore_file_with_matcher(&hunk.file_path, &matcher)
        {
            added_by_file
                .entry(&hunk.file_path)
                .or_default()
                .extend(&hunk.added_lines);
        }
    }
    StatsCostEstimate {
        files_with_additions: added_by_file.len(),
        added_lines: added_by_file.values().map(Vec::len).sum(),
        hunk_ranges: added_by_file
            .values()
            .map(|lines| count_line_ranges(lines))
            .sum(),
        deleted_lines: hunks.iter().map(|hunk| hunk.deleted_lines.len()).sum(),
    }
}
