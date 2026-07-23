//! In-memory aggregation of persisted metric events for `git-ai usage`.

/// How long after a session's last message a subsequent commit is attributed
/// to that session for yield and ai_lines_committed calculations.
const YIELD_WINDOW_SECS: u32 = 4 * 3600;

pub(super) const SESSION_RAW_JSON_KEY: &str = "0";
const USAGE_EVENT_IDS: &[u16] = &[
    1, // Committed
    4, // Checkpoint
    5, // SessionEvent
];

mod aggregation;
mod buckets;
mod repo_summaries;
mod tokens;

mod types;

pub use types::{
    ActivitySummary, BucketGranularity, BucketStats, CheckpointSummary, CommitSummary, DayActivity,
    LocalActivityStats, RepoActivitySummary, SessionSummary, TokenModelStat, TokenSummary,
    WowSpend, YieldStats,
};

#[cfg(test)]
mod tests;

use crate::error::GitAiError;
use crate::metrics::attrs::attr_pos;
use crate::metrics::pos_encoded::sparse_get_string;
use crate::model::repository::metrics_db::{MetricHistoryRecord, MetricsDatabase};
use aggregation::{aggregate_checkpoint, aggregate_committed, aggregate_session, compute_streaks};
use buckets::{BucketAccum, bucket_key, fill_buckets, ts_to_local};
use chrono::{Datelike, Local, Timelike};
use repo_summaries::repo_summaries_from_records;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use tokens::{
    CodexSessionAccum, TokenAccum, aggregate_codex_tokens, aggregate_session_tokens,
    build_token_summary,
};

/// Acquire the global DB lock and fetch metric history for the given window.
fn fetch_metric_history(
    since_ts: u32,
    repo_filter: Option<&str>,
) -> Result<Vec<MetricHistoryRecord>, GitAiError> {
    let db = MetricsDatabase::global()?;
    let db_lock = db
        .lock()
        .map_err(|_| GitAiError::Generic("metrics DB lock poisoned".to_string()))?;
    db_lock.get_metric_history(since_ts, repo_filter, USAGE_EVENT_IDS)
}

/// Aggregate metric history since `since_ts` (Unix seconds) into activity stats.
///
/// When `repo_filter` is `Some(url)`, only events from that repository are
/// aggregated. When `None`, events from all repositories are included.
pub fn compute_activity(
    since_ts: u32,
    period_label: String,
    granularity: BucketGranularity,
    repo_filter: Option<&str>,
) -> Result<LocalActivityStats, GitAiError> {
    let records = fetch_metric_history(since_ts, repo_filter)?;
    let refs: Vec<&MetricHistoryRecord> = records.iter().collect();
    compute_activity_from_records(&refs, since_ts, period_label, granularity)
}

