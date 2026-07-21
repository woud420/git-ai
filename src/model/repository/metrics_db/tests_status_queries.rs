use super::test_support::*;
use super::*;
use rusqlite::params;

#[test]
fn test_status_counts_delivery_buckets() {
    let (mut db, _temp_dir) = create_test_db();
    let now = unix_now();

    let delivered_ids = db
        .insert_events_with_delivered_ts(&[event_json(days_ago(5))], Some(now))
        .unwrap();
    let delivered_id = delivered_ids[0];
    let ids = db
        .insert_events(&[
            event_json(days_ago(4)),
            event_json(days_ago(3)),
            event_json(days_ago(2)),
            event_json(days_ago(1)),
        ])
        .unwrap();
    let pending_id = ids[0];
    let waiting_id = ids[1];
    let processing_id = ids[2];
    let stopped_id = ids[3];

    db.conn
        .execute(
            "UPDATE metrics \
             SET last_sync_error = ?1, last_sync_at = ?2 \
             WHERE id = ?3",
            params![
                "delivered retry recovered",
                now.saturating_add(60) as i64,
                delivered_id
            ],
        )
        .unwrap();
    db.conn
        .execute(
            "UPDATE metrics \
             SET attempts = 1, last_sync_error = ?1, last_sync_at = ?2, next_retry_at = ?3 \
             WHERE id = ?4",
            params![
                "temporary outage",
                now.saturating_sub(10) as i64,
                now.saturating_add(600) as i64,
                waiting_id
            ],
        )
        .unwrap();
    db.conn
        .execute(
            "UPDATE metrics SET processing_started_at = ?1 WHERE id = ?2",
            params![now as i64, processing_id],
        )
        .unwrap();
    db.conn
        .execute(
            "UPDATE metrics \
             SET attempts = ?1, last_sync_error = ?2, last_sync_at = ?3, next_retry_at = ?3 \
             WHERE id = ?4",
            params![
                MAX_METRIC_UPLOAD_ATTEMPTS as i64,
                "validation failed",
                now as i64,
                stopped_id
            ],
        )
        .unwrap();

    assert_ne!(pending_id, waiting_id);
    let status = db.status().unwrap();
    assert_eq!(status.total, 5);
    assert_eq!(status.delivered, 1);
    assert_eq!(status.not_delivered, 4);
    assert_eq!(status.pending_retryable, 1);
    assert_eq!(status.waiting_retry, 1);
    assert_eq!(status.processing, 1);
    assert_eq!(status.stopped_after_errors, 1);
    assert_eq!(status.rows_with_errors, 2);
    assert_eq!(status.latest_error.as_deref(), Some("validation failed"));
}

#[test]
fn test_get_metric_history_reads_authoritative_metrics_table() {
    let (mut db, _temp_dir) = create_test_db();

    let delivered_ts = unix_now();
    let ts1 = days_ago(4);
    let ts2 = days_ago(3);
    let ts3 = days_ago(2);
    let ts4 = days_ago(1);
    let delivered = vec![event_json_with_repo(
        ts1,
        1,
        "https://github.com/acme/project",
    )];
    let pending = vec![
        event_json_with_repo(ts2, 4, "https://github.com/acme/project"),
        event_json_with_repo(ts3, 2, "https://github.com/acme/project"),
        event_json_with_repo(ts4, 5, "https://github.com/other/repo"),
    ];

    db.insert_events_with_delivered_ts(&delivered, Some(delivered_ts))
        .unwrap();
    db.insert_events(&pending).unwrap();

    let records = db
        .get_metric_history(0, Some("acme/project"), &[1, 4, 5])
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].event_id, 1);
    assert_eq!(records[0].ts, ts1);
    assert_eq!(records[1].event_id, 4);
    assert_eq!(records[1].ts, ts2);

    // Delivered rows are retained for history, but only undelivered rows flush.
    assert_eq!(db.count().unwrap(), 3);
}

#[test]
fn test_get_metric_history_reads_legacy_rows_before_and_after_metadata_backfill() {
    let (mut db, _temp_dir) = create_test_db();
    let ts1 = days_ago(2);
    let ts2 = days_ago(1);
    db.conn
        .execute(
            "INSERT INTO metrics (event_json) VALUES (?1), (?2)",
            params![
                event_json_with_repo(ts1, 4, "https://github.com/acme/project"),
                event_json_with_repo(ts2, 5, "https://github.com/acme/project"),
            ],
        )
        .unwrap();

    let before = db
        .get_metric_history(0, Some("acme/project"), &[4, 5])
        .unwrap();
    assert_eq!(
        before
            .iter()
            .map(|record| (record.event_id, record.ts))
            .collect::<Vec<_>>(),
        vec![(4, ts1), (5, ts2)]
    );

    let summary = db.backfill_event_metadata_batch(100).unwrap();
    assert_eq!(summary.scanned, 2);
    assert_eq!(summary.updated, 2);

    let after = db
        .get_metric_history(0, Some("acme/project"), &[4, 5])
        .unwrap();
    assert_eq!(
        after
            .iter()
            .map(|record| (record.event_id, record.ts))
            .collect::<Vec<_>>(),
        vec![(4, ts1), (5, ts2)]
    );
}

#[test]
fn test_empty_operations() {
    let (mut db, _temp_dir) = create_test_db();

    // Insert empty should succeed
    db.insert_events(&[]).unwrap();

    // Dequeue from empty should return empty.
    let batch = db.dequeue_pending_batch(10).unwrap();
    assert!(batch.is_empty());

    // Marking an empty set delivered should succeed.
    db.mark_records_delivered(&[], 1_700_000_000).unwrap();

    // Count empty should return 0
    let count = db.count().unwrap();
    assert_eq!(count, 0);

    let status = db.status().unwrap();
    assert_eq!(status.total, 0);
    assert_eq!(status.delivered, 0);
    assert_eq!(status.not_delivered, 0);
    assert_eq!(status.pending_retryable, 0);
    assert_eq!(status.waiting_retry, 0);
    assert_eq!(status.processing, 0);
    assert_eq!(status.stopped_after_errors, 0);
    assert_eq!(status.rows_with_errors, 0);
    assert_eq!(status.latest_error, None);
}

#[test]
fn test_should_emit_agent_usage_rate_limit() {
    let (mut db, _temp_dir) = create_test_db();
    let prompt_id = "prompt-123";

    // First event for a prompt should be allowed.
    assert!(
        db.should_emit_agent_usage(prompt_id, 1_700_000_000, 300)
            .unwrap()
    );
    // Subsequent event inside the window should be throttled.
    assert!(
        !db.should_emit_agent_usage(prompt_id, 1_700_000_120, 300)
            .unwrap()
    );
    // Event outside the window should be allowed again.
    assert!(
        db.should_emit_agent_usage(prompt_id, 1_700_000_301, 300)
            .unwrap()
    );
}
