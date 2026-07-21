use super::test_support::*;
use super::*;
use rusqlite::params;

#[test]
fn test_insert_events() {
    let (mut db, _temp_dir) = create_test_db();
    let ts1 = days_ago(2);
    let ts2 = days_ago(1);

    let events = vec![
        format!(r#"{{"t":{ts1},"e":1,"v":{{"0":"abc123"}},"a":{{"0":"1.0.0"}}}}"#),
        format!(r#"{{"t":{ts2},"e":1,"v":{{"0":"def456"}},"a":{{"0":"1.0.0"}}}}"#),
    ];

    let ids = db.insert_events(&events).unwrap();

    let count = db.count().unwrap();
    assert_eq!(count, 2);
    assert_eq!(db.count_retryable().unwrap(), 2);
    assert_eq!(ids.len(), 2);
    assert_eq!(
        metric_metadata_rows(&db),
        vec![(Some(ts1 as i64), Some(1)), (Some(ts2 as i64), Some(1))]
    );
}

#[test]
fn test_insert_events_populates_existing_common_metadata_from_attrs() {
    let (mut db, _temp_dir) = create_test_db();
    let event_ts = days_ago(1);
    db.insert_events(&[event_json_with_all_common_metadata(event_ts, 5)])
        .unwrap();

    let row: (Option<i64>, Option<i64>) = db
        .conn
        .query_row("SELECT event_ts, event_kind FROM metrics", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(row, (Some(event_ts as i64), Some(5)));
    assert_eq!(
        metric_identifier_rows(&db),
        vec![MetricIdentifierRow {
            trace_id: Some("trace-1".to_string()),
            session_id: Some("session-1".to_string()),
            parent_session_id: Some("parent-session-1".to_string()),
            tool: Some("codex".to_string()),
            external_session_id: Some("external-session-1".to_string()),
            external_parent_session_id: Some("external-parent-session-1".to_string()),
            external_event_id: None,
            external_parent_event_id: None,
            external_tool_use_id: None,
        }]
    );
}

#[test]
fn test_insert_events_with_delivered_ts_populates_event_metadata() {
    let (mut db, _temp_dir) = create_test_db();
    let delivered_ts = unix_now();
    let event_ts = days_ago(1);
    db.insert_events_with_delivered_ts(
        &[event_json_with_all_common_metadata(event_ts, 6)],
        Some(delivered_ts),
    )
    .unwrap();

    let row: (Option<i64>, Option<i64>, Option<i64>, Option<String>) = db
        .conn
        .query_row(
            "SELECT event_ts, event_kind, delivered_ts, trace_id FROM metrics",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            Some(event_ts as i64),
            Some(6),
            Some(delivered_ts as i64),
            Some("trace-1".to_string())
        )
    );
}

#[test]
fn test_insert_events_populates_event_specific_external_ids() {
    let (mut db, _temp_dir) = create_test_db();
    let session_event_ts = days_ago(2);
    let otel_trace_ts = days_ago(1);
    let checkpoint_ts = unix_now().min(u32::MAX as u64) as u32;
    let events = vec![
        format!(
            r#"{{
                "t":{session_event_ts},
                "e":5,
                "v":{{"1":"legacy-event","2":"legacy-parent","3":"legacy-tool"}},
                "a":{{"24":"session-from-attrs"}}
            }}"#
        ),
        format!(
            r#"{{
                "t":{otel_trace_ts},
                "e":6,
                "v":{{"1":"otel-event","2":"otel-parent","3":"otel-tool"}},
                "a":{{"25":"trace-from-attrs"}}
            }}"#
        ),
        format!(
            r#"{{
                "t":{checkpoint_ts},
                "e":4,
                "v":{{"7":"checkpoint-tool-use"}},
                "a":{{"20":"claude-code"}}
            }}"#
        ),
    ];

    db.insert_events(&events).unwrap();

    assert_eq!(
        metric_identifier_rows(&db),
        vec![
            MetricIdentifierRow {
                trace_id: None,
                session_id: Some("session-from-attrs".to_string()),
                parent_session_id: None,
                tool: None,
                external_session_id: None,
                external_parent_session_id: None,
                external_event_id: Some("legacy-event".to_string()),
                external_parent_event_id: Some("legacy-parent".to_string()),
                external_tool_use_id: Some("legacy-tool".to_string()),
            },
            MetricIdentifierRow {
                trace_id: Some("trace-from-attrs".to_string()),
                session_id: None,
                parent_session_id: None,
                tool: None,
                external_session_id: None,
                external_parent_session_id: None,
                external_event_id: Some("otel-event".to_string()),
                external_parent_event_id: Some("otel-parent".to_string()),
                external_tool_use_id: Some("otel-tool".to_string()),
            },
            MetricIdentifierRow {
                trace_id: None,
                session_id: None,
                parent_session_id: None,
                tool: Some("claude-code".to_string()),
                external_session_id: None,
                external_parent_session_id: None,
                external_event_id: None,
                external_parent_event_id: None,
                external_tool_use_id: Some("checkpoint-tool-use".to_string()),
            },
        ]
    );
}

