use super::*;

#[tokio::test]
async fn idle_root_close_marker_reclaims_pending_sequencer_slot() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    let root_sid = "idle-root";
    let family = "idle-family";
    coord
        .append_pending_root_entry(family, root_sid, 1)
        .unwrap();
    {
        let mut ingress = coord.trace_ingress_state.lock().unwrap();
        ingress.root_last_activity_ns.insert(root_sid.into(), 7);
        ingress.root_mutating.insert(root_sid.into(), true);
        ingress.root_open_connections.insert(root_sid.into(), 1);
        ingress.root_close_markers_enqueued.insert(root_sid.into());
    }

    let outcome = coord
        .apply_trace_payload_to_state(serde_json::json!({
            "event": TRACE_CONNECTION_CLOSED_EVENT,
            "sid": root_sid,
            (TRACE_IDLE_ROOT_LAST_ACTIVITY_NS_FIELD): 7,
        }))
        .await
        .unwrap();

    assert!(matches!(outcome, TracePayloadApplyOutcome::QueuedFamily));
    assert!(
        !coord
            .pending_root_slots_by_root
            .lock()
            .unwrap()
            .contains_key(root_sid)
    );
    assert!(
        !coord
            .trace_ingress_state
            .lock()
            .unwrap()
            .root_last_activity_ns
            .contains_key(root_sid)
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while coord.has_pending_attribution_work() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("scheduled cancellation should drain after the idle root is reclaimed");
    assert!(
        coord
            .family_sequencers_by_family
            .lock()
            .unwrap()
            .get(family)
            .is_none_or(|state| state.entries.is_empty())
    );
}

#[tokio::test]
async fn idle_root_close_marker_yields_to_refreshed_activity() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    let root_sid = "refreshed-root";
    let family = "refreshed-family";
    coord
        .append_pending_root_entry(family, root_sid, 1)
        .unwrap();
    {
        let mut ingress = coord.trace_ingress_state.lock().unwrap();
        ingress.root_last_activity_ns.insert(root_sid.into(), 8);
        ingress.root_mutating.insert(root_sid.into(), true);
        ingress.root_open_connections.insert(root_sid.into(), 1);
        ingress.root_close_markers_enqueued.insert(root_sid.into());
    }

    let outcome = coord
        .apply_trace_payload_to_state(serde_json::json!({
            "event": TRACE_CONNECTION_CLOSED_EVENT,
            "sid": root_sid,
            (TRACE_IDLE_ROOT_LAST_ACTIVITY_NS_FIELD): 7,
        }))
        .await
        .unwrap();

    assert!(matches!(outcome, TracePayloadApplyOutcome::None));
    assert!(
        coord
            .pending_root_slots_by_root
            .lock()
            .unwrap()
            .contains_key(root_sid)
    );
    let ingress = coord.trace_ingress_state.lock().unwrap();
    assert_eq!(ingress.root_last_activity_ns.get(root_sid), Some(&8));
    assert!(!ingress.root_close_markers_enqueued.contains(root_sid));
}