/// Aggregate a pre-fetched slice of `MetricHistoryRecord`s into activity stats.
///
/// Separated from `compute_activity` so callers that already hold all events
/// (e.g. `compute_repo_summaries`) can avoid re-fetching from the DB per repo.
fn compute_activity_from_records(
    records: &[&MetricHistoryRecord],
    since_ts: u32,
    period_label: String,
    granularity: BucketGranularity,
) -> Result<LocalActivityStats, GitAiError> {
    let mut total_commits = 0u32;
    let mut total_ai_lines = 0u32;
    let mut total_human_lines = 0u32;
    let mut total_diff_added = 0u32;
    let mut commit_tool_counts: HashMap<String, u32> = HashMap::new();

    let mut total_checkpoints = 0u32;
    let mut ai_lines_added = 0u32;
    let mut human_lines_added = 0u32;
    let mut files_edited: HashSet<String> = HashSet::new();
    // Checkpoint AI lines keyed by plain tool name, for per-tool acceptance rate.
    let mut checkpoint_ai_by_tool: HashMap<String, u32> = HashMap::new();
    // Committed AI lines keyed by plain tool name (extracted from tool::model pairs).
    let mut committed_ai_by_plain_tool: HashMap<String, u32> = HashMap::new();

    let mut session_ids: HashSet<String> = HashSet::new();
    let mut session_tool_counts: HashMap<String, u32> = HashMap::new();

    // Claude-shaped token usage keyed by assistant message id. Value is
    // (model, accum, record_ts, session_id). `record_ts` is the Unix timestamp of the
    // first event that introduced this message id — used for WoW bucketing.
    let mut message_usage: HashMap<String, (String, TokenAccum, u32, String)> = HashMap::new();

    // Codex-shaped token usage keyed by session id. Codex reports cumulative
    // session totals (total_token_usage) on each token_count event, so we keep
    // the per-session max rather than summing.
    let mut codex_sessions: HashMap<String, CodexSessionAccum> = HashMap::new();

    // bucket_key -> accumulated stats
    let mut bucket_map: HashMap<String, BucketAccum> = HashMap::new();
    // bucket_key -> sort key (for ordering)
    let mut bucket_order: HashMap<String, i64> = HashMap::new();

    let mut hourly: Vec<u32> = vec![0u32; 24];
    let mut daily: Vec<u32> = vec![0u32; 7];
    // AI lines committed per local calendar day (sorted; sparse — only days with
    // AI activity). Drives the contribution calendar and all derived day stats.
    let mut ai_lines_by_day: BTreeMap<chrono::NaiveDate, u32> = BTreeMap::new();

    // Yield classification: track the latest timestamp seen per session, and
    // all commit timestamps, then correlate after the loop.
    let mut session_last_ts: HashMap<String, u32> = HashMap::new();
    // First timestamp seen per session, for longest-session duration.
    let mut session_first_ts: HashMap<String, u32> = HashMap::new();
    let mut commit_timestamps: Vec<u32> = Vec::new();

    for record in records {
        let event = &record.event;

        match record.event_id {
            1 => {
                commit_timestamps.push(record.ts);
                let c = aggregate_committed(
                    event,
                    &mut total_commits,
                    &mut total_ai_lines,
                    &mut total_human_lines,
                    &mut total_diff_added,
                    &mut commit_tool_counts,
                    &mut committed_ai_by_plain_tool,
                );

                // Bucket every commit that added lines so coverage spans all
                // committed code, not just AI commits.
                if c.diff_added > 0 {
                    let local_dt = ts_to_local(record.ts);
                    if c.ai_lines > 0 {
                        hourly[local_dt.hour() as usize] += c.ai_lines;
                        // Weekday: Mon=0 … Sun=6 (chrono's num_days_from_monday).
                        daily[local_dt.weekday().num_days_from_monday() as usize] += c.ai_lines;
                        *ai_lines_by_day.entry(local_dt.date_naive()).or_insert(0) += c.ai_lines;
                    }

                    let (key, order_key) = bucket_key(&local_dt, granularity);
                    let entry = bucket_map.entry(key.clone()).or_default();
                    entry.ai_lines += c.ai_lines;
                    // Count AI commits only, to match the AI-lines bar.
                    if c.ai_lines > 0 {
                        entry.commit_count += 1;
                    }
                    entry.diff_added += c.diff_added;
                    entry.attributed += c.ai_lines + c.human_lines;
                    bucket_order.entry(key).or_insert(order_key);
                }
            }
            4 => aggregate_checkpoint(
                event,
                &mut total_checkpoints,
                &mut ai_lines_added,
                &mut human_lines_added,
                &mut files_edited,
                &mut checkpoint_ai_by_tool,
            ),
            5 => {
                aggregate_session(event, &mut session_ids, &mut session_tool_counts);

                // Track first/last-seen timestamp per session for yield
                // classification and longest-session duration.
                if let Some(sid) = sparse_get_string(&event.attrs, attr_pos::SESSION_ID).flatten() {
                    let last = session_last_ts.entry(sid.clone()).or_insert(0);
                    *last = (*last).max(record.ts);
                    let first = session_first_ts.entry(sid).or_insert(record.ts);
                    *first = (*first).min(record.ts);
                }
                let tool = sparse_get_string(&event.attrs, attr_pos::TOOL)
                    .flatten()
                    .unwrap_or_default();
                if tool == "codex" {
                    aggregate_codex_tokens(event, record.ts, &mut codex_sessions);
                } else {
                    let sid = sparse_get_string(&event.attrs, attr_pos::SESSION_ID)
                        .flatten()
                        .unwrap_or_default();
                    aggregate_session_tokens(event, record.ts, sid, &mut message_usage);
                }
            }
            _ => {}
        }
    }

    // Yield classification: for each unique session, check if a commit landed
    // within 4 hours of the session's last observed event.
    //
    // Limitation: the all-repos view aggregates activity globally, so a commit
    // in repo-A can incorrectly "claim" a nearby session from repo-B. The
    // per-repo view avoids this by grouping on repo_url before aggregation.

    commit_timestamps.sort_unstable();
    let mut yield_shipped = 0u32;
    let mut yield_abandoned = 0u32;
    for last_ts in session_last_ts.values() {
        let window_end = last_ts.saturating_add(YIELD_WINDOW_SECS);
        // Find the first commit at or after this session's last event.
        let pos = commit_timestamps.partition_point(|&t| t < *last_ts);
        if commit_timestamps.get(pos).is_some_and(|&t| t <= window_end) {
            yield_shipped += 1;
        } else {
            yield_abandoned += 1;
        }
    }

    // Per-tool acceptance rate: committed AI lines / checkpoint AI lines.
    // Values >100 indicate incomplete checkpoint data (e.g. checkpoint events
    // aged out of the window while committed events remain). u32::MAX is the
    // sentinel for "no checkpoint events at all" — same display path as >100.
    let mut acceptance_by_tool: Vec<(String, u32)> = committed_ai_by_plain_tool
        .iter()
        .map(|(tool, &committed)| {
            let pct = match checkpoint_ai_by_tool.get(tool).copied() {
                Some(checkpoint) if checkpoint > 0 => (committed as u64 * 100)
                    .checked_div(checkpoint as u64)
                    .map(|p| p as u32)
                    .unwrap_or(u32::MAX),
                _ => u32::MAX,
            };
            (tool.clone(), pct)
        })
        .collect();
    acceptance_by_tool.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut commit_by_tool: Vec<(String, u32)> = commit_tool_counts.into_iter().collect();
    commit_by_tool.sort_by_key(|&(_, count)| Reverse(count));

    let mut session_by_tool: Vec<(String, u32)> = session_tool_counts.into_iter().collect();
    session_by_tool.sort_by_key(|&(_, count)| Reverse(count));

    let now_ts = crate::model::clock::now_secs() as u32;
    let (tokens, cost_by_day) =
        build_token_summary(message_usage, codex_sessions, now_ts, since_ts);

    // Map by order key for fill_buckets to look up real data.
    let bucket_by_order: HashMap<i64, BucketAccum> = bucket_map
        .into_iter()
        .map(|(label, accum)| (bucket_order[&label], accum))
        .collect();

    // Fill in empty buckets between since_ts and now so the chart has no gaps.
    let filled = fill_buckets(bucket_by_order, since_ts, granularity);

    // ── Derived calendar + summary stats ──
    let calendar_end = Local::now().date_naive();
    // Earliest day with any activity — AI lines OR token spend — so a spend-only
    // day before the first AI-line day isn't clipped from the all-time window.
    let first_active = ai_lines_by_day
        .keys()
        .next()
        .copied()
        .into_iter()
        .chain(cost_by_day.keys().next().copied())
        .min();
    let calendar_start = if since_ts == 0 {
        first_active.unwrap_or(calendar_end)
    } else {
        ts_to_local(since_ts).date_naive()
    };

    let active_days = ai_lines_by_day.len() as u32;
    // Denominator for "active days X/Y": the length of the selected window in
    // days. For the all-time window (since_ts == 0) there is no fixed length, so
    // fall back to days elapsed since the first active day.
    let total_days = if since_ts == 0 {
        first_active
            .map(|first| ((calendar_end - first).num_days() + 1).max(0) as u32)
            .unwrap_or(0)
    } else {
        (now_ts.saturating_sub(since_ts) / 86_400).max(1)
    };
    let (longest_streak, current_streak) = compute_streaks(&ai_lines_by_day, calendar_end);
    let most_active_day = ai_lines_by_day
        .iter()
        .fold(
            None::<(chrono::NaiveDate, u32)>,
            |best, (&d, &v)| match best {
                Some((_, bv)) if bv >= v => best,
                _ => Some((d, v)),
            },
        )
        .map(|(date, ai_lines)| DayActivity {
            date,
            ai_lines,
            estimated_cost_usd: cost_by_day.get(&date).copied().unwrap_or(0.0),
        });
    let longest_session_secs = session_last_ts
        .iter()
        .map(|(sid, &last)| last.saturating_sub(*session_first_ts.get(sid).unwrap_or(&last)))
        .max()
        .unwrap_or(0);
    // Union of AI-line days and spend days: a spend-heavy / low-lines day must
    // appear so the second heatmap row can surface it.
    let mut all_days: BTreeSet<chrono::NaiveDate> = ai_lines_by_day.keys().copied().collect();
    all_days.extend(cost_by_day.keys().copied());
    let calendar: Vec<DayActivity> = all_days
        .iter()
        .map(|&date| DayActivity {
            date,
            ai_lines: ai_lines_by_day.get(&date).copied().unwrap_or(0),
            estimated_cost_usd: cost_by_day.get(&date).copied().unwrap_or(0.0),
        })
        .collect();

    let summary = ActivitySummary {
        active_days,
        total_days,
        longest_streak,
        current_streak,
        most_active_day,
        longest_session_secs,
        favorite_model: tokens.by_model.first().map(|m| m.model.clone()),
    };

    Ok(LocalActivityStats {
        period_label,
        commits: CommitSummary {
            total: total_commits,
            ai_lines: total_ai_lines,
            human_lines: total_human_lines,
            diff_added_lines: total_diff_added,
            by_tool: commit_by_tool,
            acceptance_by_tool,
        },
        checkpoints: CheckpointSummary {
            total: total_checkpoints,
            ai_lines_added,
            human_lines_added,
            files_edited: files_edited.len() as u32,
        },
        sessions: SessionSummary {
            total: session_ids.len() as u32,
            by_tool: session_by_tool,
            yield_stats: YieldStats {
                shipped: yield_shipped,
                abandoned: yield_abandoned,
            },
        },
        tokens,
        buckets: filled,
        hourly,
        daily,
        calendar,
        calendar_start,
        calendar_end,
        summary,
    })
}

