//! Tests for `flush_pending_metric_records_with`.

use super::*;
use crate::clients::api::metrics::MetricsUploadError;
use crate::model::repository::metrics_db::MetricsDatabase;
use std::cell::RefCell;
use std::rc::Rc;

fn event_json(ts: u32) -> String {
    format!(r#"{{"t":{ts},"e":1,"v":{{}},"a":{{}}}}"#)
}

use super::current_unix_ts as unix_now;

fn now_ts() -> u32 {
    unix_now().min(u32::MAX as u64) as u32
}

#[test]
fn flush_pending_metric_records_uploads_from_db_and_marks_delivered() {
    let (metrics_db, _metrics_db_dir) = MetricsDatabase::new_temp_for_tests().unwrap();
    let db = Rc::new(RefCell::new(metrics_db));
    let ts1 = now_ts().saturating_sub(2);
    let ts2 = now_ts().saturating_sub(1);
    db.borrow_mut()
        .insert_events(&[event_json(ts1), event_json(ts2)])
        .unwrap();

    let uploaded = Rc::new(RefCell::new(Vec::<Vec<u32>>::new()));
    let result = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        {
            let uploaded = Rc::clone(&uploaded);
            move |batch| {
                uploaded
                    .borrow_mut()
                    .push(batch.events.iter().map(|event| event.timestamp).collect());
                Ok(MetricsUploadResponse { errors: vec![] })
            }
        },
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        1,
    )
    .unwrap();

    assert_eq!(
        result,
        PendingMetricsFlushResult {
            uploaded_events: 2,
            uploaded_batches: 2,
            invalid_records: 0,
        }
    );
    assert_eq!(*uploaded.borrow(), vec![vec![ts2], vec![ts1]]);
    assert_eq!(db.borrow().count().unwrap(), 0);
    assert_eq!(
        db.borrow().get_metric_history(0, None, &[1]).unwrap().len(),
        2
    );
}

#[test]
fn flush_pending_metric_records_marks_invalid_rows_delivered() {
    let (metrics_db, _metrics_db_dir) = MetricsDatabase::new_temp_for_tests().unwrap();
    let db = Rc::new(RefCell::new(metrics_db));
    let ts = now_ts();
    db.borrow_mut()
        .insert_events(&["not-json".to_string(), event_json(ts)])
        .unwrap();

    let uploaded = Rc::new(RefCell::new(Vec::<u32>::new()));
    let result = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        {
            let uploaded = Rc::clone(&uploaded);
            move |batch| {
                uploaded
                    .borrow_mut()
                    .extend(batch.events.iter().map(|event| event.timestamp));
                Ok(MetricsUploadResponse { errors: vec![] })
            }
        },
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        10,
    )
    .unwrap();

    assert_eq!(
        result,
        PendingMetricsFlushResult {
            uploaded_events: 1,
            uploaded_batches: 1,
            invalid_records: 1,
        }
    );
    assert_eq!(*uploaded.borrow(), vec![ts]);
    assert_eq!(db.borrow().count().unwrap(), 0);
    assert_eq!(
        db.borrow().get_metric_history(0, None, &[1]).unwrap().len(),
        1
    );
}

#[test]
fn flush_pending_metric_records_marks_partial_server_errors_undeliverable() {
    let (metrics_db, _metrics_db_dir) = MetricsDatabase::new_temp_for_tests().unwrap();
    let db = Rc::new(RefCell::new(metrics_db));
    let ts1 = now_ts().saturating_sub(3);
    let ts2 = now_ts().saturating_sub(2);
    let ts3 = now_ts().saturating_sub(1);
    db.borrow_mut()
        .insert_events(&[event_json(ts1), event_json(ts2), event_json(ts3)])
        .unwrap();

    let uploaded = Rc::new(RefCell::new(Vec::<u32>::new()));
    let result = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        {
            let uploaded = Rc::clone(&uploaded);
            move |batch| {
                uploaded
                    .borrow_mut()
                    .extend(batch.events.iter().map(|event| event.timestamp));
                Ok(MetricsUploadResponse {
                    errors: vec![MetricsUploadError {
                        index: 1,
                        error: "validation failed".to_string(),
                    }],
                })
            }
        },
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        10,
    )
    .unwrap();

    assert_eq!(
        result,
        PendingMetricsFlushResult {
            uploaded_events: 2,
            uploaded_batches: 1,
            invalid_records: 0,
        }
    );
    assert_eq!(*uploaded.borrow(), vec![ts3, ts2, ts1]);
    assert_eq!(db.borrow().count().unwrap(), 1);
    assert_eq!(db.borrow().count_retryable().unwrap(), 0);
    assert!(
        db.borrow_mut()
            .dequeue_pending_batch(10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.borrow().get_metric_history(0, None, &[1]).unwrap().len(),
        3
    );
}

