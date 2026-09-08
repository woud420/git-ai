use crate::config::Config;
use crate::error::GitAiError;
use crate::model::working_log::Checkpoint;
use crate::operations::git::patch_id::{PatchDiffMode, stable_patch_ids_for_commits};
use crate::operations::git::repository::Repository;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MetricToolModelBreakdown {
    pub tool_model_pairs: Vec<String>,
    pub ai_additions: Vec<u32>,
    pub ai_accepted: Vec<u32>,
}

/// Build the metrics tool/model arrays and remove mock_ai test data.
/// Returns None when the entire event would only represent mock_ai data.
pub(crate) fn metric_tool_model_breakdown(
    stats: &crate::operations::authorship::stats::CommitStats,
) -> Option<MetricToolModelBreakdown> {
    let only_mock_ai = !stats.tool_model_breakdown.is_empty()
        && stats
            .tool_model_breakdown
            .keys()
            .all(|k| k.starts_with("mock_ai::"));
    if only_mock_ai {
        return None;
    }

    let mut agg_ai = stats.ai_additions;
    let mut agg_accepted = stats.ai_accepted;
    for (key, ts) in &stats.tool_model_breakdown {
        if key.starts_with("mock_ai::") {
            agg_ai = agg_ai.saturating_sub(ts.ai_additions);
            agg_accepted = agg_accepted.saturating_sub(ts.ai_accepted);
        }
    }

    let mut tool_model_pairs: Vec<String> = vec!["all".to_string()];
    let mut ai_additions: Vec<u32> = vec![agg_ai];
    let mut ai_accepted: Vec<u32> = vec![agg_accepted];

    for (tool_model, tool_stats) in &stats.tool_model_breakdown {
        if tool_model.starts_with("mock_ai::") {
            continue;
        }
        tool_model_pairs.push(tool_model.clone());
        ai_additions.push(tool_stats.ai_additions);
        ai_accepted.push(tool_stats.ai_accepted);
    }

    Some(MetricToolModelBreakdown {
        tool_model_pairs,
        ai_additions,
        ai_accepted,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommitMetricMetadata {
    pub subject: Option<String>,
    pub body: Option<String>,
    pub author_ts: Option<u64>,
    pub commit_ts: Option<u64>,
}

pub(crate) fn commit_metric_metadata(
    repo: &Repository,
    commit_sha: &str,
) -> Result<CommitMetricMetadata, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.extend([
        "show".to_string(),
        "-s".to_string(),
        "--no-notes".to_string(),
        "--encoding=UTF-8".to_string(),
        "--format=%s%x00%b%x00%at%x00%ct".to_string(),
        commit_sha.to_string(),
    ]);
    let output = crate::clients::git_cli::exec_git(&args)?;
    let stdout = String::from_utf8(output.stdout)?;
    Ok(parse_commit_metric_metadata_output(&stdout))
}

pub(super) fn parse_commit_metric_metadata_output(output: &str) -> CommitMetricMetadata {
    let mut parts = output.splitn(4, '\0');
    let Some(subject) = parts.next() else {
        return CommitMetricMetadata::default();
    };
    let Some(body) = parts.next() else {
        return CommitMetricMetadata::default();
    };
    let Some(author_ts) = parts.next() else {
        return CommitMetricMetadata::default();
    };
    let Some(commit_ts) = parts.next() else {
        return CommitMetricMetadata::default();
    };

    let subject = subject.trim().to_string();
    let body = body.trim().to_string();

    CommitMetricMetadata {
        subject: Some(subject),
        body: (!body.is_empty()).then_some(body),
        author_ts: author_ts.trim().parse::<u64>().ok(),
        commit_ts: commit_ts.trim().parse::<u64>().ok(),
    }
}

pub(crate) fn stable_patch_id_for_commit(repo: &Repository, commit_sha: &str) -> Option<String> {
    stable_patch_ids_for_commits(repo, &[commit_sha.to_string()], PatchDiffMode::Configured)
        .ok()
        .and_then(|patch_ids| patch_ids.into_iter().next())
        .flatten()
}

pub(crate) fn commit_metric_attrs(
    repo: &Repository,
    commit_sha: &str,
    parent_sha: &str,
    human_author: &str,
) -> crate::metrics::EventAttributes {
    let mut attrs = crate::metrics::EventAttributes::with_version(env!("CARGO_PKG_VERSION"))
        .author(human_author)
        .commit_sha(commit_sha)
        .base_commit_sha(parent_sha);

    if let Ok(Some(remote_name)) = repo.get_default_remote()
        && let Ok(remotes) = repo.remotes_with_urls()
        && let Some((_, url)) = remotes.into_iter().find(|(n, _)| n == &remote_name)
        && let Ok(normalized) = crate::repo_url::normalize_repo_url(&url)
    {
        attrs = attrs.repo_url(normalized);
    }

    if let Ok(head_ref) = repo.head()
        && let Ok(short_branch) = head_ref.shorthand()
    {
        attrs = attrs.branch(short_branch);
    }

    attrs.custom_attributes_map(Config::fresh().custom_attributes())
}

/// Record metrics for a committed change.
/// This is a best-effort operation - failures are silently ignored.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_commit_metrics(
    repo: &Repository,
    commit_sha: &str,
    parent_sha: &str,
    human_author: &str,
    authorship_note: &str,
    stats: &crate::operations::authorship::stats::CommitStats,
    checkpoints: &[Checkpoint],
    hunks_json: Option<&str>,
) {
    use crate::metrics::{CommittedValues, record};

    let Some(breakdown) = metric_tool_model_breakdown(stats) else {
        return;
    };

    // Build values with all stats
    let values = CommittedValues::new()
        .human_additions(stats.human_additions)
        .git_diff_deleted_lines(stats.git_diff_deleted_lines)
        .git_diff_added_lines(stats.git_diff_added_lines)
        .tool_model_pairs(breakdown.tool_model_pairs)
        .ai_additions(breakdown.ai_additions)
        .ai_accepted(breakdown.ai_accepted);

    // Add first checkpoint timestamp (null if no checkpoints)
    let values = if let Some(first) = checkpoints.first() {
        values.first_checkpoint_ts(first.timestamp)
    } else {
        values.first_checkpoint_ts_null()
    };

    let metadata = commit_metric_metadata(repo, commit_sha).unwrap_or_default();
    let values = match metadata.subject {
        Some(subject) => values.commit_subject(subject),
        None => values.commit_subject_null(),
    };
    let values = match metadata.body {
        Some(body) => values.commit_body(body),
        None => values.commit_body_null(),
    };
    let values = match metadata.author_ts {
        Some(author_ts) => values.author_ts(author_ts),
        None => values.author_ts_null(),
    };
    let values = match metadata.commit_ts {
        Some(commit_ts) => values.commit_ts(commit_ts),
        None => values.commit_ts_null(),
    };
    let values = match stable_patch_id_for_commit(repo, commit_sha) {
        Some(patch_id) => values.patch_id(patch_id),
        None => values.patch_id_null(),
    }
    .authorship_note(authorship_note);

    let values = if let Some(hunks) = hunks_json {
        values.hunks(hunks)
    } else {
        values.hunks_null()
    };

    let attrs = commit_metric_attrs(repo, commit_sha, parent_sha, human_author);

    // Record the metric
    record(values, attrs);
}
