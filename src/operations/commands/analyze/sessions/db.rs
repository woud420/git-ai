//! SQLite connection helpers, session/event persistence, and cross-cube
//! baseline enrichment for the `sessions` command.

use super::cube_fields::{
    EVENTS_CUBE, PR_SESSIONS_CUBE, SESSION_MODELS_CUBE, SESSIONS_CUBE, TOKEN_USAGE_CUBE, id_filter,
    qualify,
};
use super::schema::{DERIVED_COLUMNS, SCHEMA, SESSION_COLUMNS};
use super::value::{member_float, member_int, member_str, member_time, row_to_json_object};
use crate::operations::commands::analyze::cube::{CubeClient, QueryArgs};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Idempotently add the derived funnel-gap columns. Skips any that already
/// exist (so it is safe to run on every `open_db`, new DB or old).
pub(super) fn ensure_derived_columns(conn: &Connection) -> Result<(), rusqlite::Error> {
    for (name, sql_type, expr) in DERIVED_COLUMNS {
        let sql = format!(
            "ALTER TABLE sessions ADD COLUMN {} {} GENERATED ALWAYS AS ({}) VIRTUAL",
            name, sql_type, expr
        );
        match conn.execute(&sql, []) {
            Ok(_) => {}
            // Already present (re-opening an existing DB): nothing to do.
            Err(e) if e.to_string().contains("duplicate column name") => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub(super) fn open_db(path: &Path) -> Result<Connection, rusqlite::Error> {
    let conn = crate::model::repository::sqlite::open_with_memory_limits(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    conn.execute_batch(SCHEMA)?;
    ensure_derived_columns(&conn)?;
    Ok(conn)
}

pub(super) fn open_existing_db(path: &str) -> Result<Connection, String> {
    if !Path::new(path).exists() {
        return Err(format!(
            "no such db: {} (run `git-ai analyze sessions pull` first)",
            path
        ));
    }
    open_db(Path::new(path)).map_err(|e| e.to_string())
}

pub(super) fn init_cursor(conn: &Connection, target: u64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO cursor (name, fetched, target, pull_complete) VALUES ('default', 0, ?1, 0) \
         ON CONFLICT(name) DO UPDATE SET target=MAX(target, excluded.target), pull_complete=0",
        params![target as i64],
    )?;
    Ok(())
}

pub(super) fn get_fetched(conn: &Connection) -> Result<u64, rusqlite::Error> {
    let v: i64 = conn
        .query_row("SELECT fetched FROM cursor WHERE name='default'", [], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(0);
    Ok(v.max(0) as u64)
}

pub(super) fn set_fetched(conn: &Connection, fetched: u64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE cursor SET fetched=?1 WHERE name='default'",
        params![fetched as i64],
    )?;
    Ok(())
}

pub(super) fn set_pull_complete(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("UPDATE cursor SET pull_complete=1 WHERE name='default'", [])?;
    Ok(())
}

pub(super) fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Insert session rows (deduped by session_id). Returns count newly inserted.
pub(super) fn insert_sessions(conn: &Connection, rows: &[Value]) -> Result<usize, rusqlite::Error> {
    let now = now_secs();
    let mut inserted = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO sessions (
                session_id, user_id, agent, repo_url, parent_session_id,
                child_session_count, session_start_time, session_end_time,
                generated_lines, deleted_lines, net_generated_lines, generated_sloc,
                committed_lines, pr_opened_lines, merged_lines, production_lines,
                total_checkpoints, total_events, usage_minutes, created_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        )?;
        for row in rows {
            let session_id = match member_str(row, SESSIONS_CUBE, "session_id") {
                Some(id) if !id.is_empty() => id,
                _ => continue,
            };
            let n = stmt.execute(params![
                session_id,
                member_str(row, SESSIONS_CUBE, "user_id"),
                member_str(row, SESSIONS_CUBE, "agent"),
                member_str(row, SESSIONS_CUBE, "repo_url"),
                member_str(row, SESSIONS_CUBE, "parent_session_id"),
                member_int(row, SESSIONS_CUBE, "child_session_count"),
                member_time(row, SESSIONS_CUBE, "session_start_time"),
                member_time(row, SESSIONS_CUBE, "session_end_time"),
                member_int(row, SESSIONS_CUBE, "total_generated_lines"),
                member_int(row, SESSIONS_CUBE, "total_deleted_lines"),
                member_int(row, SESSIONS_CUBE, "net_generated_lines"),
                member_int(row, SESSIONS_CUBE, "total_generated_sloc"),
                member_int(row, SESSIONS_CUBE, "total_committed_lines"),
                member_int(row, SESSIONS_CUBE, "total_pr_opened_lines"),
                member_int(row, SESSIONS_CUBE, "total_merged_lines"),
                member_int(row, SESSIONS_CUBE, "total_production_lines"),
                member_int(row, SESSIONS_CUBE, "total_checkpoints"),
                member_int(row, SESSIONS_CUBE, "total_events"),
                member_int(row, SESSIONS_CUBE, "total_usage_minutes"),
                now,
            ])?;
            inserted += n;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

// ---------------------------------------------------------------------------
// Cross-cube baseline enrichment (tokens / cost / models / PRs)
// ---------------------------------------------------------------------------

/// Backfill the comparable baseline columns for a batch of freshly-pulled
/// sessions: per-session token usage + cost, the models each session used, and
/// the PRs it appears in. Best-effort — a failure in any one source warns but
/// does not abort the pull, since the core session rows are already persisted.
pub(super) fn enrich_sessions(client: &CubeClient, conn: &Connection, session_ids: &[String]) {
    if session_ids.is_empty() {
        return;
    }
    if let Err(e) = enrich_token_usage(client, conn, session_ids) {
        eprintln!("Warning: token-usage enrichment failed: {}", e);
    }
    if let Err(e) = enrich_models(client, conn, session_ids) {
        eprintln!("Warning: model enrichment failed: {}", e);
    }
    if let Err(e) = enrich_prs(client, conn, session_ids) {
        eprintln!("Warning: PR enrichment failed: {}", e);
    }
}

fn enrich_token_usage(
    client: &CubeClient,
    conn: &Connection,
    session_ids: &[String],
) -> Result<(), String> {
    let measures = qualify(
        TOKEN_USAGE_CUBE,
        &[
            "total_input_tokens",
            "total_output_tokens",
            "total_cache_read_tokens",
            "total_cache_creation_tokens",
            "total_reasoning_tokens",
            "total_cost",
        ],
    );
    let args = QueryArgs {
        measures,
        dimensions: vec![format!("{}.session_id", TOKEN_USAGE_CUBE)],
        filters_json: Some(id_filter(TOKEN_USAGE_CUBE, session_ids)),
        limit: Some(session_ids.len() as u64),
        ..Default::default()
    };
    let rows = client
        .load_rows(&args.to_query()?)
        .map_err(|e| e.to_string())?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "UPDATE sessions SET input_tokens=?2, output_tokens=?3, \
                 cache_read_tokens=?4, cache_creation_tokens=?5, reasoning_tokens=?6, \
                 cost_usd=?7 WHERE session_id=?1",
            )
            .map_err(|e| e.to_string())?;
        for row in &rows {
            let Some(sid) = member_str(row, TOKEN_USAGE_CUBE, "session_id") else {
                continue;
            };
            stmt.execute(params![
                sid,
                member_int(row, TOKEN_USAGE_CUBE, "total_input_tokens"),
                member_int(row, TOKEN_USAGE_CUBE, "total_output_tokens"),
                member_int(row, TOKEN_USAGE_CUBE, "total_cache_read_tokens"),
                member_int(row, TOKEN_USAGE_CUBE, "total_cache_creation_tokens"),
                member_int(row, TOKEN_USAGE_CUBE, "total_reasoning_tokens"),
                member_float(row, TOKEN_USAGE_CUBE, "total_cost"),
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

fn enrich_models(
    client: &CubeClient,
    conn: &Connection,
    session_ids: &[String],
) -> Result<(), String> {
    let args = QueryArgs {
        measures: vec![format!("{}.event_count", SESSION_MODELS_CUBE)],
        dimensions: vec![
            format!("{}.session_id", SESSION_MODELS_CUBE),
            format!("{}.model", SESSION_MODELS_CUBE),
        ],
        filters_json: Some(id_filter(SESSION_MODELS_CUBE, session_ids)),
        // A session can use several models; allow generous headroom per id.
        limit: Some((session_ids.len() as u64).saturating_mul(20).max(50)),
        ..Default::default()
    };
    let rows = client
        .load_rows(&args.to_query()?)
        .map_err(|e| e.to_string())?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO session_models (session_id, model, event_count) \
                 VALUES (?1,?2,?3)",
            )
            .map_err(|e| e.to_string())?;
        for row in &rows {
            let Some(sid) = member_str(row, SESSION_MODELS_CUBE, "session_id") else {
                continue;
            };
            let Some(model) = member_str(row, SESSION_MODELS_CUBE, "model") else {
                continue;
            };
            stmt.execute(params![
                sid,
                model,
                member_int(row, SESSION_MODELS_CUBE, "event_count"),
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    recompute_models(&tx).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn enrich_prs(
    client: &CubeClient,
    conn: &Connection,
    session_ids: &[String],
) -> Result<(), String> {
    let args = QueryArgs {
        measures: vec![format!("{}.total_ai_lines", PR_SESSIONS_CUBE)],
        dimensions: qualify(
            PR_SESSIONS_CUBE,
            &["session_id", "repo_url", "pr_number", "agent", "model_raw"],
        ),
        filters_json: Some(id_filter(PR_SESSIONS_CUBE, session_ids)),
        // A session can land in several PRs; allow generous headroom per id.
        limit: Some((session_ids.len() as u64).saturating_mul(20).max(50)),
        ..Default::default()
    };
    let rows = client
        .load_rows(&args.to_query()?)
        .map_err(|e| e.to_string())?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO session_prs \
                 (session_id, repo_url, pr_number, agent, model_raw, ai_lines) \
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )
            .map_err(|e| e.to_string())?;
        for row in &rows {
            let Some(sid) = member_str(row, PR_SESSIONS_CUBE, "session_id") else {
                continue;
            };
            let Some(pr) = member_int(row, PR_SESSIONS_CUBE, "pr_number") else {
                continue;
            };
            stmt.execute(params![
                sid,
                // repo_url is part of the PK; coalesce to '' so NULLs don't
                // produce duplicate rows for the same PR.
                member_str(row, PR_SESSIONS_CUBE, "repo_url").unwrap_or_default(),
                pr,
                member_str(row, PR_SESSIONS_CUBE, "agent"),
                member_str(row, PR_SESSIONS_CUBE, "model_raw"),
                member_int(row, PR_SESSIONS_CUBE, "total_ai_lines"),
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    recompute_pr_counts(&tx).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// Denormalize the distinct models per session into `sessions.models` (a
/// comma-joined, alphabetically-ordered list) for quick filtering.
pub(super) fn recompute_models(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE sessions SET models = (
             SELECT group_concat(model, ',') FROM (
                 SELECT model FROM session_models sm
                 WHERE sm.session_id = sessions.session_id ORDER BY model
             )
         ) WHERE session_id IN (SELECT DISTINCT session_id FROM session_models)",
        [],
    )?;
    Ok(())
}

/// Denormalize the count of distinct PRs per session into `sessions.pr_count`.
pub(super) fn recompute_pr_counts(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE sessions SET pr_count = (
             SELECT COUNT(*) FROM session_prs sp WHERE sp.session_id = sessions.session_id
         ) WHERE session_id IN (SELECT DISTINCT session_id FROM session_prs)",
        [],
    )?;
    Ok(())
}

pub(super) fn has_events(conn: &Connection, session_id: &str) -> Result<bool, rusqlite::Error> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM session_events WHERE session_id=?1",
        params![session_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub(super) fn insert_events(
    conn: &Connection,
    session_id: &str,
    rows: &[Value],
) -> Result<usize, rusqlite::Error> {
    let mut inserted = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO session_events (
                session_id, output_seq, event_time, event_kind, tool, tool_kind,
                model, target, text, summary, tool_input, tool_output
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )?;
        for row in rows {
            let n = stmt.execute(params![
                session_id,
                member_int(row, EVENTS_CUBE, "output_seq"),
                member_time(row, EVENTS_CUBE, "event_time"),
                // event_kind/tool are part of the dedup key; coalesce to '' so
                // re-fetching is idempotent (SQLite treats NULLs as distinct).
                member_str(row, EVENTS_CUBE, "event_kind").unwrap_or_default(),
                member_str(row, EVENTS_CUBE, "tool").unwrap_or_default(),
                member_str(row, EVENTS_CUBE, "tool_kind"),
                member_str(row, EVENTS_CUBE, "model"),
                member_str(row, EVENTS_CUBE, "target"),
                member_str(row, EVENTS_CUBE, "text"),
                member_str(row, EVENTS_CUBE, "summary"),
                member_str(row, EVENTS_CUBE, "tool_input"),
                member_str(row, EVENTS_CUBE, "tool_output"),
            ])?;
            inserted += n;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

/// Read one session row as a JSON object keyed by the public column names.
pub(super) fn session_row_json(
    conn: &Connection,
    session_id: &str,
) -> Result<Map<String, Value>, rusqlite::Error> {
    let sql = format!(
        "SELECT {} FROM sessions WHERE session_id=?1",
        SESSION_COLUMNS.join(", ")
    );
    conn.query_row(&sql, params![session_id], |row| {
        row_to_json_object(row, SESSION_COLUMNS)
    })
}

/// Read a session's transcript from `session_events`, ordered chronologically.
pub(super) fn transcript_json(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<Value>, rusqlite::Error> {
    let cols = [
        "output_seq",
        "event_time",
        "event_kind",
        "tool",
        "tool_kind",
        "model",
        "target",
        "text",
        "summary",
        "tool_input",
        "tool_output",
    ];
    let sql = format!(
        "SELECT {} FROM session_events WHERE session_id=?1 \
         ORDER BY event_time ASC, output_seq ASC, id ASC",
        cols.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(Value::Object(row_to_json_object(row, &cols)?))
    })?;
    rows.collect()
}

/// Read the PRs a session appears in from `session_prs`, ordered by PR number.
pub(super) fn session_prs_json(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<Value>, rusqlite::Error> {
    let cols = ["repo_url", "pr_number", "agent", "model_raw", "ai_lines"];
    let sql = format!(
        "SELECT {} FROM session_prs WHERE session_id=?1 ORDER BY repo_url, pr_number",
        cols.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(Value::Object(row_to_json_object(row, &cols)?))
    })?;
    rows.collect()
}

pub(super) fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
