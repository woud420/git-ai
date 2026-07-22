//! Public data types for `git-ai usage` output.

use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LocalActivityStats {
    pub period_label: String,
    pub commits: CommitSummary,
    pub checkpoints: CheckpointSummary,
    pub sessions: SessionSummary,
    pub tokens: TokenSummary,
    /// Activity bucketed by day/week/month depending on period.
    pub buckets: Vec<BucketStats>,
    /// AI lines committed per hour of day (local time), 24 elements.
    pub hourly: Vec<u32>,
    /// AI lines committed per day of week (local time), 7 elements: Mon=0 … Sun=6.
    pub daily: Vec<u32>,
    /// AI lines committed per calendar day (local time), sparse — only days with
    /// AI activity are present. Drives the contribution-calendar heatmap.
    pub calendar: Vec<DayActivity>,
    /// First day rendered in the calendar grid (window start, or earliest activity
    /// for the all-time window).
    pub calendar_start: NaiveDate,
    /// Last day rendered in the calendar grid (today, local time).
    pub calendar_end: NaiveDate,
    /// Derived headline stats for the compact summary block.
    pub summary: ActivitySummary,
}

/// AI lines committed on a single local calendar day.
#[derive(Debug, Clone, Serialize)]
pub struct DayActivity {
    pub date: NaiveDate,
    pub ai_lines: u32,
    /// Estimated token spend (USD) on this day, summed across models with known
    /// pricing. Lines and spend diverge — a low-lines day can still be expensive.
    #[serde(default)]
    pub estimated_cost_usd: f64,
}

/// Derived headline statistics for the `git-ai usage` summary block.
#[derive(Debug, Default, Serialize)]
pub struct ActivitySummary {
    /// Distinct local days with AI activity in the window.
    pub active_days: u32,
    /// Days from the first active day through today, inclusive.
    pub total_days: u32,
    /// Longest run of consecutive active days.
    pub longest_streak: u32,
    /// Trailing run of consecutive active days, counted only when it reaches
    /// today or yesterday (one-day grace).
    pub current_streak: u32,
    /// Day with the most AI lines (earliest wins ties).
    pub most_active_day: Option<DayActivity>,
    /// Longest session duration (last event − first event) in seconds.
    pub longest_session_secs: u32,
    /// Top model by total tokens (already shortened). None when no token data.
    pub favorite_model: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct TokenSummary {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    /// Estimated cost in USD, summed across models with known pricing.
    pub estimated_cost_usd: f64,
    /// Per-model breakdown, sorted by total tokens descending.
    pub by_model: Vec<TokenModelStat>,
    /// Week-over-week spend comparison (current 7 days vs previous 7 days).
    /// None when either week has no cost data (e.g. viewing a period < 14 days
    /// or when pricing is unavailable for all models).
    pub wow_spend: Option<WowSpend>,
}

/// Week-over-week spend comparison.
#[derive(Debug, Serialize)]
pub struct WowSpend {
    pub this_week_usd: f64,
    pub last_week_usd: f64,
    /// Percentage change: positive = up, negative = down. None when last week
    /// was zero and this week has spend.
    pub change_pct: Option<f64>,
    pub new_this_week: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct TokenModelStat {
    pub model: String,
    pub sessions: u32,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    /// Estimated cost in USD; None if the model has no pricing entry.
    pub estimated_cost_usd: Option<f64>,
    /// Cache hit ratio: cache_read / (cache_read + cache_creation), 0.0–1.0.
    /// None when neither cache_read nor cache_creation is non-zero (model
    /// doesn't use prompt caching, e.g. codex).
    pub cache_hit_ratio: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct BucketStats {
    pub label: String,
    pub ai_lines: u32,
    pub commit_count: u32,
    /// Total git diff additions in this bucket (across all commits).
    pub diff_added_lines: u32,
    /// Lines attributed to AI or known-human in this bucket.
    pub attributed_lines: u32,
}

#[derive(Debug, Serialize)]
pub struct CommitSummary {
    /// Commits that include at least one AI-attributed line. Human-only commits
    /// are not counted here; use the diff/human stats for full commit coverage.
    pub total: u32,
    pub ai_lines: u32,
    pub human_lines: u32,
    /// Total lines added across all commits (git diff additions), used to
    /// measure attribution coverage: lines not attributed to AI or known-human
    /// are "untracked" holes in the data.
    pub diff_added_lines: u32,
    /// Per-tool AI line counts (tool · model label), sorted descending.
    pub by_tool: Vec<(String, u32)>,
    /// Per-tool acceptance rate: committed AI lines / checkpoint AI lines, as a
    /// percentage. Values >100 indicate incomplete checkpoint data (e.g. events
    /// recorded before the repo_url backfill). Sorted by tool name.
    pub acceptance_by_tool: Vec<(String, u32)>,
}

#[derive(Debug, Serialize)]
pub struct CheckpointSummary {
    pub total: u32,
    pub ai_lines_added: u32,
    pub human_lines_added: u32,
    pub files_edited: u32,
}

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub total: u32,
    pub by_tool: Vec<(String, u32)>,
    pub yield_stats: YieldStats,
}

/// Classifies sessions by whether they were followed by a commit within
/// a short window — a proxy for "did this AI session actually ship work?"
#[derive(Debug, Default, Serialize)]
pub struct YieldStats {
    /// Sessions followed by at least one commit within `YIELD_WINDOW_SECS`.
    pub shipped: u32,
    /// Sessions with no commit found within the window.
    pub abandoned: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BucketGranularity {
    Daily,
    Weekly,
    Monthly,
}

/// Summary of activity for a single repository.
#[derive(Debug, Serialize)]
pub struct RepoActivitySummary {
    /// Normalised repository URL (e.g. `github.com/org/repo`).
    pub repo_url: String,
    pub ai_lines: u32,
    pub commits: u32,
    pub sessions: u32,
    pub estimated_cost_usd: f64,
}
