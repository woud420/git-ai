use super::*;
use crate::model::jj_observation::JJ_OBSERVATION_READER_PROFILE;
use crate::model::jj_observer::{JjObserverTarget, paths};
use crate::model::repository::sqlite::open_with_flags_and_memory_limits;
use rusqlite::{OpenFlags, params};

async fn blocked_controller() -> (
    tempfile::TempDir,
    Arc<ActorDaemonCoordinator>,
    Observer,
    StoredIntent,
) {
    let (directory, coordinator, observer) = super::super::tests::controller();
    let mut saved = StoredIntent {
        schema_version: 1,
        revision: 1,
        target: StoredTarget {
            journal_path_hex: paths::encode(&directory.path().join("missing-journal.sqlite"))
                .unwrap(),
            workspace_path_hex: paths::encode(directory.path()).unwrap(),
            metadata: JjObserverTarget {
                source_id: "1".repeat(64),
                initialization_receipt_id: "2".repeat(64),
                reader_profile: JJ_OBSERVATION_READER_PROFILE.to_owned(),
                baseline_id: "3".repeat(64),
                baseline_generation: 1,
                workspace_name: "default".to_owned(),
                attachment_id: "4".repeat(64),
            },
        },
        enabled: true,
        blocked: None,
    };
    store::replace(&observer.path, &None, &saved).unwrap();
    saved.revision = i64::MAX as u64;
    let connection =
        open_with_flags_and_memory_limits(&observer.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE jj_observer_intent SET revision=?1,payload=?2 WHERE slot=1",
                params![i64::MAX, serde_json::to_vec(&saved).unwrap()],
            )
            .unwrap(),
        1,
    );
    drop(connection);
    assert_eq!(store::load(&observer.path).unwrap().as_ref(), Some(&saved));
    {
        let mut state = poisoned_lock(&observer.state);
        state.intent = Some(saved.clone());
        state.stopped = false;
        assert!(state.error.is_none() && !state.jobs.in_flight());
    }
    {
        let _gate = observer.mutation.lock().await;
        observer
            .block(Error::new(
                "admission_unavailable",
                "injected sample refusal",
            ))
            .await;
    }
    assert_pinned(&observer, &saved);
    (directory, coordinator, observer, saved)
}

fn assert_pinned(observer: &Observer, saved: &StoredIntent) {
    let state = poisoned_lock(&observer.state);
    assert!(state.stopped, "failed activation cleared volatile stop");
    assert!(!state.jobs.in_flight());
    assert_eq!(state.intent.as_ref(), Some(saved));
    assert!(state.intent.as_ref().unwrap().blocked.is_none());
    let error = state.error.as_ref().expect("volatile error retained");
    assert_eq!(error.code, "intent_persistence");
    assert!(!error.persisted);
    assert_eq!(state.runtime(), "blocked");
    drop(state);
    assert_eq!(store::load(&observer.path).unwrap().as_ref(), Some(saved));
    let journal = paths::decode(&saved.target.journal_path_hex).unwrap();
    assert!(!journal.exists());
}

#[tokio::test]
async fn jj_observer_controller_resume_revision_exhaustion_refuses_before_reserving() {
    let (_directory, _coordinator, observer, saved) = blocked_controller().await;
    {
        let mut state = poisoned_lock(&observer.state);
        let epoch = state.jobs.epoch;
        let result = observer.prepare_activation(&mut state, &Activation::Resume);
        assert!(result.is_err(), "exhausted revision entered activation");
        assert_eq!(result.err().unwrap().code, "intent_persistence");
        assert_eq!(state.jobs.epoch, epoch);
        assert!(!state.jobs.in_flight());
    }
    assert_pinned(&observer, &saved);
}

#[tokio::test]
async fn jj_observer_controller_failed_resume_retains_volatile_stop_and_saved_intent() {
    let (_directory, _coordinator, observer, saved) = blocked_controller().await;
    let reply = tokio::time::timeout(Duration::from_secs(10), observer.resume())
        .await
        .unwrap();
    assert_eq!(reply.action, "observer_resume");
    assert_ne!(reply.disposition, "already_enabled");
    assert!(reply.error.is_some());
    assert_eq!(reply.disposition, "error");
    assert_eq!(reply.revision, Some(i64::MAX as u64));
    assert_eq!(reply.desired_intent.as_deref(), Some("enabled"));
    assert_pinned(&observer, &saved);
}

#[tokio::test]
async fn jj_observer_controller_enable_requires_resume_for_volatile_block() {
    let (_directory, _coordinator, observer, saved) = blocked_controller().await;
    let reply = tokio::time::timeout(
        Duration::from_secs(10),
        observer.enable(
            saved.target.journal_path_hex.clone(),
            saved.target.workspace_path_hex.clone(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(reply.action, "observer_enable");
    assert_eq!(reply.disposition, "error");
    assert_eq!(reply.error.as_ref().unwrap().code, "resume_required");
    assert_eq!(reply.revision, Some(i64::MAX as u64));
    assert_pinned(&observer, &saved);
}
