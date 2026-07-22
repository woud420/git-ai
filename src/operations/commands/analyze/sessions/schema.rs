//! SQLite schema definition and column metadata for the `sessions` scratch DB.

/// DDL for the sessions scratch database. Applied on every `open_db`, so old
/// DBs gain new tables/indexes transparently on the next open.
pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    seq_id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL UNIQUE,
    user_id TEXT,
    agent TEXT,
    repo_url TEXT,
    parent_session_id TEXT,
    child_session_count INTEGER,
    session_start_time INTEGER,
    session_end_time INTEGER,
    generated_lines INTEGER,
    deleted_lines INTEGER,
    net_generated_lines INTEGER,
    generated_sloc INTEGER,
    -- Funnel stages, in order: every line a session committed, then how many of
    -- those reached a PR, got merged, and finally landed in production. The
    -- stage-to-stage "gap" columns (committed_not_pr_opened, …) are derived from
    -- these by `ensure_derived_columns` (see DERIVED_COLUMNS) so an analyst never
    -- has to hand-compute "committed but didn't ship".
    committed_lines INTEGER,
    pr_opened_lines INTEGER,
    merged_lines INTEGER,
    production_lines INTEGER,
    total_checkpoints INTEGER,
    total_events INTEGER,
    usage_minutes INTEGER,
    -- Cross-cube baseline, backfilled at pull time (best-effort) so every
    -- session carries the same comparable columns without re-querying:
    --   models      : comma-joined distinct models used (see session_models)
    --   *_tokens    : per-session token usage (public_v1_token_usage)
    --   cost_usd    : per-session estimated cost (public_v1_token_usage)
    --   pr_count    : how many PRs this session appears in (see session_prs)
    models TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    cache_read_tokens INTEGER,
    cache_creation_tokens INTEGER,
    reasoning_tokens INTEGER,
    cost_usd REAL,
    pr_count INTEGER,
    created_at INTEGER NOT NULL
);

-- One row per (session, model). Backfilled from public_v1_session_models; the
-- distinct models are also denormalized into sessions.models for quick filters.
CREATE TABLE IF NOT EXISTS session_models (
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    model TEXT NOT NULL,
    event_count INTEGER,
    PRIMARY KEY (session_id, model)
);
CREATE INDEX IF NOT EXISTS idx_session_models_sid ON session_models(session_id);

-- One row per (session, PR) the session contributed to. Backfilled from
-- public_v1_pr_sessions; the count is denormalized into sessions.pr_count.
CREATE TABLE IF NOT EXISTS session_prs (
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    repo_url TEXT NOT NULL DEFAULT '',
    pr_number INTEGER NOT NULL,
    agent TEXT,
    model_raw TEXT,
    ai_lines INTEGER,
    PRIMARY KEY (session_id, repo_url, pr_number)
);
CREATE INDEX IF NOT EXISTS idx_session_prs_sid ON session_prs(session_id);

CREATE TABLE IF NOT EXISTS session_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    output_seq INTEGER,
    event_time INTEGER,
    event_kind TEXT,
    tool TEXT,
    tool_kind TEXT,
    model TEXT,
    target TEXT,
    text TEXT,
    summary TEXT,
    tool_input TEXT,
    tool_output TEXT,
    UNIQUE(session_id, output_seq, event_time, event_kind, tool)
);
CREATE INDEX IF NOT EXISTS idx_session_events_sid ON session_events(session_id);

-- Two cursors live here:
--   fetched/target/pull_complete : ingestion progress paging through Cube (pull)
--   analyzed_seq                 : the analysis cursor — the highest sessions.seq_id
--                                  handed out by `next`. `next` advances it atomically
--                                  so every row is served exactly once; `reset` sets it
--                                  back to 0 to start a fresh analysis pass.
CREATE TABLE IF NOT EXISTS cursor (
    name TEXT PRIMARY KEY DEFAULT 'default',
    fetched INTEGER NOT NULL DEFAULT 0,
    target INTEGER,
    pull_complete INTEGER NOT NULL DEFAULT 0,
    analyzed_seq INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT
);
"#;

/// Session columns selected for output, in display order.
pub(super) const SESSION_COLUMNS: &[&str] = &[
    "seq_id",
    "session_id",
    "user_id",
    "agent",
    "repo_url",
    "parent_session_id",
    "child_session_count",
    "session_start_time",
    "session_end_time",
    "generated_lines",
    "deleted_lines",
    "net_generated_lines",
    "generated_sloc",
    "committed_lines",
    "pr_opened_lines",
    "merged_lines",
    "production_lines",
    // Derived funnel gaps (generated columns — see DERIVED_COLUMNS).
    "committed_not_pr_opened",
    "pr_opened_not_merged",
    "merged_not_production",
    "committed_not_production",
    "production_rate",
    "total_checkpoints",
    "total_events",
    "usage_minutes",
    "models",
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_creation_tokens",
    "reasoning_tokens",
    "cost_usd",
    "pr_count",
];

/// Derived "funnel gap" columns, computed straight from the raw stage measures
/// (`committed_lines` → `pr_opened_lines` → `merged_lines` → `production_lines`)
/// so an analyst can ask "what didn't ship, and where did it fall out?" with a
/// plain SELECT — no hand arithmetic, no reaching for transcripts. Each gap is
/// "lines that reached this stage but not the next"; `committed_not_production`
/// is the headline "committed but never shipped" number, and `production_rate`
/// is the share of committed work that landed in production. NULL bases are
/// coalesced to 0 so the gaps are always clean integers. The *why* behind a gap
/// (still-open PR vs. reverted vs. superseded) is NOT here — that genuinely
/// needs the transcript.
///
/// `(name, sql_type, expression)`. Added as VIRTUAL generated columns, which
/// (unlike STORED) can be introduced via `ALTER TABLE ADD COLUMN`, so old
/// scratch DBs gain them on the next open too.
pub(super) const DERIVED_COLUMNS: &[(&str, &str, &str)] = &[
    (
        "committed_not_pr_opened",
        "INTEGER",
        "COALESCE(committed_lines, 0) - COALESCE(pr_opened_lines, 0)",
    ),
    (
        "pr_opened_not_merged",
        "INTEGER",
        "COALESCE(pr_opened_lines, 0) - COALESCE(merged_lines, 0)",
    ),
    (
        "merged_not_production",
        "INTEGER",
        "COALESCE(merged_lines, 0) - COALESCE(production_lines, 0)",
    ),
    (
        "committed_not_production",
        "INTEGER",
        "COALESCE(committed_lines, 0) - COALESCE(production_lines, 0)",
    ),
    (
        "production_rate",
        "REAL",
        "CAST(COALESCE(production_lines, 0) AS REAL) / NULLIF(committed_lines, 0)",
    ),
];
