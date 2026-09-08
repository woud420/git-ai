use super::{JobSlot, JobToken};

#[test]
fn jj_observer_job_slot_refuses_overlapping_native_work() {
    let mut slot = JobSlot::default();
    let first = slot.reserve(7).unwrap();
    assert!(slot.reserve(7).is_err());
    assert!(slot.in_flight());
    assert!(slot.finish(first, 7));
    assert!(!slot.in_flight());
    let second = slot.reserve(7).unwrap();
    assert_ne!(first, second);
}

#[test]
fn jj_observer_job_slot_cancel_retains_ownership_until_drain() {
    let mut slot = JobSlot::default();
    let validating_absent_intent = slot.reserve(0).unwrap();
    slot.cancel().unwrap();
    assert!(slot.in_flight());
    assert!(slot.reserve(0).is_err());
    assert!(!slot.finish(validating_absent_intent, 0));
    assert!(!slot.in_flight());
    assert!(slot.reserve(0).is_ok());
}

#[test]
fn jj_observer_job_slot_old_error_cannot_publish_after_disable() {
    let mut slot = JobSlot::default();
    let tick = slot.reserve(12).unwrap();
    slot.cancel().unwrap();
    assert!(!slot.finish(tick, 13));
    let resumed = slot.reserve(14).unwrap();
    assert!(!slot.finish(tick, 14));
    assert!(slot.in_flight());
    assert!(slot.finish(resumed, 14));
}

#[test]
fn jj_observer_job_slot_revision_change_refuses_stale_result() {
    let mut slot = JobSlot::default();
    let old = slot.reserve(2).unwrap();
    assert!(!slot.finish(old, 3));
    assert!(!slot.in_flight());
}

#[test]
fn jj_observer_job_slot_foreign_completion_does_not_release_current_work() {
    let mut slot = JobSlot::default();
    let current = slot.reserve(8).unwrap();
    let foreign = JobToken {
        revision: 8,
        epoch: current.epoch + 1,
    };
    assert!(!slot.finish(foreign, 8));
    assert!(slot.in_flight());
    assert!(slot.finish(current, 8));
}

#[test]
fn jj_observer_job_slot_overflow_never_reuses_cancellation_epoch() {
    let mut slot = JobSlot {
        epoch: u64::MAX - 1,
        active: None,
    };
    let last = slot.reserve(1).unwrap();
    assert_eq!(last.epoch, u64::MAX);
    assert!(slot.cancel().is_err());
    assert!(slot.in_flight());
    assert!(!slot.finish(last, 2));
    assert!(slot.reserve(2).is_err());
    assert!(!slot.in_flight());
}
