use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

fn complete_root(coord: &ActorDaemonCoordinator, sid: &str) {
    coord.trace_root_connection_opened(sid).unwrap();
    coord.prepare_trace_payload_for_ingest(&mut json!({
        "event":"start", "sid":sid, "argv":["git", "commit"], "time_ns":1000,
    }));
    // Ingress drops the root's metadata before the asynchronous worker clears it.
    coord.prepare_trace_payload_for_ingest(&mut json!({
        "event":"atexit", "sid":sid, "code":0, "time_ns":1100,
    }));
    coord.clear_trace_root_tracking(sid).unwrap();
}

fn child_connection(coord: &Arc<ActorDaemonCoordinator>, sid: &str) -> BTreeMap<String, bool> {
    let mut roots = BTreeMap::new();
    process_trace_connection_line(
        &json!({"event":"version", "sid":format!("{sid}/child"), "time_ns":2000}).to_string(),
        Arc::clone(coord),
        &mut roots,
    )
    .unwrap();
    roots
}

#[tokio::test]
async fn completed_roots_child_does_not_recreate_ingress_state() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    complete_root(&coord, "finished");
    let child = child_connection(&coord, "finished");
    assert!(!coord.has_open_trace_roots_that_may_mutate_refs());
    assert!(
        !coord
            .trace_ingress_state
            .lock()
            .unwrap()
            .root_last_activity_ns
            .contains_key("finished")
    );
    finalize_trace_connection_roots(Arc::clone(&coord), child).unwrap();
    coord.request_shutdown();
}

#[tokio::test]
async fn completed_roots_inert_child_close_preserves_a_reactivated_root() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    complete_root(&coord, "reactivated");
    let child = child_connection(&coord, "reactivated");
    coord.trace_root_connection_opened("reactivated").unwrap();
    finalize_trace_connection_roots(Arc::clone(&coord), child).unwrap();
    assert_eq!(
        coord
            .trace_ingress_state
            .lock()
            .unwrap()
            .root_open_connections
            .get("reactivated"),
        Some(&1)
    );
    assert!(coord.has_open_trace_roots_that_may_mutate_refs());
    coord.request_shutdown();
}

#[tokio::test]
async fn completed_roots_own_frame_promotes_an_inert_connection() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    complete_root(&coord, "resumed");
    let mut connection = child_connection(&coord, "resumed");
    process_trace_connection_line(
        &json!({"event":"start", "sid":"resumed", "argv":["git", "commit"], "time_ns":3000})
            .to_string(),
        Arc::clone(&coord),
        &mut connection,
    )
    .unwrap();
    assert_eq!(connection.get("resumed"), Some(&true));
    let ingress = coord.trace_ingress_state.lock().unwrap();
    assert_eq!(ingress.root_open_connections.get("resumed"), Some(&1));
    assert!(!ingress.completed_roots.contains("resumed"));
    drop(ingress);
    assert!(coord.has_open_trace_roots_that_may_mutate_refs());
    coord.request_shutdown();
}

#[tokio::test]
async fn completed_roots_child_only_observation_does_not_prove_parent_completion() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    let first = child_connection(&coord, "unseen");
    assert!(coord.has_open_trace_roots_that_may_mutate_refs());
    finalize_trace_connection_roots(Arc::clone(&coord), first).unwrap();
    let later = child_connection(&coord, "unseen");
    assert!(coord.has_open_trace_roots_that_may_mutate_refs());
    finalize_trace_connection_roots(Arc::clone(&coord), later).unwrap();
    coord.request_shutdown();
}
