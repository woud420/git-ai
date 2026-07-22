use super::cube_fields::{equals_filters, merge_filters};
use super::db::{
    ensure_derived_columns, get_fetched, init_cursor, insert_events, insert_sessions,
    recompute_models, recompute_pr_counts, set_fetched, set_pull_complete, transcript_json,
};
use super::schema::SCHEMA;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

fn temp_db() -> (rusqlite::Connection, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("analyze-sessions.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    // Mirror open_db so the derived funnel-gap columns are present in tests.
    ensure_derived_columns(&conn).unwrap();
    (conn, dir)
}

fn session_row(id: &str) -> Value {
    json!({
        "public_v1_sessions.session_id": id,
        "public_v1_sessions.user_id": "u1",
        "public_v1_sessions.agent": "claude-code",
        "public_v1_sessions.repo_url": "https://example.com/repo",
        "public_v1_sessions.session_start_time": "2024-06-01T12:00:00.000",
        "public_v1_sessions.total_generated_lines": "240",
        "public_v1_sessions.total_production_lines": "180",
        "public_v1_sessions.net_generated_lines": "-5",
    })
}

#[test]
fn insert_sessions_dedupes_by_session_id() {
    let (conn, _db_dir) = temp_db();
    let rows = vec![session_row("s1"), session_row("s2")];
    assert_eq!(insert_sessions(&conn, &rows).unwrap(), 2);
    // Re-inserting the same ids adds nothing.
    let rows2 = vec![session_row("s1"), session_row("s3")];
    assert_eq!(insert_sessions(&conn, &rows2).unwrap(), 1);
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 3);
}

