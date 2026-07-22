use super::aggregation;
use super::repo_summaries;
use super::*;
use crate::metrics::attrs::EventAttributes;
use crate::metrics::events::{CheckpointValues, CommittedValues, SessionEventValues};
use crate::metrics::pos_encoded::{PosEncoded, sparse_get_string};
use crate::metrics::types::MetricEvent;
use serde_json::json;

fn now_ts() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32
}

fn attrs(
    repo_url: Option<&str>,
    tool: &str,
    session_id: Option<&str>,
) -> crate::metrics::types::SparseArray {
    let mut attrs = EventAttributes::with_version("test").tool(tool);
    if let Some(repo_url) = repo_url {
        attrs = attrs.repo_url(repo_url);
    }
    if let Some(session_id) = session_id {
        attrs = attrs.session_id(session_id);
    }
    attrs.to_sparse()
}

fn record(event: MetricEvent) -> MetricHistoryRecord {
    use crate::metrics::attrs::attr_pos;
    let repo_url = sparse_get_string(&event.attrs, attr_pos::REPO_URL).flatten();
    MetricHistoryRecord {
        event_id: event.event_id,
        ts: event.timestamp,
        repo_url,
        event,
    }
}

fn committed(ts: u32, repo_url: &str, ai: u32, human: u32, diff_added: u32) -> MetricHistoryRecord {
    let values = CommittedValues::new()
        .human_additions(human)
        .git_diff_added_lines(diff_added)
        .tool_model_pairs(vec![
            "all".to_string(),
            "claude::claude-sonnet-4-6".to_string(),
        ])
        .ai_additions(vec![ai, ai]);
    record(MetricEvent::with_timestamp(
        ts,
        &values,
        attrs(Some(repo_url), "claude", None),
    ))
}

fn checkpoint(ts: u32, repo_url: &str, lines_added: u32) -> MetricHistoryRecord {
    let values = CheckpointValues::new()
        .kind("ai_agent")
        .file_path("src/main.rs")
        .lines_added(lines_added);
    record(MetricEvent::with_timestamp(
        ts,
        &values,
        attrs(Some(repo_url), "claude", Some("session-1")),
    ))
}

fn claude_session(ts: u32, repo_url: Option<&str>, session_id: &str) -> MetricHistoryRecord {
    let values = SessionEventValues::new(json!({
        "message": {
            "id": "msg-1",
            "role": "assistant",
            "model": "claude-sonnet-4-6-20250101",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10
            }
        }
    }));
    record(MetricEvent::with_timestamp(
        ts,
        &values,
        attrs(repo_url, "claude", Some(session_id)),
    ))
}

#[test]
fn compute_activity_aggregates_commits_checkpoints_sessions_and_tokens() {
    let now = now_ts();
    let repo = "github.com/acme/project";
    let session_ts = now.saturating_sub(600);
    let commit_ts = now.saturating_sub(300);
    let records = [
        claude_session(session_ts, Some(repo), "session-1"),
        checkpoint(session_ts + 10, repo, 12),
        committed(commit_ts, repo, 10, 2, 12),
    ];
    let refs: Vec<&MetricHistoryRecord> = records.iter().collect();

    let stats = compute_activity_from_records(
        &refs,
        now.saturating_sub(24 * 3600),
        "last 1 day".to_string(),
        BucketGranularity::Daily,
    )
    .unwrap();

    assert_eq!(stats.commits.total, 1);
    assert_eq!(stats.commits.ai_lines, 10);
    assert_eq!(stats.commits.human_lines, 2);
    assert_eq!(stats.commits.diff_added_lines, 12);
    assert_eq!(
        stats.commits.by_tool,
        vec![("claude · sonnet-4-6".to_string(), 10)]
    );
    assert_eq!(
        stats.commits.acceptance_by_tool,
        vec![("claude".to_string(), 83)]
    );
    assert_eq!(stats.checkpoints.total, 1);
    assert_eq!(stats.checkpoints.ai_lines_added, 12);
    assert_eq!(stats.checkpoints.files_edited, 1);
    assert_eq!(stats.sessions.total, 1);
    assert_eq!(stats.sessions.yield_stats.shipped, 1);
    assert_eq!(stats.sessions.yield_stats.abandoned, 0);
    assert_eq!(stats.tokens.input, 100);
    assert_eq!(stats.tokens.output, 50);
    assert_eq!(stats.tokens.cache_read, 20);
    assert_eq!(stats.tokens.cache_creation, 10);
    assert_eq!(stats.tokens.by_model[0].model, "claude-sonnet-4-6");
    assert!(stats.buckets.iter().any(|bucket| bucket.ai_lines == 10));
}