#[test]
fn flush_pending_metric_records_marks_all_server_errors_undeliverable() {
    let (metrics_db, _metrics_db_dir) = MetricsDatabase::new_temp_for_tests().unwrap();
    let db = Rc::new(RefCell::new(metrics_db));
    let ts1 = now_ts().saturating_sub(2);
    let ts2 = now_ts().saturating_sub(1);
    db.borrow_mut()
        .insert_events(&[event_json(ts1), event_json(ts2)])
        .unwrap();

    let result = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        |_batch| {
            Ok(MetricsUploadResponse {
                errors: vec![
                    MetricsUploadError {
                        index: 0,
                        error: "first failed".to_string(),
                    },
                    MetricsUploadError {
                        index: 1,
                        error: "second failed".to_string(),
                    },
                ],
            })
        },
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        10,
    )
    .unwrap();

    assert_eq!(
        result,
        PendingMetricsFlushResult {
            uploaded_events: 0,
            uploaded_batches: 1,
            invalid_records: 0,
        }
    );
    assert_eq!(db.borrow().count().unwrap(), 2);
    assert_eq!(db.borrow().count_retryable().unwrap(), 0);
    assert!(
        db.borrow_mut()
            .dequeue_pending_batch(10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.borrow().get_metric_history(0, None, &[1]).unwrap().len(),
        2
    );
}

#[test]
fn flush_pending_metric_records_retries_batch_for_invalid_server_error_index() {
    let (metrics_db, _metrics_db_dir) = MetricsDatabase::new_temp_for_tests().unwrap();
    let db = Rc::new(RefCell::new(metrics_db));
    db.borrow_mut()
        .insert_events(&[event_json(now_ts().saturating_sub(1))])
        .unwrap();

    let result = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        |_batch| {
            Ok(MetricsUploadResponse {
                errors: vec![MetricsUploadError {
                    index: 1,
                    error: "out of bounds".to_string(),
                }],
            })
        },
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        10,
    );

    assert!(result.is_err());
    assert_eq!(db.borrow().count().unwrap(), 1);
    assert_eq!(db.borrow().count_retryable().unwrap(), 0);
    assert_eq!(
        db.borrow().get_metric_history(0, None, &[1]).unwrap().len(),
        1
    );
}

#[test]
fn flush_pending_metric_records_keeps_rows_pending_after_upload_failure() {
    let (metrics_db, _metrics_db_dir) = MetricsDatabase::new_temp_for_tests().unwrap();
    let db = Rc::new(RefCell::new(metrics_db));
    let ts = now_ts();
    db.borrow_mut().insert_events(&[event_json(ts)]).unwrap();

    let result = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        |_batch| Err(GitAiError::Generic("upload failed".to_string())),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        10,
    );

    assert!(result.is_err());
    assert_eq!(db.borrow().count().unwrap(), 1);
    assert_eq!(db.borrow().count_retryable().unwrap(), 0);
}

#[test]
fn flush_pending_metric_records_uploads_new_rows_after_old_failure() {
    let (metrics_db, _metrics_db_dir) = MetricsDatabase::new_temp_for_tests().unwrap();
    let db = Rc::new(RefCell::new(metrics_db));
    let old_ts = now_ts().saturating_sub(10);
    db.borrow_mut()
        .insert_events(&[event_json(old_ts)])
        .unwrap();

    let failed = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        |_batch| Err(GitAiError::Generic("upload failed".to_string())),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        1,
    );
    assert!(failed.is_err());
    assert_eq!(db.borrow().count_retryable().unwrap(), 0);

    let new_ts = now_ts();
    db.borrow_mut()
        .insert_events(&[event_json(new_ts)])
        .unwrap();
    assert_eq!(db.borrow().count_retryable().unwrap(), 1);

    let uploaded = Rc::new(RefCell::new(Vec::<Vec<u32>>::new()));
    let result = flush_pending_metric_records_with(
        {
            let db = Rc::clone(&db);
            move |limit| db.borrow_mut().dequeue_pending_batch(limit)
        },
        {
            let db = Rc::clone(&db);
            move |ids| db.borrow_mut().mark_records_delivered(ids, unix_now())
        },
        {
            let db = Rc::clone(&db);
            move |ids, err| {
                let now = unix_now();
                db.borrow_mut()
                    .mark_records_failed(ids, &err.to_string(), now)
            }
        },
        {
            let db = Rc::clone(&db);
            move |records| {
                db.borrow_mut()
                    .mark_records_undeliverable(records, unix_now())
            }
        },
        {
            let uploaded = Rc::clone(&uploaded);
            move |batch| {
                uploaded
                    .borrow_mut()
                    .push(batch.events.iter().map(|event| event.timestamp).collect());
                Ok(MetricsUploadResponse { errors: vec![] })
            }
        },
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        1,
    )
    .unwrap();

    assert_eq!(
        result,
        PendingMetricsFlushResult {
            uploaded_events: 1,
            uploaded_batches: 1,
            invalid_records: 0,
        }
    );
    assert_eq!(*uploaded.borrow(), vec![vec![new_ts]]);
    assert_eq!(db.borrow().count().unwrap(), 1);
    let history = db.borrow().get_metric_history(0, None, &[1]).unwrap();
    assert!(history.iter().any(|record| record.ts == old_ts));
}
