use std::collections::{HashMap, HashSet};

use crate::config::Config;
use crate::error::GitAiError;
use crate::metrics::{EventAttributes, MetricEvent, PosEncoded, RewriteCommittedValues};
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::authorship::ignore::effective_ignore_patterns;
use crate::operations::authorship::post_commit::metric_tool_model_breakdown;
use crate::operations::authorship::rewrite::{DiffTreeResult, RewriteMetricCommit};
use crate::operations::git::repository::Repository;

pub(crate) fn spawn_rewrite_commit_metrics(
    repo: &Repository,
    metric_commits: Vec<RewriteMetricCommit>,
) {
    if !crate::operations::authorship::rewrite::rewrite_metrics_enabled() {
        return;
    }
    if metric_commits.is_empty() {
        return;
    }

    let repo = repo.clone();
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                build_rewrite_metric_events(&repo, &metric_commits)
            })
            .await;
            match result {
                Ok(events) => submit_events(events),
                Err(err) => tracing::warn!(%err, "rewrite metrics worker panicked"),
            }
        });
    } else {
        std::thread::spawn(move || {
            submit_events(build_rewrite_metric_events(&repo, &metric_commits));
        });
    }
}

fn submit_events(events: Vec<MetricEvent>) {
    if !events.is_empty() {
        crate::observability::log_metrics(events);
    }
}

pub(crate) fn dedupe_metric_commits(
    metric_commits: Vec<RewriteMetricCommit>,
) -> Vec<RewriteMetricCommit> {
    #[derive(Hash, PartialEq, Eq)]
    struct MetricCommitKey {
        new_sha: String,
        original_shas: Vec<String>,
        operation: crate::operations::authorship::rewrite::RewriteMetricOperation,
        branch: Option<String>,
    }

    let mut deduped = Vec::new();
    let mut indices_by_key: HashMap<MetricCommitKey, usize> = HashMap::new();
    for commit in metric_commits {
        if commit.new_sha.is_empty() {
            continue;
        }
        let key = MetricCommitKey {
            new_sha: commit.new_sha.clone(),
            original_shas: commit.original_shas.clone(),
            operation: commit.operation,
            branch: commit.branch.clone(),
        };
        if let Some(index) = indices_by_key.get(&key).copied() {
            merge_metric_commit_context(&mut deduped[index], commit);
        } else {
            indices_by_key.insert(key, deduped.len());
            deduped.push(commit);
        }
    }
    deduped
}

fn merge_metric_commit_context(target: &mut RewriteMetricCommit, source: RewriteMetricCommit) {
    if target.parent_sha.is_none() {
        target.parent_sha = source.parent_sha;
    }
    if target.authorship_note.is_none() {
        target.authorship_note = source.authorship_note;
    }
    if target.parent_diff.is_none() {
        target.parent_diff = source.parent_diff;
    }
}

fn build_rewrite_metric_events(
    repo: &Repository,
    metric_commits: &[RewriteMetricCommit],
) -> Vec<MetricEvent> {
    let mut metric_commits = dedupe_metric_commits(metric_commits.to_vec());
    hydrate_missing_parent_shas(repo, &mut metric_commits);
    hydrate_missing_parent_diffs(repo, &mut metric_commits);
    let batch_context = RewriteMetricBatchContext::new(repo);

    let mut events = Vec::new();
    for metric_commit in &metric_commits {
        match build_rewrite_committed_metric_event(metric_commit, &batch_context) {
            Ok(Some(event)) => events.push(event),
            Ok(None) => {}
            Err(err) => {
                tracing::debug!(
                    %err,
                    commit_sha = %metric_commit.new_sha,
                    operation_kind = metric_commit.operation.as_str(),
                    "skipping rewrite committed metric"
                );
            }
        }
    }
    events
}

fn hydrate_missing_parent_shas(repo: &Repository, metric_commits: &mut [RewriteMetricCommit]) {
    let mut new_shas = Vec::new();
    let mut seen = HashSet::new();
    for metric_commit in metric_commits.iter() {
        if metric_commit.parent_sha.is_some() {
            continue;
        }
        if seen.insert(metric_commit.new_sha.clone()) {
            new_shas.push(metric_commit.new_sha.clone());
        }
    }
    if new_shas.is_empty() {
        return;
    }

    let Some(parent_by_commit) = parent_shas_for_commits(repo, &new_shas) else {
        return;
    };
    for metric_commit in metric_commits {
        if metric_commit.parent_sha.is_none()
            && let Some(parent_sha) = parent_by_commit.get(&metric_commit.new_sha)
        {
            metric_commit.parent_sha = Some(parent_sha.clone());
        }
    }
}

