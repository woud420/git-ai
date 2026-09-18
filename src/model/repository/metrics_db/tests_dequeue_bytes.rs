use super::test_support::*;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;

fn payload() -> String {
    serde_json::json!({"t": unix_now(), "e":1, "v":{"0":"界".repeat(40)}, "a":{}}).to_string()
}

#[test]
fn failed_byte_limited_batch_leaves_unclaimed_rows_immediately_retryable() {
    let (mut db, _dir) = create_test_db();
    let event = payload();
    let ids = db
        .insert_events(&[event.clone(), event.clone(), event.clone()])
        .unwrap();
    let batch = db
        .dequeue_pending_batch_with_byte_limit(1000, event.len() * 2)
        .unwrap();
    assert_eq!(
        batch.iter().map(|r| r.id).collect::<Vec<_>>(),
        [ids[2], ids[1]]
    );
    assert_eq!(
        batch.iter().map(|r| r.event_json.len()).sum::<usize>(),
        event.len() * 2
    );
    assert_eq!(db.status().unwrap().processing, 2);
    assert_eq!(db.status().unwrap().pending_retryable, 1);
    db.mark_records_failed(&[ids[2], ids[1]], "fixture upload failure", unix_now())
        .unwrap();
    assert_eq!(db.status().unwrap().processing, 0);
    assert_eq!(db.status().unwrap().waiting_retry, 2);
    let next = db
        .dequeue_pending_batch_with_byte_limit(1000, event.len())
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].id, ids[0]);
    assert_eq!(next[0].attempts, 0);
}

#[test]
fn oversized_unicode_metric_is_retained_without_claim_or_retry_penalty() {
    let (mut db, _dir) = create_test_db();
    let event = payload();
    let id = db.insert_events(std::slice::from_ref(&event)).unwrap()[0];
    let error = db
        .dequeue_pending_batch_with_byte_limit(1000, event.len() - 1)
        .unwrap_err();
    assert!(
        matches!(error, GitAiError::Persistence(PersistenceError::ReadBudgetExceeded { actual_bytes, limit_bytes, .. }) if actual_bytes == event.len() as u64 && limit_bytes == event.len() as u64 - 1)
    );
    let status = db.status().unwrap();
    assert_eq!(status.pending_retryable, 1);
    assert_eq!(status.processing, 0);
    assert_eq!(status.rows_with_errors, 0);
    let resumed = db
        .dequeue_pending_batch_with_byte_limit(1000, event.len())
        .unwrap();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].id, id);
    assert_eq!(resumed[0].attempts, 0);
    assert_eq!(resumed[0].event_json, event);
}
