use super::test_support::*;
use super::*;
use rusqlite::{StatementStatus, params};

#[test]
fn test_dequeue_pending_batch_locks_rows() {
    let (mut db, _temp_dir) = create_test_db();
    let events = vec![event_json(days_ago(2)), event_json(days_ago(1))];
    db.insert_events(&events).unwrap();

    let batch = db.dequeue_pending_batch(1).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(db.count().unwrap(), 2);
    assert_eq!(db.count_retryable().unwrap(), 1);

    db.mark_records_delivered(&[batch[0].id], unix_now())
        .unwrap();
    assert_eq!(db.count().unwrap(), 1);
    assert_eq!(db.count_retryable().unwrap(), 1);
}

#[test]
fn test_dequeue_pending_batch_prefers_newest_retryable_rows() {
    let (mut db, _temp_dir) = create_test_db();
    let oldest_ts = days_ago(3);
    let middle_ts = days_ago(2);
    let newest_ts = days_ago(1);
    db.insert_events(&[
        event_json(oldest_ts),
        event_json(middle_ts),
        event_json(newest_ts),
    ])
    .unwrap();

    let batch = db.dequeue_pending_batch(2).unwrap();
    assert_eq!(batch.len(), 2);
    assert!(batch[0].id > batch[1].id);
    assert!(batch[0].event_json.contains(&format!("\"t\":{newest_ts}")));
    assert!(batch[1].event_json.contains(&format!("\"t\":{middle_ts}")));
}

#[test]
fn test_retryable_query_work_is_independent_of_exhausted_history() {
    let (db, _temp_dir) = create_test_db();
    let now = unix_now() as i64;

    db.conn
        .execute(
            "INSERT INTO metrics (event_json, next_retry_at) VALUES (?1, 0)",
            params![event_json(days_ago(1))],
        )
        .unwrap();
    db.conn
        .execute(
            r#"
            WITH RECURSIVE exhausted(n) AS (
                VALUES(1)
                UNION ALL
                SELECT n + 1 FROM exhausted WHERE n < 20000
            )
            INSERT INTO metrics (event_json, attempts, next_retry_at)
            SELECT '{"t":1,"e":1,"v":{},"a":{}}', 6, 0 FROM exhausted
            "#,
            [],
        )
        .unwrap();

    let mut stmt = db.conn.prepare(RETRYABLE_METRIC_IDS_SQL).unwrap();
    let ids = stmt
        .query_map(params![now, 100], |row| row.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(ids, vec![1]);
    assert_eq!(stmt.get_status(StatementStatus::FullscanStep), 0);
    assert_eq!(stmt.get_status(StatementStatus::Sort), 0);
    assert!(
        stmt.get_status(StatementStatus::VmStep) < 1_000,
        "retryable lookup must not scale with exhausted history"
    );
}

#[test]
fn test_failed_records_do_not_block_unfailed_retryable_rows() {
    let (mut db, _temp_dir) = create_test_db();
    db.insert_events(&[event_json(days_ago(2)), event_json(days_ago(1))])
        .unwrap();

    let batch = db.dequeue_pending_batch(1).unwrap();
    let failed_id = batch[0].id;
    let failed_at = unix_now();
    db.mark_records_failed(&[failed_id], "upload failed", failed_at)
        .unwrap();

    assert_eq!(db.count().unwrap(), 2);
    assert_eq!(db.count_retryable().unwrap(), 1);

    let retryable_batch = db.dequeue_pending_batch(10).unwrap();
    assert_eq!(retryable_batch.len(), 1);
    assert_ne!(retryable_batch[0].id, failed_id);

    let (attempts, next_retry_at): (i64, i64) = db
        .conn
        .query_row(
            "SELECT attempts, next_retry_at FROM metrics WHERE id = ?1",
            params![failed_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(attempts, 1);
    assert!(next_retry_at > failed_at as i64);
}

#[test]
fn test_dequeue_releases_stale_processing_locks() {
    let (mut db, _temp_dir) = create_test_db();
    db.insert_events(&[event_json(days_ago(1))]).unwrap();

    let first_batch = db.dequeue_pending_batch(1).unwrap();
    assert_eq!(first_batch.len(), 1);
    assert_eq!(db.count_retryable().unwrap(), 0);

    let stale_started_at = unix_now().saturating_sub(METRIC_PROCESSING_LOCK_TIMEOUT_SECS + 1);
    db.conn
        .execute(
            "UPDATE metrics SET processing_started_at = ?1 WHERE id = ?2",
            params![stale_started_at as i64, first_batch[0].id],
        )
        .unwrap();

    let second_batch = db.dequeue_pending_batch(1).unwrap();
    assert_eq!(second_batch.len(), 1);
    assert_eq!(second_batch[0].id, first_batch[0].id);
}

#[test]
fn test_max_attempts_are_not_retryable() {
    let (mut db, _temp_dir) = create_test_db();
    let ids = db.insert_events(&[event_json(days_ago(1))]).unwrap();
    db.conn
        .execute(
            "UPDATE metrics SET attempts = ?1 WHERE id = ?2",
            params![MAX_METRIC_UPLOAD_ATTEMPTS as i64, ids[0]],
        )
        .unwrap();

    assert_eq!(db.count().unwrap(), 1);
    assert_eq!(db.count_retryable().unwrap(), 0);
    assert!(db.dequeue_pending_batch(1).unwrap().is_empty());
}

#[test]
fn test_mark_records_undeliverable_keeps_history_without_retrying() {
    let (mut db, _temp_dir) = create_test_db();
    let event_ts = days_ago(1);
    let ids = db.insert_events(&[event_json(event_ts)]).unwrap();

    let batch = db.dequeue_pending_batch(1).unwrap();
    assert_eq!(batch.len(), 1);
    db.mark_records_undeliverable(&[(ids[0], "validation failed".to_string())], unix_now())
        .unwrap();

    assert_eq!(db.count().unwrap(), 1);
    assert_eq!(db.count_retryable().unwrap(), 0);
    assert!(db.dequeue_pending_batch(1).unwrap().is_empty());
    assert_eq!(db.get_metric_history(0, None, &[1]).unwrap().len(), 1);

    let (delivered_ts, attempts, last_sync_error): (Option<i64>, i64, Option<String>) = db
        .conn
        .query_row(
            "SELECT delivered_ts, attempts, last_sync_error FROM metrics WHERE id = ?1",
            params![ids[0]],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(delivered_ts.is_none());
    assert_eq!(attempts, MAX_METRIC_UPLOAD_ATTEMPTS as i64);
    assert_eq!(last_sync_error.as_deref(), Some("validation failed"));
}

#[test]
fn test_mark_records_delivered() {
    let (mut db, _temp_dir) = create_test_db();
    let ts1 = days_ago(3);
    let ts2 = days_ago(2);
    let ts3 = days_ago(1);

    let events = vec![event_json(ts1), event_json(ts2), event_json(ts3)];

    db.insert_events(&events).unwrap();

    // Dequeue newest rows and mark them delivered.
    let batch = db.dequeue_pending_batch(2).unwrap();
    let ids: Vec<i64> = batch.iter().map(|r| r.id).collect();

    db.mark_records_delivered(&ids, unix_now()).unwrap();

    // Verify only one remains pending.
    let count = db.count().unwrap();
    assert_eq!(count, 1);

    // Verify remaining pending row is the oldest one.
    let remaining = pending_event_jsons(&db);
    assert_eq!(remaining.len(), 1);
    assert!(remaining[0].contains(&format!("\"t\":{ts1}")));

    // Verify delivered rows are retained.
    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 3);
}
