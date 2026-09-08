use super::*;

pub(super) fn controller() -> (tempfile::TempDir, Arc<ActorDaemonCoordinator>, Observer) {
    let directory = tempfile::tempdir().unwrap();
    let coordinator = Arc::new(ActorDaemonCoordinator::new());
    let observer = Observer::new(directory.path().join("observer.sqlite"));
    {
        let mut state = poisoned_lock(&observer.state);
        state.loaded = true;
        state.available = true;
        state.coordinator = Arc::downgrade(&coordinator);
    }
    (directory, coordinator, observer)
}

#[tokio::test]
async fn jj_observer_controller_stopped_completion_at_epoch_exhaustion_refuses_and_drains() {
    let (_directory, _coordinator, observer) = controller();
    let mut state = poisoned_lock(&observer.state);
    state.stopped = false;
    state.jobs.epoch = u64::MAX - 1;
    let revision = state.revision();
    assert_eq!(revision, 0);
    let token = state.jobs.reserve(revision).unwrap();
    assert_eq!(token.epoch, u64::MAX);
    state.stopped = true;
    assert!(state.jobs.cancel().is_err());
    assert!(state.jobs.in_flight());
    assert_eq!(state.revision(), revision);
    assert!(state.available && !state.shutting_down());
    assert!(!state.finish(token));
    assert!(!state.jobs.in_flight());
    assert!(state.stopped);
    assert!(state.intent.is_none());
}

#[tokio::test]
async fn jj_observer_controller_disable_absent_validation_waits_for_owned_completion() {
    let (directory, _coordinator, observer) = controller();
    let token = {
        let mut state = poisoned_lock(&observer.state);
        state.stopped = false;
        let revision = state.revision();
        state.jobs.reserve(revision).unwrap()
    };
    let reply = observer.disable().await;
    assert!(reply.error.is_none());
    assert_eq!(reply.action, "observer_disable");
    assert_eq!(reply.disposition, "already_disabled");
    assert_eq!(reply.revision, Some(0));
    assert_eq!(reply.desired_intent.as_deref(), Some("disabled"));
    assert_eq!(reply.runtime, "stopping");
    assert!(reply.in_flight);
    assert!(reply.target.is_none() && reply.session_cursor.is_none());
    assert!(!directory.path().join("observer.sqlite").exists());
    {
        let mut state = poisoned_lock(&observer.state);
        assert!(!state.finish(token));
        assert!(!state.jobs.in_flight());
    }
    let drained = observer.status("status", "status", None);
    assert_eq!(drained.runtime, "disabled");
    assert!(!drained.in_flight);
    assert_eq!(drained.revision, Some(0));
    assert!(drained.error.is_none() && drained.last_error.is_none());
    assert!(!directory.path().join("observer.sqlite").exists());
}
