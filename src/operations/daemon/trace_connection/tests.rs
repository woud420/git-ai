use super::*;
use std::collections::BTreeMap;
use std::io::Cursor;

#[tokio::test]
async fn invalid_trace_bytes_stop_admission_and_release_unidentified_connection() {
    let coordinator = Arc::new(ActorDaemonCoordinator::new());
    coordinator.trace_unidentified_connection_opened().unwrap();
    let reader = TraceReader::new(Cursor::new([0xff, b'\n']));
    assert!(
        handle_trace_connection_actor_reader(reader, coordinator.clone(), BTreeMap::new()).is_err()
    );
    assert!(coordinator.is_shutting_down());
    assert!(
        !coordinator
            .accepting_checkpoints
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert_eq!(
        coordinator
            .trace_ingress_state
            .lock()
            .unwrap()
            .unidentified_open_connections,
        0
    );
}

#[tokio::test]
async fn worker_read_error_releases_its_existing_root_registration() {
    struct BrokenReader;
    impl Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::ConnectionReset.into())
        }
    }
    let coordinator = Arc::new(ActorDaemonCoordinator::new());
    coordinator.trace_root_connection_opened("root").unwrap();
    let reader = TraceReader::new(BrokenReader);
    assert!(
        handle_trace_connection_actor_reader(
            reader,
            coordinator.clone(),
            BTreeMap::from([("root".to_string(), true)])
        )
        .is_err()
    );
    assert!(
        !coordinator
            .trace_ingress_state
            .lock()
            .unwrap()
            .root_open_connections
            .contains_key("root")
    );
    assert!(!coordinator.is_shutting_down());
    coordinator.request_shutdown();
}