fn day(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn streaks_empty_is_zero() {
    use std::collections::BTreeMap;
    let days: BTreeMap<chrono::NaiveDate, u32> = BTreeMap::new();
    assert_eq!(
        aggregation::compute_streaks(&days, day(2026, 4, 10)),
        (0, 0)
    );
}

#[test]
fn streaks_longest_breaks_on_gap_and_current_requires_recency() {
    use std::collections::BTreeMap;
    // Three-day run, a gap, then a two-day run ending "today".
    let mut days: BTreeMap<chrono::NaiveDate, u32> = BTreeMap::new();
    for d in [1u32, 2, 3, 7, 8] {
        days.insert(day(2026, 4, d), 5);
    }
    let today = day(2026, 4, 8);
    let (longest, current) = aggregation::compute_streaks(&days, today);
    assert_eq!(longest, 3);
    assert_eq!(current, 2);
}

#[test]
fn streaks_current_zero_when_trailing_run_is_stale() {
    use std::collections::BTreeMap;
    let mut days: BTreeMap<chrono::NaiveDate, u32> = BTreeMap::new();
    for d in [1u32, 2, 3] {
        days.insert(day(2026, 4, d), 5);
    }
    // Today is well past the last active day → current streak is 0.
    let (longest, current) = aggregation::compute_streaks(&days, day(2026, 4, 20));
    assert_eq!(longest, 3);
    assert_eq!(current, 0);
}

#[test]
fn streaks_current_counts_with_yesterday_grace() {
    use std::collections::BTreeMap;
    let mut days: BTreeMap<chrono::NaiveDate, u32> = BTreeMap::new();
    for d in [4u32, 5, 6] {
        days.insert(day(2026, 4, d), 5);
    }
    // Last active day is yesterday relative to today → still counts.
    let (longest, current) = aggregation::compute_streaks(&days, day(2026, 4, 7));
    assert_eq!(longest, 3);
    assert_eq!(current, 3);
}

#[test]
fn derived_summary_from_records() {
    use chrono::{Local, TimeZone};
    let now = Local
        .from_local_datetime(
            &Local::now()
                .date_naive()
                .and_hms_opt(12, 0, 0)
                .expect("local noon should exist"),
        )
        .single()
        .expect("local noon should be unambiguous")
        .timestamp() as u32;
    let repo = "github.com/acme/project";
    // Two sessions: one spanning ~1h, one a single event.
    let session_start = now.saturating_sub(7200);
    let records = [
        claude_session(session_start, Some(repo), "session-1"),
        claude_session(session_start + 3600, Some(repo), "session-1"),
        claude_session(now.saturating_sub(60), Some(repo), "session-2"),
        committed(now.saturating_sub(300), repo, 40, 0, 40),
        committed(now.saturating_sub(120), repo, 10, 0, 10),
    ];
    let refs: Vec<&MetricHistoryRecord> = records.iter().collect();

    let stats = compute_activity_from_records(
        &refs,
        now.saturating_sub(24 * 3600),
        "last 1 day".to_string(),
        BucketGranularity::Daily,
    )
    .unwrap();

    // Both commits land on the same local day → one active day, 50 AI lines.
    assert_eq!(stats.summary.active_days, 1);
    assert_eq!(stats.calendar.len(), 1);
    assert_eq!(stats.calendar[0].ai_lines, 50);
    assert_eq!(stats.summary.total_days, 1);
    assert_eq!(stats.summary.longest_streak, 1);
    assert_eq!(stats.summary.current_streak, 1);
    let most = stats.summary.most_active_day.as_ref().unwrap();
    assert_eq!(most.ai_lines, 50);
    // Longest session spans the two session-1 events (~3600s); session-2 is 0.
    assert_eq!(stats.summary.longest_session_secs, 3600);
    assert_eq!(
        stats.summary.favorite_model.as_deref(),
        Some("claude-sonnet-4-6")
    );
}

#[test]
fn repo_summaries_group_records_by_repo_and_skip_unknown_repo() {
    let now = now_ts();
    let repo = "github.com/acme/project";
    let records = [
        committed(now.saturating_sub(300), repo, 8, 0, 8),
        claude_session(now.saturating_sub(200), Some(repo), "session-1"),
        claude_session(now.saturating_sub(100), None, "session-unknown"),
    ];

    let summaries = repo_summaries::repo_summaries_from_records(
        &records,
        now.saturating_sub(24 * 3600),
        BucketGranularity::Daily,
    )
    .unwrap();

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].repo_url, repo);
    assert_eq!(summaries[0].ai_lines, 8);
    assert_eq!(summaries[0].commits, 1);
    assert_eq!(summaries[0].sessions, 1);
    assert!(summaries[0].estimated_cost_usd > 0.0);
}