#[test]
fn derived_funnel_gaps_compute_from_stage_columns() {
    let (conn, _db_dir) = temp_db();
    insert_sessions(&conn, &[session_row("s1")]).unwrap();
    // Set a full funnel: 100 committed → 70 pr_opened → 40 merged → 30 prod.
    conn.execute(
        "UPDATE sessions SET committed_lines=100, pr_opened_lines=70, \
         merged_lines=40, production_lines=30 WHERE session_id='s1'",
        [],
    )
    .unwrap();
    let (c_npr, pr_nm, m_np, c_np, rate): (i64, i64, i64, i64, f64) = conn
        .query_row(
            "SELECT committed_not_pr_opened, pr_opened_not_merged, \
             merged_not_production, committed_not_production, production_rate \
             FROM sessions WHERE session_id='s1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(c_npr, 30); // 100 - 70
    assert_eq!(pr_nm, 30); // 70 - 40
    assert_eq!(m_np, 10); // 40 - 30
    assert_eq!(c_np, 70); // 100 - 30
    assert!((rate - 0.30).abs() < 1e-9); // 30 / 100

    // NULL stage columns coalesce to 0 (clean integer gaps), and a 0/NULL
    // committed denominator yields a NULL rate rather than a divide error.
    insert_sessions(&conn, &[session_row("s2")]).unwrap();
    let (c_np2, rate2): (i64, Option<f64>) = conn
        .query_row(
            "SELECT committed_not_production, production_rate \
             FROM sessions WHERE session_id='s2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    // s2 has production=180 but committed is NULL → 0 - 180 = -180.
    assert_eq!(c_np2, -180);
    assert_eq!(rate2, None);
}

#[test]
fn recompute_models_denormalizes_distinct_models() {
    let (conn, _db_dir) = temp_db();
    insert_sessions(&conn, &[session_row("s1"), session_row("s2")]).unwrap();
    // Two models for s1 (out of order), one for s2.
    conn.execute(
        "INSERT INTO session_models (session_id, model, event_count) VALUES \
         ('s1','sonnet',3),('s1','opus',1),('s2','opus',2)",
        [],
    )
    .unwrap();
    recompute_models(&conn).unwrap();
    let m1: String = conn
        .query_row(
            "SELECT models FROM sessions WHERE session_id='s1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // Alphabetically ordered, comma-joined.
    assert_eq!(m1, "opus,sonnet");
    let m2: String = conn
        .query_row(
            "SELECT models FROM sessions WHERE session_id='s2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(m2, "opus");
}

#[test]
fn recompute_pr_counts_counts_distinct_prs() {
    let (conn, _db_dir) = temp_db();
    insert_sessions(&conn, &[session_row("s1"), session_row("s2")]).unwrap();
    conn.execute(
        "INSERT INTO session_prs (session_id, repo_url, pr_number, ai_lines) VALUES \
         ('s1','r',1,10),('s1','r',2,20)",
        [],
    )
    .unwrap();
    recompute_pr_counts(&conn).unwrap();
    let (c1, c2): (i64, Option<i64>) = (
        conn.query_row(
            "SELECT pr_count FROM sessions WHERE session_id='s1'",
            [],
            |r| r.get(0),
        )
        .unwrap(),
        conn.query_row(
            "SELECT pr_count FROM sessions WHERE session_id='s2'",
            [],
            |r| r.get(0),
        )
        .unwrap(),
    );
    assert_eq!(c1, 2);
    // s2 has no PRs, so it stays NULL (untouched by recompute).
    assert_eq!(c2, None);
}

#[test]
fn session_numbers_and_time_parse() {
    let (conn, _db_dir) = temp_db();
    insert_sessions(&conn, &[session_row("s1")]).unwrap();
    let (g, prod, net, start): (i64, i64, i64, i64) = conn
        .query_row(
            "SELECT generated_lines, production_lines, net_generated_lines, session_start_time \
             FROM sessions WHERE session_id='s1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(g, 240);
    assert_eq!(prod, 180);
    assert_eq!(net, -5);
    // 2024-06-01T12:00:00Z
    assert_eq!(start, 1_717_243_200);
}

/// Mirror of cmd_next's atomic cursor advance: read analyzed_seq, take the
/// first row past it, persist the new position. Returns the served id.
fn claim_next(conn: &mut rusqlite::Connection) -> Option<String> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let pos: i64 = tx
        .query_row(
            "SELECT analyzed_seq FROM cursor WHERE name='default'",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
        .unwrap_or(0);
    let row: Option<(i64, String)> = tx
        .query_row(
            "SELECT seq_id, session_id FROM sessions WHERE seq_id > ?1 ORDER BY seq_id LIMIT 1",
            params![pos],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap();
    if let Some((seq, _)) = &row {
        tx.execute(
            "UPDATE cursor SET analyzed_seq=?1 WHERE name='default'",
            params![seq],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    row.map(|(_, id)| id)
}

#[test]
fn cursor_serves_each_row_exactly_once() {
    let (mut conn, _db_dir) = temp_db();
    init_cursor(&conn, 100).unwrap();
    insert_sessions(&conn, &[session_row("s1"), session_row("s2")]).unwrap();

    assert_eq!(claim_next(&mut conn), Some("s1".to_string()));
    assert_eq!(claim_next(&mut conn), Some("s2".to_string()));
    // Past the end: nothing more, and it stays exhausted.
    assert_eq!(claim_next(&mut conn), None);
    assert_eq!(claim_next(&mut conn), None);
}

#[test]
fn reset_rewinds_cursor_to_reserve_from_start() {
    let (mut conn, _db_dir) = temp_db();
    init_cursor(&conn, 100).unwrap();
    insert_sessions(&conn, &[session_row("s1"), session_row("s2")]).unwrap();
    assert_eq!(claim_next(&mut conn), Some("s1".to_string()));
    assert_eq!(claim_next(&mut conn), Some("s2".to_string()));

    // reset <db> sets the cursor back to 0.
    conn.execute("UPDATE cursor SET analyzed_seq=0 WHERE name='default'", [])
        .unwrap();
    // next now re-serves from the top.
    assert_eq!(claim_next(&mut conn), Some("s1".to_string()));
}

#[test]
fn pull_does_not_rewind_analysis_cursor() {
    // Re-running pull (init_cursor) must not reset analysis progress.
    let (conn, _db_dir) = temp_db();
    init_cursor(&conn, 100).unwrap();
    conn.execute("UPDATE cursor SET analyzed_seq=5 WHERE name='default'", [])
        .unwrap();
    init_cursor(&conn, 100).unwrap();
    let pos: i64 = conn
        .query_row(
            "SELECT analyzed_seq FROM cursor WHERE name='default'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pos, 5);
}

#[test]
fn events_persist_idempotently_and_round_trip() {
    let (conn, _db_dir) = temp_db();
    insert_sessions(&conn, &[session_row("s1")]).unwrap();
    let events = vec![
        json!({
            "public_v1_normalized_events.event_kind": "user_message",
            "public_v1_normalized_events.text": "fix the bug",
            "public_v1_normalized_events.event_time": "2024-06-01T12:00:00.000",
            "public_v1_normalized_events.output_seq": "0",
        }),
        json!({
            "public_v1_normalized_events.event_kind": "tool_call",
            "public_v1_normalized_events.tool": "Edit",
            "public_v1_normalized_events.tool_input": "...",
            "public_v1_normalized_events.event_time": "2024-06-01T12:00:05.000",
            "public_v1_normalized_events.output_seq": "1",
        }),
    ];
    assert_eq!(insert_events(&conn, "s1", &events).unwrap(), 2);
    // Idempotent: re-inserting the same events adds nothing.
    assert_eq!(insert_events(&conn, "s1", &events).unwrap(), 0);

    let transcript = transcript_json(&conn, "s1").unwrap();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0]["event_kind"], json!("user_message"));
    assert_eq!(transcript[0]["text"], json!("fix the bug"));
    assert_eq!(transcript[1]["tool"], json!("Edit"));
}

#[test]
fn cursor_tracks_fetch_progress() {
    let (conn, _db_dir) = temp_db();
    init_cursor(&conn, 100).unwrap();
    assert_eq!(get_fetched(&conn).unwrap(), 0);
    set_fetched(&conn, 50).unwrap();
    assert_eq!(get_fetched(&conn).unwrap(), 50);
    set_pull_complete(&conn).unwrap();
    let complete: i64 = conn
        .query_row(
            "SELECT pull_complete FROM cursor WHERE name='default'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(complete, 1);
}

#[test]
fn equals_filters_skips_unset() {
    assert!(equals_filters(&[("m".into(), None)]).is_empty());
    let f = equals_filters(&[
        ("public_v1_sessions.agent".into(), Some("cursor")),
        ("public_v1_sessions.repo_url".into(), None),
    ]);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0]["values"][0], json!("cursor"));
}

#[test]
fn merge_filters_combines_convenience_and_raw() {
    // No filters at all -> None.
    assert_eq!(merge_filters(vec![], None).unwrap(), None);

    // Convenience-only passes through.
    let conv = equals_filters(&[("public_v1_sessions.agent".into(), Some("cursor"))]);
    let only_conv = merge_filters(conv.clone(), None).unwrap().unwrap();
    let v: Value = serde_json::from_str(&only_conv).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);

    // Convenience + raw --filters are concatenated so both apply, letting you
    // slice on any member (here: net_generated_lines) alongside --agent.
    let raw =
        r#"[{"member":"public_v1_sessions.net_generated_lines","operator":"gt","values":["100"]}]"#;
    let merged = merge_filters(conv, Some(raw)).unwrap().unwrap();
    let v: Value = serde_json::from_str(&merged).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["member"], json!("public_v1_sessions.agent"));
    assert_eq!(
        arr[1]["member"],
        json!("public_v1_sessions.net_generated_lines")
    );
    assert_eq!(arr[1]["operator"], json!("gt"));
}

#[test]
fn merge_filters_rejects_non_array_raw() {
    assert!(merge_filters(vec![], Some("not json")).is_err());
    assert!(merge_filters(vec![], Some(r#"{"not":"an array"}"#)).is_err());
}
