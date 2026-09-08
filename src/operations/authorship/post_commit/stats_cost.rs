use crate::error::GitAiError;
use crate::operations::authorship::ignore::{
    build_ignore_matcher, should_ignore_file_with_matcher,
};
use crate::operations::git::repository::Repository;

/// Skip expensive post-commit stats when this threshold is exceeded.
/// High hunk density is the strongest predictor of slow diff_ai_accepted_stats.
#[doc(hidden)]
pub const STATS_SKIP_MAX_HUNKS: usize = 1000;
/// Skip expensive stats for very large net additions even if hunks are moderate.
#[doc(hidden)]
pub const STATS_SKIP_MAX_ADDED_LINES: usize = 6000;
/// Skip expensive stats for extremely wide commits touching many added-line files.
#[doc(hidden)]
pub const STATS_SKIP_MAX_FILES_WITH_ADDITIONS: usize = 200;
/// Skip expensive stats for commits that delete a large number of lines.
/// Deletion-heavy commits (e.g. removing many files) trigger the same expensive
/// diff-parsing path as large addition commits, but the added-lines estimate is
/// near zero, so the cost was previously invisible to the estimator.
#[doc(hidden)]
pub const STATS_SKIP_MAX_DELETED_LINES: usize = 6000;

#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct StatsCostEstimate {
    pub files_with_additions: usize,
    pub added_lines: usize,
    pub hunk_ranges: usize,
    pub deleted_lines: usize,
}

#[doc(hidden)]
pub fn should_skip_expensive_post_commit_stats(estimate: &StatsCostEstimate) -> bool {
    estimate.hunk_ranges >= STATS_SKIP_MAX_HUNKS
        || estimate.added_lines >= STATS_SKIP_MAX_ADDED_LINES
        || estimate.files_with_additions >= STATS_SKIP_MAX_FILES_WITH_ADDITIONS
        || estimate.deleted_lines >= STATS_SKIP_MAX_DELETED_LINES
}

/// Public result of the stats cost estimate for a commit, used by the async
/// wrapper path to decide whether to skip expensive stats computation.
pub struct StatsSkipEstimate {
    should_skip: bool,
}

impl StatsSkipEstimate {
    pub fn should_skip(&self) -> bool {
        self.should_skip
    }
}

/// Estimate whether stats computation for `commit_sha` would be too expensive.
/// Resolves the parent commit automatically. Intended for callers outside the
/// normal post-commit flow (e.g. the async wrapper path).
pub fn estimate_stats_cost_for_head(
    repo: &Repository,
    commit_sha: &str,
    ignore_patterns: &[String],
) -> Result<StatsSkipEstimate, GitAiError> {
    let commit = repo.find_commit(commit_sha.to_string())?;
    let parent_sha = if commit.parent_count().unwrap_or(0) > 0 {
        commit
            .parent(0)
            .map(|p| p.id())
            .unwrap_or_else(|_| "initial".to_string())
    } else {
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()
    };
    estimate_stats_cost_for_commit_range(repo, &parent_sha, commit_sha, ignore_patterns)
}

pub fn estimate_stats_cost_for_commit_range(
    repo: &Repository,
    parent_sha: &str,
    commit_sha: &str,
    ignore_patterns: &[String],
) -> Result<StatsSkipEstimate, GitAiError> {
    let estimate = estimate_stats_cost(repo, parent_sha, commit_sha, ignore_patterns)?;
    Ok(StatsSkipEstimate {
        should_skip: should_skip_expensive_post_commit_stats(&estimate),
    })
}

pub(super) fn estimate_stats_cost(
    repo: &Repository,
    parent_sha: &str,
    commit_sha: &str,
    ignore_patterns: &[String],
) -> Result<StatsCostEstimate, GitAiError> {
    let (mut added_lines_by_file, total_deleted_lines) =
        repo.diff_added_lines_with_deleted_count(parent_sha, commit_sha)?;
    let ignore_matcher = build_ignore_matcher(ignore_patterns);
    added_lines_by_file
        .retain(|file_path, _| !should_ignore_file_with_matcher(file_path, &ignore_matcher));

    let files_with_additions = added_lines_by_file
        .values()
        .filter(|lines| !lines.is_empty())
        .count();

    let mut added_lines = 0usize;
    let mut hunk_ranges = 0usize;

    for (_file, lines) in added_lines_by_file {
        if lines.is_empty() {
            continue;
        }
        added_lines += lines.len();
        hunk_ranges += count_line_ranges(&lines);
    }

    Ok(StatsCostEstimate {
        files_with_additions,
        added_lines,
        hunk_ranges,
        deleted_lines: total_deleted_lines,
    })
}

#[doc(hidden)]
pub fn count_line_ranges(lines: &[u32]) -> usize {
    if lines.is_empty() {
        return 0;
    }

    let mut sorted = lines.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut ranges = 1usize;
    let mut prev = sorted[0];
    for &line in &sorted[1..] {
        if line != prev + 1 {
            ranges += 1;
        }
        prev = line;
    }
    ranges
}
