use super::*;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[tokio::test]
async fn graceful_shutdown_drains_visible_trace_ingest_and_late_ready_work() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.next_trace_ingest_seq.store(1, Ordering::Release);

    let mut shutdown = {
        let coord = Arc::clone(&coord);
        tokio::spawn(async move { coord.handle_control_request(ControlRequest::Shutdown).await })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut shutdown)
            .await
            .is_err(),
        "shutdown must wait for trace work already visible to the daemon"
    );

    coord
        .append_family_sequencer_entry("late-family", 1, FamilySequencerEntry::Canceled)
        .unwrap();
    coord.processed_trace_ingest_seq.store(1, Ordering::Release);
    coord.trace_ingest_progress_notify.notify_waiters();

    let response = tokio::time::timeout(Duration::from_secs(1), shutdown)
        .await
        .expect("shutdown should drain the late sequencer entry")
        .expect("shutdown task should not panic");
    assert!(response.ok, "graceful shutdown failed: {response:?}");
    assert!(
        coord.family_sequencers_by_family.lock().unwrap()["late-family"]
            .entries
            .is_empty(),
        "late ready work must be drained before shutdown acknowledges"
    );
}

#[tokio::test]
async fn graceful_shutdown_ignores_entries_blocked_by_an_open_root() {
    let coord = ActorDaemonCoordinator::new();
    coord
        .append_pending_root_entry("blocked-family", "open-root", 1)
        .unwrap();
    coord
        .append_family_sequencer_entry("blocked-family", 2, FamilySequencerEntry::Canceled)
        .unwrap();

    let response = tokio::time::timeout(
        Duration::from_millis(100),
        coord.handle_control_request(ControlRequest::Shutdown),
    )
    .await
    .expect("shutdown must not wait on work gated behind an unfinished trace root");
    assert!(response.ok, "graceful shutdown failed: {response:?}");
    assert_eq!(
        coord.family_sequencers_by_family.lock().unwrap()["blocked-family"]
            .entries
            .len(),
        2,
        "blocked entries must stay ordered for replay instead of running early"
    );
}