fn parent_shas_for_commits(
    repo: &Repository,
    commit_shas: &[String],
) -> Option<HashMap<String, String>> {
    if commit_shas.is_empty() {
        return Some(HashMap::new());
    }

    let mut args = repo.global_args_for_exec();
    args.extend([
        "show".to_string(),
        "-s".to_string(),
        "--format=%H %P".to_string(),
        "--no-walk".to_string(),
    ]);
    args.extend(commit_shas.iter().cloned());

    let output = crate::clients::git_cli::exec_git(&args).ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parent_by_commit = HashMap::new();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let Some(commit_sha) = parts.next() else {
            continue;
        };
        let parents = parts.collect::<Vec<_>>();
        match parents.as_slice() {
            [] => {
                parent_by_commit.insert(commit_sha.to_string(), "initial".to_string());
            }
            [parent_sha] => {
                parent_by_commit.insert(commit_sha.to_string(), (*parent_sha).to_string());
            }
            _ => {
                // Existing rewrite metrics skip merge commits.
            }
        }
    }
    Some(parent_by_commit)
}

struct RewriteMetricBatchContext {
    ignore_patterns: Vec<String>,
    repo_url: Option<String>,
    custom_attributes_json: Option<String>,
}

impl RewriteMetricBatchContext {
    fn new(repo: &Repository) -> Self {
        Self {
            ignore_patterns: effective_ignore_patterns(repo, &[], &[]),
            repo_url: rewrite_metric_repo_url(repo),
            custom_attributes_json: rewrite_metric_custom_attributes_json(),
        }
    }
}

fn rewrite_metric_repo_url(repo: &Repository) -> Option<String> {
    let remotes = repo.remotes_with_urls().ok()?;
    let (_, url) = remotes
        .iter()
        .find(|(name, _)| name == "origin")
        .or_else(|| remotes.first())?;
    crate::repo_url::normalize_repo_url(url).ok()
}

fn rewrite_metric_custom_attributes_json() -> Option<String> {
    let config = Config::fresh();
    let attrs = config.custom_attributes();
    if attrs.is_empty() {
        None
    } else {
        serde_json::to_string(attrs).ok()
    }
}

fn hydrate_missing_parent_diffs(repo: &Repository, metric_commits: &mut [RewriteMetricCommit]) {
    let mut indices = Vec::new();
    let mut pairs = Vec::new();
    for (index, metric_commit) in metric_commits.iter().enumerate() {
        if metric_commit.parent_diff.is_some() {
            continue;
        }
        let Some(parent_sha) = metric_commit.parent_sha.as_ref() else {
            continue;
        };
        indices.push(index);
        pairs.push((parent_sha.clone(), metric_commit.new_sha.clone()));
    }
    if pairs.is_empty() {
        return;
    }

    let Ok(results) =
        crate::operations::authorship::rewrite::compute_diff_trees_batch(repo, &pairs)
    else {
        return;
    };
    for (index, result) in indices.into_iter().zip(results) {
        metric_commits[index].parent_diff = Some(result);
    }
}

fn build_rewrite_committed_metric_event(
    metric_commit: &RewriteMetricCommit,
    batch_context: &RewriteMetricBatchContext,
) -> Result<Option<MetricEvent>, GitAiError> {
    let Some(raw_note) = metric_commit.authorship_note.as_ref() else {
        return Ok(None);
    };
    let authorship_log = match AuthorshipLog::deserialize_from_string(raw_note) {
        Ok(log) => log,
        Err(_) => return Ok(None),
    };

    let Some(parent_diff) = metric_commit.parent_diff.as_ref() else {
        return Ok(None);
    };

    let diff_hunks = diff_hunks_from_diff_tree_result(parent_diff);
    if should_skip_rewrite_metric_stats(&diff_hunks, &batch_context.ignore_patterns) {
        return Ok(None);
    }
    let stats =
        crate::operations::authorship::stats::stats_for_commit_stats_from_hunks_with_merge_flag(
            &batch_context.ignore_patterns,
            &diff_hunks,
            Some(&authorship_log),
            false,
        );
    let Some(breakdown) = metric_tool_model_breakdown(&stats) else {
        return Ok(None);
    };

    let mut values = RewriteCommittedValues::new()
        .human_additions(stats.human_additions)
        .git_diff_deleted_lines(stats.git_diff_deleted_lines)
        .git_diff_added_lines(stats.git_diff_added_lines)
        .tool_model_pairs(breakdown.tool_model_pairs)
        .ai_additions(breakdown.ai_additions)
        .ai_accepted(breakdown.ai_accepted)
        .authorship_note(raw_note.clone())
        .operation_kind(metric_commit.operation.as_str())
        .original_commit_shas(metric_commit.original_shas.clone());

    values = values.commit_subject_null().commit_body_null().hunks_null();

    let attrs = rewrite_metric_attrs(metric_commit, batch_context);

    Ok(Some(MetricEvent::from_values(values, attrs.to_sparse())))
}