#[test]
fn test_insert_events_leaves_event_metadata_null_for_invalid_json() {
    let (mut db, _temp_dir) = create_test_db();
    let recent_event_ts = days_ago(1);
    let events = vec![
        "not-json".to_string(),
        format!(r#"{{"t":{recent_event_ts},"v":{{}},"a":{{}}}}"#),
        format!(r#"{{"t":{recent_event_ts},"e":null,"v":{{}},"a":{{}}}}"#),
    ];

    db.insert_events(&events).unwrap();

    assert_eq!(
        metric_metadata_rows(&db),
        vec![(None, None), (None, None), (None, None)]
    );
    assert_eq!(
        metric_identifier_rows(&db),
        vec![
            MetricIdentifierRow {
                trace_id: None,
                session_id: None,
                parent_session_id: None,
                tool: None,
                external_session_id: None,
                external_parent_session_id: None,
                external_event_id: None,
                external_parent_event_id: None,
                external_tool_use_id: None,
            },
            MetricIdentifierRow {
                trace_id: None,
                session_id: None,
                parent_session_id: None,
                tool: None,
                external_session_id: None,
                external_parent_session_id: None,
                external_event_id: None,
                external_parent_event_id: None,
                external_tool_use_id: None,
            },
            MetricIdentifierRow {
                trace_id: None,
                session_id: None,
                parent_session_id: None,
                tool: None,
                external_session_id: None,
                external_parent_session_id: None,
                external_event_id: None,
                external_parent_event_id: None,
                external_tool_use_id: None,
            },
        ]
    );
    assert_eq!(db.count().unwrap(), 3);
}

#[test]
fn test_insert_events_with_delivered_ts_skips_batch() {
    let (mut db, _temp_dir) = create_test_db();

    let delivered_ts = unix_now();
    let delivered_event_ts = days_ago(2);
    let pending_event_ts = days_ago(1);
    let delivered = vec![event_json(delivered_event_ts)];
    let pending = vec![event_json(pending_event_ts)];

    db.insert_events_with_delivered_ts(&delivered, Some(delivered_ts))
        .unwrap();
    db.insert_events(&pending).unwrap();

    let batch = pending_event_jsons(&db);
    assert_eq!(batch.len(), 1);
    assert!(batch[0].contains(&format!("\"t\":{pending_event_ts}")));
    assert_eq!(db.count().unwrap(), 1);

    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 2);
}

#[test]
fn test_prunes_metric_rows_older_than_retention_by_event_timestamp() {
    let (mut db, _temp_dir) = create_test_db();

    let delivered_ts = unix_now();
    let old_event_ts = seconds_ago(MetricsDatabase::METRICS_RETENTION_SECS + 1);
    let recent_event_ts = seconds_ago(MetricsDatabase::METRICS_RETENTION_SECS - 1);
    let events = vec![event_json(old_event_ts), event_json(recent_event_ts)];

    db.insert_events_with_delivered_ts(&events, Some(delivered_ts))
        .unwrap();

    let total_after_prune: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total_after_prune, 1);

    let records = db.get_metric_history(0, None, &[1]).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].ts, recent_event_ts);
}

#[test]
fn test_prunes_metric_rows_older_than_retention_by_cached_event_timestamp() {
    let (mut db, _temp_dir) = create_test_db();

    let old_event_ts = seconds_ago(MetricsDatabase::METRICS_RETENTION_SECS + 1);
    let recent_json_ts = days_ago(1);
    db.conn
        .execute(
            "INSERT INTO metrics (event_json, event_ts, event_kind) VALUES (?1, ?2, ?3)",
            params![event_json(recent_json_ts), old_event_ts as i64, 1],
        )
        .unwrap();

    db.prune_old_metrics_if_due().unwrap();

    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 0);
}

#[test]
fn test_prunes_old_pending_metric_rows() {
    let (mut db, _temp_dir) = create_test_db();

    let old_event_ts = seconds_ago(MetricsDatabase::METRICS_RETENTION_SECS + 1);
    let recent_event_ts = days_ago(1);
    let pending = vec![event_json(old_event_ts), event_json(recent_event_ts)];

    db.insert_events(&pending).unwrap();

    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(db.count().unwrap(), 1);

    let batch = pending_event_jsons(&db);
    assert_eq!(batch.len(), 1);
    assert!(batch[0].contains(&format!("\"t\":{recent_event_ts}")));
}

#[test]
fn test_prunes_pending_rows_with_timestamp_even_when_kind_is_missing() {
    let (mut db, _temp_dir) = create_test_db();

    let old_event_ts = seconds_ago(MetricsDatabase::METRICS_RETENTION_SECS + 1);
    let recent_event_ts = days_ago(1);
    let pending = vec![
        format!(r#"{{"t":{old_event_ts},"v":{{}},"a":{{}}}}"#),
        format!(r#"{{"t":{recent_event_ts},"v":{{}},"a":{{}}}}"#),
    ];

    db.insert_events(&pending).unwrap();

    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 1);
    let remaining: String = db
        .conn
        .query_row("SELECT event_json FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert!(remaining.contains(&format!("\"t\":{recent_event_ts}")));
}

#[test]
fn test_prunes_malformed_delivered_rows_by_delivered_timestamp() {
    let (mut db, _temp_dir) = create_test_db();

    let old_delivered_ts = unix_now().saturating_sub(MetricsDatabase::METRICS_RETENTION_SECS + 1);
    db.insert_events_with_delivered_ts(&["not-json".to_string()], Some(old_delivered_ts))
        .unwrap();

    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 0);
}
