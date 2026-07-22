//! Per-repository activity breakdown.

use crate::error::GitAiError;
use crate::metrics::local_stats::compute_activity_from_records;
use crate::metrics::local_stats::types::{BucketGranularity, RepoActivitySummary};
use crate::model::repository::metrics_db::MetricHistoryRecord;
use std::collections::HashMap;

/// Aggregate a pre-fetched slice of events into a per-repository breakdown.
pub(super) fn repo_summaries_from_records(
    all_records: &[MetricHistoryRecord],
    since_ts: u32,
    granularity: BucketGranularity,
) -> Result<Vec<RepoActivitySummary>, GitAiError> {
    // Group records by repo_url, skipping events with no repo (NULL) — these
    // predate repo_url emission and have no meaningful identity to display.
    let mut by_repo: HashMap<&str, Vec<&MetricHistoryRecord>> = HashMap::new();
    for record in all_records {
        if let Some(ref url) = record.repo_url {
            by_repo.entry(url.as_str()).or_default().push(record);
        }
    }

    let mut summaries: Vec<RepoActivitySummary> = by_repo
        .into_iter()
        .filter_map(|(url, records)| {
            let stats =
                compute_activity_from_records(&records, since_ts, String::new(), granularity)
                    .ok()?;
            Some(RepoActivitySummary {
                repo_url: url.to_string(),
                ai_lines: stats.commits.ai_lines,
                commits: stats.commits.total,
                sessions: stats.sessions.total,
                estimated_cost_usd: stats.tokens.estimated_cost_usd,
            })
        })
        .collect();

    summaries.sort_by_key(|s| std::cmp::Reverse(s.ai_lines));
    Ok(summaries)
}
