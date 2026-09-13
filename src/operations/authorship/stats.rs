mod render;
pub use render::{write_stats_to_markdown, write_stats_to_terminal};

use crate::error::GitAiError;
use crate::model::authorship_log::LineRange;
use crate::operations::authorship::ignore::{
    build_ignore_matcher, should_ignore_file_with_matcher,
};
use crate::operations::git::notes_api::read_authorship;
use crate::operations::git::repository::Repository;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolModelHeadlineStats {
    #[serde(default)]
    pub ai_additions: u32, // Number of lines committed with AI attribution
    #[serde(default)]
    pub ai_accepted: u32, // Number of AI-generated lines that were accepted by the user without any human edits
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommitStats {
    #[serde(default)]
    pub human_additions: u32, // Number of lines committed with human attribution
    #[serde(default)]
    pub unknown_additions: u32, // Number of lines with no attestation at all
    #[serde(default)]
    pub ai_additions: u32, // Number of lines committed with AI attribution
    #[serde(default)]
    pub ai_accepted: u32, // Number of AI-generated lines that were accepted by the user without any human edits
    #[serde(default)]
    pub git_diff_deleted_lines: u32,
    #[serde(default)]
    pub git_diff_added_lines: u32,
    #[serde(default)]
    pub tool_model_breakdown: BTreeMap<String, ToolModelHeadlineStats>,
}

pub fn stats_command(
    repo: &Repository,
    commit_sha: Option<&str>,
    json: bool,
    ignore_patterns: &[String],
) -> Result<(), GitAiError> {
    let (target, refname) = if let Some(sha) = commit_sha {
        // Validate that the commit exists using revparse_single
        match repo.revparse_single(sha) {
            Ok(commit_obj) => {
                // For a specific commit, we don't have a refname, so use the commit SHA
                let full_sha = commit_obj.id();
                (full_sha, sha.to_string())
            }
            Err(GitAiError::GitCliError { .. }) => {
                return Err(GitAiError::Generic(format!("No commit found: {}", sha)));
            }
            Err(e) => return Err(e),
        }
    } else {
        // Default behavior: use current HEAD
        let head = repo.head()?;
        let target = head.target()?;
        let name = head.name().unwrap_or("HEAD").to_string();
        (target, name)
    };

    tracing::debug!(
        "Stats command found commit: {} refname: {}",
        target,
        refname
    );

    let stats = super::recent_stats::stats_for_commit(repo, &target, ignore_patterns)?;

    if json {
        let json_str = serde_json::to_string(&stats)?;
        println!("{}", json_str);
    } else {
        write_stats_to_terminal(&stats, true);
    }

    Ok(())
}

/// Calculate commit stats from an authorship log
/// This helper can work with both fetched and in-memory authorship logs
pub fn stats_from_authorship_log(
    _authorship_log: Option<&crate::model::authorship_log_serialization::AuthorshipLog>,
    git_diff_added_lines: u32,
    git_diff_deleted_lines: u32,
    ai_accepted: u32,
    known_human_accepted: u32,
    ai_accepted_by_tool: &BTreeMap<String, u32>,
) -> CommitStats {
    let mut commit_stats = CommitStats {
        human_additions: 0,
        unknown_additions: 0,
        ai_additions: 0,
        ai_accepted,
        tool_model_breakdown: BTreeMap::new(),
        git_diff_deleted_lines,
        git_diff_added_lines,
    };

    // Update tool-level accepted counts using diff-based attribution.
    for (tool_model, accepted) in ai_accepted_by_tool {
        let tool_stats = commit_stats
            .tool_model_breakdown
            .entry(tool_model.clone())
            .or_default();
        tool_stats.ai_accepted = *accepted;
    }

    // AI additions = ai_accepted (no mixed component)
    commit_stats.ai_additions = commit_stats.ai_accepted;

    // Set ai_additions for each tool: ai_additions = ai_accepted
    for tool_stats in commit_stats.tool_model_breakdown.values_mut() {
        tool_stats.ai_additions = tool_stats.ai_accepted;
    }

    // KnownHuman-attested additions (positively identified as human-authored)
    commit_stats.human_additions = known_human_accepted;

    // Unknown additions: lines with no attestation at all (not AI-accepted, not KnownHuman)
    commit_stats.unknown_additions = git_diff_added_lines
        .saturating_sub(commit_stats.ai_accepted)
        .saturating_sub(known_human_accepted);

    commit_stats
}

pub fn stats_for_commit_stats(
    repo: &Repository,
    commit_sha: &str,
    ignore_patterns: &[String],
) -> Result<CommitStats, GitAiError> {
    let authorship_log = read_authorship(repo, commit_sha);
    stats_for_commit_stats_with_authorship(
        repo,
        commit_sha,
        ignore_patterns,
        authorship_log.as_ref(),
    )
}

pub fn stats_for_commit_stats_with_authorship(
    repo: &Repository,
    commit_sha: &str,
    ignore_patterns: &[String],
    authorship_log: Option<&crate::model::authorship_log_serialization::AuthorshipLog>,
) -> Result<CommitStats, GitAiError> {
    let commit_obj = repo.revparse_single(commit_sha)?.peel_to_commit()?;
    let parent_count = commit_obj.parent_count()?;

    if parent_count > 1 {
        return stats_for_commit_stats_from_hunks(
            repo,
            commit_sha,
            ignore_patterns,
            &[],
            authorship_log,
        );
    }

    let parent_sha = if parent_count == 0 {
        None
    } else {
        Some(commit_obj.parent(0)?.id())
    };

    stats_for_commit_stats_with_parent_and_authorship(
        repo,
        commit_sha,
        parent_sha.as_deref(),
        ignore_patterns,
        authorship_log,
    )
}

pub fn stats_for_commit_stats_with_parent_and_authorship(
    repo: &Repository,
    commit_sha: &str,
    parent_sha: Option<&str>,
    ignore_patterns: &[String],
    authorship_log: Option<&crate::model::authorship_log_serialization::AuthorshipLog>,
) -> Result<CommitStats, GitAiError> {
    use crate::operations::commands::diff::get_diff_with_line_numbers;

    let from_ref = parent_sha.unwrap_or("4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    let hunks = get_diff_with_line_numbers(repo, from_ref, commit_sha)?;
    stats_for_commit_stats_from_hunks(repo, commit_sha, ignore_patterns, &hunks, authorship_log)
}

#[doc(hidden)]
pub fn accepted_lines_from_attestations(
    authorship_log: Option<&crate::model::authorship_log_serialization::AuthorshipLog>,
    added_lines_by_file: &HashMap<String, Vec<u32>>,
    is_merge_commit: bool,
) -> (u32, u32, BTreeMap<String, u32>) {
    // returns (ai_accepted, known_human_accepted, per_tool_model)
    if is_merge_commit {
        return (0, 0, BTreeMap::new());
    }

    let mut total_ai_accepted = 0u32;
    let mut known_human_accepted = 0u32;
    let mut per_tool_model = BTreeMap::new();

    let Some(log) = authorship_log else {
        return (0, 0, per_tool_model);
    };

    for file_attestation in &log.attestations {
        let Some(added_lines) = added_lines_by_file.get(&file_attestation.file_path) else {
            continue;
        };

        for entry in &file_attestation.entries {
            // KnownHuman entries (h_ prefix): count as known-human-attested lines.
            if entry.hash.starts_with("h_") {
                let accepted = entry
                    .line_ranges
                    .iter()
                    .map(|line_range| line_range_overlap_len(line_range, added_lines))
                    .sum::<u32>();
                if accepted > 0 {
                    known_human_accepted += accepted;
                }
                continue;
            }

            let accepted = entry
                .line_ranges
                .iter()
                .map(|line_range| line_range_overlap_len(line_range, added_lines))
                .sum::<u32>();

            if accepted == 0 {
                continue;
            }

            total_ai_accepted += accepted;

            // Session entries (s_ prefix): look up in sessions map
            if entry.hash.starts_with("s_") {
                let session_key = entry.hash.split("::").next().unwrap_or(&entry.hash);
                if let Some(session_record) = log.metadata.sessions.get(session_key) {
                    let tool_model = format!(
                        "{}::{}",
                        session_record.agent_id.tool, session_record.agent_id.model
                    );
                    *per_tool_model.entry(tool_model).or_insert(0) += accepted;
                }
            } else if let Some(prompt_record) = log.metadata.prompts.get(&entry.hash) {
                let tool_model = format!(
                    "{}::{}",
                    prompt_record.agent_id.tool, prompt_record.agent_id.model
                );
                *per_tool_model.entry(tool_model).or_insert(0) += accepted;
            }
        }
    }

    (total_ai_accepted, known_human_accepted, per_tool_model)
}

#[doc(hidden)]
pub fn line_range_overlap_len(range: &LineRange, added_lines: &[u32]) -> u32 {
    match range {
        LineRange::Single(line) => u32::from(added_lines.binary_search(line).is_ok()),
        LineRange::Range(start, end) => {
            let start_idx = added_lines.partition_point(|line| *line < *start);
            let end_idx = added_lines.partition_point(|line| *line <= *end);
            end_idx.saturating_sub(start_idx) as u32
        }
    }
}

/// Like `stats_for_commit_stats` but accepts pre-computed diff hunks and authorship log,
/// avoiding redundant git subprocess calls in the post-commit hook path.
pub fn stats_for_commit_stats_from_hunks(
    repo: &Repository,
    commit_sha: &str,
    ignore_patterns: &[String],
    hunks: &[crate::operations::commands::diff::DiffHunk],
    authorship_log: Option<&crate::model::authorship_log_serialization::AuthorshipLog>,
) -> Result<CommitStats, GitAiError> {
    let commit_obj = repo.revparse_single(commit_sha)?.peel_to_commit()?;
    let parent_count = commit_obj.parent_count()?;
    let is_merge_commit = parent_count > 1;

    Ok(stats_for_commit_stats_from_hunks_with_merge_flag(
        ignore_patterns,
        hunks,
        authorship_log,
        is_merge_commit,
    ))
}

pub(crate) fn stats_for_commit_stats_from_hunks_with_merge_flag(
    ignore_patterns: &[String],
    hunks: &[crate::operations::commands::diff::DiffHunk],
    authorship_log: Option<&crate::model::authorship_log_serialization::AuthorshipLog>,
    is_merge_commit: bool,
) -> CommitStats {
    let ignore_matcher = build_ignore_matcher(ignore_patterns);

    let mut git_diff_added_lines = 0u32;
    let mut git_diff_deleted_lines = 0u32;
    let mut added_lines_by_file: HashMap<String, Vec<u32>> = HashMap::new();

    for hunk in hunks {
        if should_ignore_file_with_matcher(&hunk.file_path, &ignore_matcher) {
            continue;
        }
        git_diff_added_lines += hunk.added_lines.len() as u32;
        git_diff_deleted_lines += hunk.deleted_lines.len() as u32;

        if !is_merge_commit && !hunk.added_lines.is_empty() {
            added_lines_by_file
                .entry(hunk.file_path.clone())
                .or_default()
                .extend(hunk.added_lines.iter().copied());
        }
    }

    for lines in added_lines_by_file.values_mut() {
        lines.sort_unstable();
        lines.dedup();
    }

    let (ai_accepted, known_human_accepted, ai_accepted_by_tool) =
        accepted_lines_from_attestations(authorship_log, &added_lines_by_file, is_merge_commit);

    stats_from_authorship_log(
        authorship_log,
        git_diff_added_lines,
        git_diff_deleted_lines,
        ai_accepted,
        known_human_accepted,
        &ai_accepted_by_tool,
    )
}

/// Get git diff statistics between commit and its parent
/// Uses the same diff engine as git ai diff to properly handle renames
pub fn get_git_diff_stats(
    repo: &Repository,
    commit_sha: &str,
    ignore_patterns: &[String],
) -> Result<(u32, u32), GitAiError> {
    use crate::operations::commands::diff::get_diff_with_line_numbers;

    let commit_obj = repo.revparse_single(commit_sha)?.peel_to_commit()?;
    let parent_count = commit_obj.parent_count()?;

    // For merge commits, return (0, 0) to match the behavior of `git show --numstat`
    // which shows a combined diff (typically 0 lines for clean merges)
    if parent_count > 1 {
        return Ok((0, 0));
    }

    let from_ref = if parent_count == 0 {
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()
    } else {
        commit_obj.parent(0)?.id()
    };

    // Use the diff engine which properly handles renames with --find-renames=1%
    let hunks = get_diff_with_line_numbers(repo, &from_ref, commit_sha)?;

    let ignore_matcher = build_ignore_matcher(ignore_patterns);
    let mut added_lines = 0u32;
    let mut deleted_lines = 0u32;

    for hunk in hunks {
        if should_ignore_file_with_matcher(&hunk.file_path, &ignore_matcher) {
            continue;
        }
        added_lines += hunk.added_lines.len() as u32;
        deleted_lines += hunk.deleted_lines.len() as u32;
    }

    Ok((added_lines, deleted_lines))
}

#[path = "stats_tests.rs"]
#[cfg(test)]
mod tests;