/// Fetch events once and compute overall activity stats and the per-repo
/// breakdown from the same snapshot, ensuring the two views are consistent.
pub fn compute_all(
    since_ts: u32,
    period_label: String,
    granularity: BucketGranularity,
    repo_filter: Option<&str>,
) -> Result<(LocalActivityStats, Vec<RepoActivitySummary>), GitAiError> {
    let records = fetch_metric_history(since_ts, repo_filter)?;
    let refs: Vec<&MetricHistoryRecord> = records.iter().collect();
    let stats = compute_activity_from_records(&refs, since_ts, period_label, granularity)?;
    let repos = repo_summaries_from_records(&records, since_ts, granularity)?;
    Ok((stats, repos))
}

/// Compute a per-repository breakdown for the given time window.
///
/// Fetches all matching events in a single DB query, groups them in memory by
/// `repo_url`, and aggregates each group — O(n) instead of O(n × repos).
/// Sorted by `ai_lines` descending.
pub fn compute_repo_summaries(
    since_ts: u32,
    granularity: BucketGranularity,
    repo_filter: Option<&str>,
) -> Result<Vec<RepoActivitySummary>, GitAiError> {
    let all_records = fetch_metric_history(since_ts, repo_filter)?;
    repo_summaries_from_records(&all_records, since_ts, granularity)
}
