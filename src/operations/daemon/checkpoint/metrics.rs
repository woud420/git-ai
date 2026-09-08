use crate::error::GitAiError;
use crate::model::authorship_log_serialization::generate_session_id;
use crate::model::imara_diff_utils::{LineChangeTag, compute_line_changes};
use crate::model::working_log::AgentId;
use crate::operations::git::repository::Repository;

/// Per-file line statistics (in-memory only, not persisted)
#[derive(Debug, Clone, Default)]
#[doc(hidden)]
pub struct FileLineStats {
    pub additions: u32,
    pub deletions: u32,
    pub additions_sloc: u32,
    pub deletions_sloc: u32,
}

/// Build EventAttributes for AgentUsage events.
/// When repo is available, includes repo_url and branch. Always includes tool, model,
/// session_id, and custom attributes.
pub fn build_agent_usage_attrs(
    repo: Option<&Repository>,
    agent_id: &AgentId,
) -> crate::metrics::EventAttributes {
    let session_id = generate_session_id(&agent_id.id, &agent_id.tool);

    let mut attrs = crate::metrics::EventAttributes::with_version(env!("CARGO_PKG_VERSION"))
        .session_id(session_id)
        .tool(&agent_id.tool)
        .model(&agent_id.model)
        .external_session_id(&agent_id.id)
        .custom_attributes_map(crate::config::Config::fresh().custom_attributes());

    if let Some(repo) = repo {
        if let Some(url) = crate::repo_url::resolve_repo_url_from_repo(repo) {
            attrs = attrs.repo_url(url);
        }

        if let Ok(head_ref) = repo.head()
            && let Ok(short_branch) = head_ref.shorthand()
        {
            attrs = attrs.branch(short_branch);
        }
    }

    attrs
}

/// Build EventAttributes with repo metadata.
/// Reused for both AgentUsage and Checkpoint events.
pub(super) fn build_checkpoint_attrs(
    repo: &Repository,
    base_commit: &str,
    agent_id: Option<&AgentId>,
) -> crate::metrics::EventAttributes {
    // Extract session_id from agent_id if available
    let session_id = agent_id
        .as_ref()
        .map(|aid| generate_session_id(&aid.id, &aid.tool))
        .unwrap_or_default();

    let mut attrs = crate::metrics::EventAttributes::with_version(env!("CARGO_PKG_VERSION"))
        .session_id(session_id)
        .base_commit_sha(base_commit);

    // Add AI-specific attributes
    if let Some(agent_id) = agent_id {
        attrs = attrs
            .tool(&agent_id.tool)
            .model(&agent_id.model)
            .external_session_id(&agent_id.id);
    }

    // Attach custom attributes using Config::fresh() to support runtime config updates
    attrs = attrs.custom_attributes_map(crate::config::Config::fresh().custom_attributes());

    // Add repo URL
    if let Some(url) = crate::repo_url::resolve_repo_url_from_repo(repo) {
        attrs = attrs.repo_url(url);
    }

    // Add branch
    if let Ok(head_ref) = repo.head()
        && let Ok(short_branch) = head_ref.shorthand()
    {
        attrs = attrs.branch(short_branch);
    }

    attrs
}

/// Compute line statistics for a single file by diffing previous and current content
#[doc(hidden)]
pub fn compute_file_line_stats(previous_content: &str, current_content: &str) -> FileLineStats {
    let mut stats = FileLineStats::default();

    // Use imara_diff to count line changes (matches git's diff algorithm)
    let changes = compute_line_changes(previous_content, current_content);
    for change in changes {
        match change.tag() {
            LineChangeTag::Insert => {
                let non_whitespace_lines = change
                    .value()
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count() as u32;
                stats.additions += change.value().lines().count() as u32;
                stats.additions_sloc += non_whitespace_lines;
            }
            LineChangeTag::Delete => {
                let non_whitespace_lines = change
                    .value()
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count() as u32;
                stats.deletions += change.value().lines().count() as u32;
                stats.deletions_sloc += non_whitespace_lines;
            }
            LineChangeTag::Equal => {}
        }
    }

    stats
}

/// Aggregate line statistics from individual file stats
/// This avoids redundant diff computation since stats are already computed during entry creation
pub(super) fn compute_line_stats(
    file_stats: &[FileLineStats],
) -> Result<crate::model::working_log::CheckpointLineStats, GitAiError> {
    let mut stats = crate::model::working_log::CheckpointLineStats::default();

    // Aggregate line stats from all files
    for file_stat in file_stats {
        stats.additions += file_stat.additions;
        stats.deletions += file_stat.deletions;
        stats.additions_sloc += file_stat.additions_sloc;
        stats.deletions_sloc += file_stat.deletions_sloc;
    }

    Ok(stats)
}