fn diff_hunks_from_diff_tree_result(
    result: &DiffTreeResult,
) -> Vec<crate::operations::commands::diff::DiffHunk> {
    let mut hunks = Vec::new();
    for (file_path, file_hunks) in &result.hunks_by_file {
        for hunk in file_hunks {
            hunks.push(crate::operations::commands::diff::DiffHunk {
                file_path: file_path.clone(),
                old_file_path: None,
                old_start: hunk.old_start,
                old_count: hunk.old_count,
                new_start: hunk.new_start,
                new_count: hunk.new_count,
                deleted_lines: line_numbers(hunk.old_start, hunk.old_count),
                added_lines: line_numbers(hunk.new_start, hunk.new_count),
                deleted_contents: Vec::new(),
                added_contents: Vec::new(),
            });
        }
    }
    hunks
}

fn line_numbers(start: u32, count: u32) -> Vec<u32> {
    if count == 0 {
        return Vec::new();
    }
    (start..start.saturating_add(count))
        .filter(|line| *line > 0)
        .collect()
}

fn should_skip_rewrite_metric_stats(
    hunks: &[crate::operations::commands::diff::DiffHunk],
    ignore_patterns: &[String],
) -> bool {
    let ignore_matcher =
        crate::operations::authorship::ignore::build_ignore_matcher(ignore_patterns);
    let mut files_with_additions = std::collections::HashSet::new();
    let mut added_lines = 0usize;
    let mut deleted_lines = 0usize;
    let mut hunk_ranges = 0usize;

    for hunk in hunks {
        if crate::operations::authorship::ignore::should_ignore_file_with_matcher(
            &hunk.file_path,
            &ignore_matcher,
        ) {
            continue;
        }
        if !hunk.added_lines.is_empty() {
            files_with_additions.insert(hunk.file_path.as_str());
            hunk_ranges += 1;
        }
        added_lines += hunk.added_lines.len();
        deleted_lines += hunk.deleted_lines.len();
    }

    crate::operations::authorship::post_commit::should_skip_expensive_post_commit_stats(
        &crate::operations::authorship::post_commit::StatsCostEstimate {
            hunk_ranges,
            added_lines,
            files_with_additions: files_with_additions.len(),
            deleted_lines,
        },
    )
}

fn rewrite_metric_attrs(
    metric_commit: &RewriteMetricCommit,
    batch_context: &RewriteMetricBatchContext,
) -> EventAttributes {
    let base_commit_sha = metric_commit.parent_sha.as_deref().unwrap_or("initial");
    let mut attrs = EventAttributes::with_version(env!("CARGO_PKG_VERSION"))
        .commit_sha(metric_commit.new_sha.clone())
        .base_commit_sha(base_commit_sha);

    attrs = apply_rewrite_metric_branch(attrs, metric_commit);

    if let Some(repo_url) = batch_context.repo_url.as_deref() {
        attrs = attrs.repo_url(repo_url);
    }

    attrs = apply_rewrite_metric_custom_attributes(
        attrs,
        batch_context.custom_attributes_json.as_deref(),
    );

    attrs
}

fn apply_rewrite_metric_custom_attributes(
    attrs: EventAttributes,
    custom_attributes_json: Option<&str>,
) -> EventAttributes {
    if let Some(custom_attributes_json) = custom_attributes_json {
        // `custom_attributes_map` serializes the map and stores this same string field.
        // Rewrite metrics pre-serialize once per batch to avoid repeated serde work.
        attrs.custom_attributes(custom_attributes_json)
    } else {
        attrs
    }
}

fn apply_rewrite_metric_branch(
    attrs: EventAttributes,
    metric_commit: &RewriteMetricCommit,
) -> EventAttributes {
    if let Some(branch) = metric_commit.branch.as_deref() {
        return attrs.branch(branch);
    }

    attrs
}

#[path = "rewrite_metrics_tests.rs"]
#[cfg(test)]
mod tests;
