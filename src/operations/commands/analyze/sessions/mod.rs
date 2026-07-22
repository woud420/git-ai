//! `git-ai analyze sessions …` — pull coding sessions from Cube into a scratch
//! SQLite DB, then hand them out one at a time for grading at scale.
//!
//! Two cursors live in the DB, both in the `cursor` table: a **pull cursor**
//! (`fetched`/`target`/`pull_complete` — how far we've paged through Cube) and
//! an **analysis cursor** (`analyzed_seq` — the highest `sessions.seq_id` handed
//! out). `sessions next` advances the analysis cursor by exactly one row inside
//! an `IMMEDIATE` transaction, so concurrent subagents each get a distinct
//! session exactly once with no dependency on anyone marking it complete; it
//! then fetches and persists that session's transcript into `session_events`
//! and returns it inline. `reset` rewinds the analysis cursor to re-run.

mod cube_fields;
mod db;
mod help;
mod schema;
mod value;

#[cfg(test)]
mod tests;

use super::cube::{CubeClient, QueryArgs};
use super::take_value;
use cube_fields::{
    EVENTS_CUBE, SESSIONS_CUBE, equals_filters, event_dimensions, merge_filters,
    session_dimensions, session_measures,
};
use db::{
    enrich_sessions, get_fetched, has_events, init_cursor, insert_events, insert_sessions,
    now_secs, open_db, open_existing_db, session_prs_json, session_row_json, set_fetched, set_meta,
    set_pull_complete, transcript_json,
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::path::PathBuf;
use value::{member_str, value_ref_to_string};

const DEFAULT_PULL_LIMIT: u64 = 100;
const PULL_BATCH: u64 = 50;
const DEFAULT_MAX_EVENTS: u64 = 2000;

pub fn handle_sessions(args: &[String]) {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };
    // `--help`/`-h` anywhere prints help, exactly like running `sessions` raw.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        help::print_help();
        return;
    }
    let result = match sub {
        "pull" => cmd_pull(rest),
        "next" => cmd_next(rest),
        "stats" => cmd_stats(rest),
        "reset" => cmd_reset(rest),
        "exec" => cmd_exec(rest),
        "help" | "--help" | "-h" | "" => {
            help::print_help();
            return;
        }
        other => Err(format!(
            "unknown sessions subcommand: {}\nRun `git-ai analyze sessions --help`.",
            other
        )),
    };
    if let Err(msg) = result {
        eprintln!("Error: {}", msg);
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// pull
// ---------------------------------------------------------------------------

fn cmd_pull(args: &[String]) -> Result<(), String> {
    let mut db_path: Option<String> = None;
    let mut limit = DEFAULT_PULL_LIMIT;
    let mut since: Option<String> = None;
    let mut repo: Option<String> = None;
    let mut user: Option<String> = None;
    let mut agent: Option<String> = None;
    // Default to top-level sessions only: a session with a parent is a subagent.
    let mut include_subagents = false;
    // Backfill the cross-cube baseline columns (tokens/cost/models/PRs) by
    // default; --no-enrich skips the extra per-batch queries.
    let mut enrich = true;
    // Query-shaping flags, identical to `analyze query`: these layer extra
    // dimensions/measures/filters/order on top of the canonical session columns
    // so you can slice the session population on ANY Cube member.
    let mut extra_measures: Vec<String> = Vec::new();
    let mut extra_dimensions: Vec<String> = Vec::new();
    let mut time_dimension: Option<String> = None;
    let mut granularity: Option<String> = None;
    let mut filters_json: Option<String> = None;
    let mut order: Vec<(String, String)> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => db_path = Some(take_value(args, &mut i, "--db")?),
            "--limit" => {
                limit = take_value(args, &mut i, "--limit")?
                    .parse()
                    .map_err(|_| "--limit must be a number".to_string())?
            }
            "--since" | "--date-range" => since = Some(take_value(args, &mut i, "--since")?),
            "--repo" => repo = Some(take_value(args, &mut i, "--repo")?),
            "--user" => user = Some(take_value(args, &mut i, "--user")?),
            "--agent" => agent = Some(take_value(args, &mut i, "--agent")?),
            "--include-subagents" => include_subagents = true,
            "--no-enrich" => enrich = false,
            "--measures" | "-m" => {
                extra_measures.extend(super::split_csv(&take_value(args, &mut i, "--measures")?));
            }
            "--dimensions" | "-d" => {
                extra_dimensions.extend(super::split_csv(&take_value(
                    args,
                    &mut i,
                    "--dimensions",
                )?));
            }
            "--time-dimension" | "--td" => {
                time_dimension = Some(take_value(args, &mut i, "--time-dimension")?);
            }
            "--granularity" | "-g" => {
                granularity = Some(take_value(args, &mut i, "--granularity")?);
            }
            "--filters" | "-f" => {
                filters_json = Some(take_value(args, &mut i, "--filters")?);
            }
            "--order" | "-o" => {
                order.push(super::parse_order(&take_value(args, &mut i, "--order")?)?);
            }
            // `--help`/`-h` is handled by `handle_sessions` before dispatch.
            other => return Err(format!("unknown pull flag: {}", other)),
        }
        i += 1;
    }

    let path = match db_path {
        Some(p) => PathBuf::from(p),
        None => random_db_path(),
    };
    let client = CubeClient::from_config().map_err(|e| e.to_string())?;
    let conn = open_db(&path).map_err(|e| e.to_string())?;
    init_cursor(&conn, limit).map_err(|e| e.to_string())?;

    // Record provenance.
    set_meta(&conn, "created_at", &now_secs().to_string()).ok();
    set_meta(
        &conn,
        "pull_filters",
        &json!({
            "since": since, "repo": repo, "user": user, "agent": agent,
            "limit": limit, "include_subagents": include_subagents,
            "filters": filters_json, "dimensions": extra_dimensions,
            "measures": extra_measures, "time_dimension": time_dimension,
            "granularity": granularity,
            "order": order.iter().map(|(m, d)| format!("{}:{}", m, d)).collect::<Vec<_>>(),
        })
        .to_string(),
    )
    .ok();

    // The canonical session columns are ALWAYS selected so the SQLite schema
    // stays fully populated; the caller's --dimensions/--measures layer on top.
    let mut dimensions = session_dimensions();
    for d in extra_dimensions {
        if !dimensions.contains(&d) {
            dimensions.push(d);
        }
    }
    let mut measures = session_measures();
    for m in extra_measures {
        if !measures.contains(&m) {
            measures.push(m);
        }
    }

    // Convenience equals-filters (subagent/repo/user/agent) merge with any raw
    // --filters array so both work together and you can slice on ANY member. A
    // non-empty parent_session_id means a subagent session; '' (not NULL) is a
    // top-level user session, so we exclude subagents by default.
    let parent_filter = if include_subagents { None } else { Some("") };
    let convenience = equals_filters(&[
        (
            format!("{}.parent_session_id", SESSIONS_CUBE),
            parent_filter,
        ),
        (format!("{}.repo_url", SESSIONS_CUBE), repo.as_deref()),
        (format!("{}.user_id", SESSIONS_CUBE), user.as_deref()),
        (format!("{}.agent", SESSIONS_CUBE), agent.as_deref()),
    ]);
    let filters = merge_filters(convenience, filters_json.as_deref())?;

    // A --since dateRange needs a time dimension; default to session_start_time
    // unless the caller named their own. Ordering defaults to newest-first but
    // an explicit --order wins.
    let time_dimension = time_dimension.or_else(|| {
        since
            .as_ref()
            .map(|_| format!("{}.session_start_time", SESSIONS_CUBE))
    });
    let order = if order.is_empty() {
        vec![(
            format!("{}.session_start_time", SESSIONS_CUBE),
            "desc".into(),
        )]
    } else {
        order
    };

    let mut fetched: u64 = get_fetched(&conn).map_err(|e| e.to_string())?;
    let mut stored_now = 0usize;

    while fetched < limit {
        let batch = PULL_BATCH.min(limit - fetched);
        let args = QueryArgs {
            measures: measures.clone(),
            dimensions: dimensions.clone(),
            time_dimension: time_dimension.clone(),
            granularity: granularity.clone(),
            date_range: since.clone(),
            filters_json: filters.clone(),
            order: order.clone(),
            limit: Some(batch),
            offset: Some(fetched),
        };
        let query = args.to_query()?;
        let rows = client.load_rows(&query).map_err(|e| e.to_string())?;
        let returned = rows.len() as u64;
        stored_now += insert_sessions(&conn, &rows).map_err(|e| e.to_string())?;
        // Backfill the cross-cube baseline columns for this batch's sessions.
        if enrich {
            let batch_ids: Vec<String> = rows
                .iter()
                .filter_map(|r| member_str(r, SESSIONS_CUBE, "session_id"))
                .collect();
            enrich_sessions(&client, &conn, &batch_ids);
        }
        fetched += returned;
        set_fetched(&conn, fetched).map_err(|e| e.to_string())?;
        // Exhausted: cube returned fewer rows than asked for.
        if returned < batch {
            set_pull_complete(&conn).map_err(|e| e.to_string())?;
            break;
        }
    }
    if fetched >= limit {
        set_pull_complete(&conn).map_err(|e| e.to_string())?;
    }

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    eprintln!(
        "Pulled {} new session(s); {} total in db.",
        stored_now, total
    );
    // The DB path goes to stdout so callers can capture it (`DB=$(… pull)`).
    println!("{}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// next
// ---------------------------------------------------------------------------

fn cmd_next(args: &[String]) -> Result<(), String> {
    let mut db_path: Option<String> = None;
    let mut max_events = DEFAULT_MAX_EVENTS;
    let mut with_transcript = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--max-events" => {
                max_events = take_value(args, &mut i, "--max-events")?
                    .parse()
                    .map_err(|_| "--max-events must be a number".to_string())?
            }
            "--no-transcript" => with_transcript = false,
            // `--help`/`-h` is handled by `handle_sessions` before dispatch.
            other if !other.starts_with('-') && db_path.is_none() => {
                db_path = Some(other.to_string())
            }
            other => return Err(format!("unknown next argument: {}", other)),
        }
        i += 1;
    }
    let path = db_path.ok_or("usage: git-ai analyze sessions next <db> [--max-events N]")?;
    let mut conn = open_existing_db(&path)?;

    // Hand out the next session by advancing the analysis cursor exactly one
    // row. `BEGIN IMMEDIATE` takes the write lock before we read the cursor, so
    // concurrent `next` callers are serialized — every row is returned exactly
    // once and only once, with no dependency on the caller ever coming back.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let pos: i64 = tx
        .query_row(
            "SELECT analyzed_seq FROM cursor WHERE name='default'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    // First row strictly past the cursor (gap-safe: skips any missing seq_ids).
    let claimed: Option<(i64, String)> = tx
        .query_row(
            "SELECT seq_id, session_id FROM sessions WHERE seq_id > ?1 ORDER BY seq_id LIMIT 1",
            params![pos],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((seq_id, _)) = &claimed {
        tx.execute(
            "UPDATE cursor SET analyzed_seq=?1 WHERE name='default'",
            params![seq_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;

    let session_id = match claimed {
        Some((_, id)) => id,
        None => {
            println!("{}", json!({ "done": true }));
            return Ok(());
        }
    };

    // Fetch + persist the transcript (skip if we already have it, or disabled).
    if with_transcript && !has_events(&conn, &session_id).map_err(|e| e.to_string())? {
        match fetch_transcript(&session_id, max_events) {
            Ok(events) => {
                insert_events(&conn, &session_id, &events).map_err(|e| e.to_string())?;
            }
            Err(e) => {
                // Don't lose the claim on a transient transcript-fetch failure;
                // surface a warning and return metadata only.
                eprintln!("Warning: failed to fetch transcript: {}", e);
            }
        }
    }

    let mut out = session_row_json(&conn, &session_id).map_err(|e| e.to_string())?;
    // Attach the PRs this session appears in (empty array if none/un-enriched).
    let prs = session_prs_json(&conn, &session_id).map_err(|e| e.to_string())?;
    out.insert("prs".into(), Value::Array(prs));
    if with_transcript {
        let transcript = transcript_json(&conn, &session_id).map_err(|e| e.to_string())?;
        out.insert("transcript".into(), Value::Array(transcript));
    }
    println!("{}", Value::Object(out));
    Ok(())
}

fn fetch_transcript(session_id: &str, max_events: u64) -> Result<Vec<Value>, String> {
    let client = CubeClient::from_config().map_err(|e| e.to_string())?;
    let filters = json!([{
        "member": format!("{}.session_id", EVENTS_CUBE),
        "operator": "equals",
        "values": [session_id],
    }])
    .to_string();
    let args = QueryArgs {
        dimensions: event_dimensions(),
        filters_json: Some(filters),
        order: vec![
            (format!("{}.event_time", EVENTS_CUBE), "asc".into()),
            (format!("{}.output_seq", EVENTS_CUBE), "asc".into()),
        ],
        limit: Some(max_events),
        ..Default::default()
    };
    let query = args.to_query()?;
    client.load_rows(&query).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// stats / reset / exec
// ---------------------------------------------------------------------------

fn cmd_stats(args: &[String]) -> Result<(), String> {
    let path = single_db_arg(args, "stats")?;
    let conn = open_existing_db(&path)?;

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    let (fetched, target, complete, analyzed_seq): (i64, Option<i64>, i64, i64) = conn
        .query_row(
            "SELECT fetched, target, pull_complete, analyzed_seq FROM cursor WHERE name='default'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or((0, None, 0, 0));
    // Analysis progress: rows at or before the cursor are served; the rest remain.
    let analyzed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE seq_id <= ?1",
            params![analyzed_seq],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_events", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    println!("db: {}", path);
    println!("sessions: {} total", total);
    println!(
        "analysis cursor: analyzed_seq={} ({}/{} served, {} remaining)",
        analyzed_seq,
        analyzed,
        total,
        total - analyzed
    );
    println!(
        "pull cursor: fetched={} target={} complete={}",
        fetched,
        target.map(|t| t.to_string()).unwrap_or_else(|| "-".into()),
        if complete != 0 { "yes" } else { "no" }
    );
    println!("transcript events stored: {}", events);
    Ok(())
}

fn cmd_reset(args: &[String]) -> Result<(), String> {
    let mut db_path: Option<String> = None;
    let mut to: i64 = 0;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            // Rewind (or set) the analysis cursor; `next` will re-serve from here.
            "--to" => {
                to = take_value(args, &mut i, "--to")?
                    .parse()
                    .map_err(|_| "--to must be a number".to_string())?
            }
            // `--help`/`-h` is handled by `handle_sessions` before dispatch.
            other if !other.starts_with('-') && db_path.is_none() => {
                db_path = Some(other.to_string())
            }
            other => return Err(format!("unknown reset argument: {}", other)),
        }
        i += 1;
    }
    let path = db_path.ok_or("usage: git-ai analyze sessions reset <db> [--to <seq>]")?;
    let conn = open_existing_db(&path)?;

    conn.execute(
        "UPDATE cursor SET analyzed_seq=?1 WHERE name='default'",
        params![to],
    )
    .map_err(|e| e.to_string())?;

    eprintln!(
        "Analysis cursor reset to {}; `next` will re-serve from there.",
        to
    );
    Ok(())
}

fn cmd_exec(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("usage: git-ai analyze sessions exec <db> \"<SQL>\"".to_string());
    }
    let path = &args[0];
    let sql = &args[1];
    let conn = open_existing_db(path)?;

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let col_count = stmt.column_count();
    if col_count == 0 {
        drop(stmt);
        let n = conn.execute(sql, []).map_err(|e| e.to_string())?;
        eprintln!("{} row(s) affected.", n);
        return Ok(());
    }

    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    println!("{}", names.join("\t"));
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let cells: Vec<String> = (0..col_count)
            .map(|idx| {
                row.get_ref(idx)
                    .map(value_ref_to_string)
                    .unwrap_or_default()
            })
            .collect();
        println!("{}", cells.join("\t"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

fn single_db_arg(args: &[String], cmd: &str) -> Result<String, String> {
    args.iter()
        .find(|a| !a.starts_with('-'))
        .cloned()
        .ok_or_else(|| format!("usage: git-ai analyze sessions {} <db>", cmd))
}

/// Pick a fresh, scrutable DB path like `git-ai-analysis-001.db` in the temp
/// dir. Scans existing `git-ai-analysis-NNN.db` files and uses the next free
/// number so repeated pulls are easy to tell apart and reference.
fn random_db_path() -> PathBuf {
    let dir = std::env::temp_dir();
    let highest = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let num = name.strip_prefix("git-ai-analysis-")?.strip_suffix(".db")?;
            num.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0);
    // Advance past any existing file (covers gaps / concurrent creators).
    let mut n = highest + 1;
    loop {
        let path = dir.join(format!("git-ai-analysis-{:03}.db", n));
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}
