//! Scheduled family-drain tests for `ActorDaemonCoordinator` (split from
//! `tests_coordinator.rs` to respect the file length cap).

use super::*;
use crate::model::checkpoint_request::CheckpointRequest;
use crate::model::working_log::CheckpointKind;
use crate::operations::daemon::git_backend::GitBackend;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn checkpoint_control_request_waits_while_blocked_behind_pending_root() {
    use crate::model::checkpoint_request::{BaseCommit, CheckpointFile, PreparedPathRole};

    let coord = Arc::new(ActorDaemonCoordinator::new());
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .arg("init")
        .output()
        .expect("git init should run");
    assert!(
        init.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    std::fs::write(repo.join("test.txt"), "checkpoint content\n").unwrap();

    let family = coord.backend.resolve_family(&repo).unwrap().0;
    let root_sid = "20260411T120000.000000-Psid-blocking-root";
    coord
        .append_pending_root_entry(&family, root_sid, 1)
        .unwrap();
    let ingest_fence_sid = "20260411T120000.000000-Psid-checkpoint-admission-fence";
    coord
        .trace_root_connection_opened(ingest_fence_sid)
        .unwrap();
    let mut trace_start = serde_json::json!({
        "event": "start",
        "sid": ingest_fence_sid,
        "argv": ["git", "commit", "-m", "still running"],
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut trace_start));

    let request = CheckpointRequest {
        trace_id: "blocked-checkpoint".to_string(),
        checkpoint_kind: CheckpointKind::Human,
        agent_id: None,
        files: vec![CheckpointFile {
            path: PathBuf::from("test.txt"),
            content: Some("checkpoint content\n".to_string()),
            repo_work_dir: repo.clone(),
            base_commit: BaseCommit::Initial,
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
        delivery_id: None,
    };

    let mut checkpoint = {
        let coord = coord.clone();
        tokio::spawn(async move { coord.ingest_checkpoint_control_payload(request).await })
    };

    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut checkpoint)
            .await
            .is_err(),
        "checkpoint control request must not complete before its sequenced side effect runs"
    );
    assert_eq!(
        coord
            .pending_checkpoint_admissions
            .load(std::sync::atomic::Ordering::Acquire),
        1,
        "checkpoint admission must be visible before sequencer insertion"
    );
    assert!(
        !coord.try_request_idle_restart(DaemonExitAction::Restart),
        "automatic restart must defer for a checkpoint waiting on trace ingestion"
    );
    assert!(
        coord
            .accepting_checkpoints
            .load(std::sync::atomic::Ordering::Acquire),
        "a deferred restart must reopen checkpoint admission"
    );

    coord.clear_trace_root_tracking(ingest_fence_sid).unwrap();

    let family = coord
        .replace_pending_root_entry(root_sid, FamilySequencerEntry::Canceled)
        .unwrap()
        .expect("pending root should belong to a family");
    coord.schedule_family_drain(&family).await.unwrap();

    let response = tokio::time::timeout(Duration::from_secs(1), checkpoint)
        .await
        .expect("checkpoint should finish once the prior root is released")
        .expect("checkpoint task should not panic")
        .expect("checkpoint request should succeed");
    assert!(
        response.ok,
        "checkpoint response should be ok: {response:?}"
    );
}

#[tokio::test]
async fn registered_coordinator_schedules_drains_and_completes_blocked_checkpoints() {
    use crate::model::checkpoint_request::{BaseCommit, CheckpointFile, PreparedPathRole};

    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.register_self();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .arg("init")
        .output()
        .expect("git init should run");
    assert!(init.status.success());
    std::fs::write(repo.join("test.txt"), "scheduled drain content\n").unwrap();

    let family = coord.backend.resolve_family(&repo).unwrap().0;
    let root_sid = "20260411T120000.000000-Psid-scheduled-root";
    coord
        .append_pending_root_entry(&family, root_sid, 1)
        .unwrap();

    let request = CheckpointRequest {
        trace_id: "scheduled-checkpoint".to_string(),
        checkpoint_kind: CheckpointKind::Human,
        agent_id: None,
        files: vec![CheckpointFile {
            path: PathBuf::from("test.txt"),
            content: Some("scheduled drain content\n".to_string()),
            repo_work_dir: repo.clone(),
            base_commit: BaseCommit::Initial,
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
        delivery_id: None,
    };
    let mut checkpoint = {
        let coord = coord.clone();
        tokio::spawn(async move { coord.ingest_checkpoint_control_payload(request).await })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut checkpoint)
            .await
            .is_err(),
        "checkpoint must stay blocked behind the pending root"
    );

    // Releasing the root schedules the family drain on its own task; the
    // blocked checkpoint must complete through that scheduled drain.
    let family = coord
        .replace_pending_root_entry(root_sid, FamilySequencerEntry::Canceled)
        .unwrap()
        .expect("pending root should belong to a family");
    coord.schedule_family_drain(&family).await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), checkpoint)
        .await
        .expect("scheduled drain should complete the blocked checkpoint")
        .expect("checkpoint task should not panic")
        .expect("checkpoint request should succeed");
    assert!(
        response.ok,
        "checkpoint response should be ok: {response:?}"
    );

    // The coalescing map must drain back to empty once the task finishes.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let empty = coord.scheduled_family_drains.lock().unwrap().is_empty();
        if empty {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "scheduled family drain task did not unregister itself"
        );
        tokio::task::yield_now().await;
    }
}

/// The transient-state GC must never remove a family's exec lock while a
/// drain holds it: handing the next acquirer a fresh mutex would let two
/// drains of the same family run concurrently, breaking the per-family
/// serialization every side effect relies on. Idle locks (held by nobody)
/// are the ones the GC exists to prune.
#[tokio::test]
async fn gc_keeps_held_exec_locks_and_prunes_idle_ones() {
    let coord = ActorDaemonCoordinator::new();

    let held = coord.side_effect_exec_lock("family-held").unwrap();
    let _guard = held.lock().await;
    let idle = coord.side_effect_exec_lock("family-idle").unwrap();
    drop(idle);

    coord.gc_stale_family_state();

    let held_after = coord.side_effect_exec_lock("family-held").unwrap();
    assert!(
        Arc::ptr_eq(&held, &held_after),
        "GC replaced a held exec lock; a concurrent acquirer would no longer \
         be excluded by the drain still holding the old lock"
    );
    let idle_after = coord.side_effect_exec_lock("family-idle").unwrap();
    assert_eq!(
        Arc::strong_count(&idle_after),
        2,
        "idle exec locks should have been pruned and recreated fresh"
    );
}

#[tokio::test]
async fn command_side_effect_concurrency_is_bounded_below_runtime_capacity() {
    let coord = ActorDaemonCoordinator::new();
    let first = coord.command_side_effect_semaphore.acquire().await.unwrap();
    let second = coord.command_side_effect_semaphore.acquire().await.unwrap();

    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            coord.command_side_effect_semaphore.acquire(),
        )
        .await
        .is_err(),
        "a third command side-effect pass must wait for the fixed two-permit bound"
    );

    drop(first);
    let third = tokio::time::timeout(
        Duration::from_secs(1),
        coord.command_side_effect_semaphore.acquire(),
    )
    .await
    .expect("releasing a permit should admit the next command")
    .expect("command side-effect semaphore is never closed");
    drop(third);
    drop(second);
}

#[tokio::test]
async fn global_family_drain_admits_only_bounded_concurrency() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    let families = ["family-a", "family-b", "family-c"];
    let mut locks = Vec::new();
    let mut guards = Vec::new();
    for family in families {
        coord
            .append_family_sequencer_entry(family, 1, FamilySequencerEntry::Canceled)
            .unwrap();
        let lock = coord.side_effect_exec_lock(family).unwrap();
        guards.push(Arc::clone(&lock).lock_owned().await);
        locks.push(lock);
    }

    let mut drain = {
        let coord = Arc::clone(&coord);
        tokio::spawn(async move { coord.drain_all_ready_family_sequencers().await })
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        let admitted = locks
            .iter()
            .filter(|lock| Arc::strong_count(lock) > 3)
            .count();
        if admitted >= 2 || tokio::time::Instant::now() >= deadline {
            assert_eq!(
                admitted, 2,
                "global drains must admit exactly the fixed family concurrency bound"
            );
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut drain)
            .await
            .is_err(),
        "held family locks must keep the global drain pending"
    );

    drop(guards);
    tokio::time::timeout(Duration::from_secs(1), drain)
        .await
        .expect("releasing family locks should complete the bounded drain")
        .expect("global drain task should not panic")
        .expect("global drain should succeed");
}

#[tokio::test]
async fn restart_fence_ignores_idle_roots_and_observes_queued_or_inflight_work() {
    let coord = ActorDaemonCoordinator::new();
    assert!(!coord.has_pending_attribution_work());

    let admission = coord.begin_checkpoint_admission().unwrap();
    assert!(coord.has_pending_attribution_work());
    assert!(
        coord.has_pending_daemon_work(),
        "await must observe a checkpoint before sequencer insertion"
    );
    drop(admission);
    assert!(!coord.has_pending_daemon_work());

    coord
        .append_pending_root_entry("family-idle", "root-idle", 1)
        .unwrap();
    assert!(
        !coord.has_pending_attribution_work(),
        "an interactive command waiting on an editor must not defer restart forever"
    );

    coord
        .append_family_sequencer_entry("family-active", 2, FamilySequencerEntry::Canceled)
        .unwrap();
    assert!(coord.has_pending_attribution_work());
    coord
        .family_sequencers_by_family
        .lock()
        .unwrap()
        .remove("family-active");
    assert!(!coord.has_pending_attribution_work());

    coord.begin_family_effect("family-running").unwrap();
    assert!(coord.has_pending_attribution_work());
    coord.end_family_effect("family-running").unwrap();
    assert!(!coord.has_pending_attribution_work());

    coord.set_checkpoint_acceptance(false).unwrap();
    coord.begin_family_effect("manual-shutdown").unwrap();
    assert!(!coord.try_request_idle_restart(DaemonExitAction::Restart));
    assert!(
        !coord
            .accepting_checkpoints
            .load(std::sync::atomic::Ordering::Acquire),
        "automatic restart must not reopen a gate owned by graceful shutdown"
    );
    coord.end_family_effect("manual-shutdown").unwrap();

    let update_restart = ActorDaemonCoordinator::new();
    assert!(update_restart.try_request_idle_restart(DaemonExitAction::RestartAfterUpdate));
    assert!(update_restart.is_shutting_down());
    assert_eq!(
        update_restart.shutdown_action(),
        DaemonExitAction::RestartAfterUpdate,
        "the atomic restart gate must preserve the selected update action"
    );
    assert!(
        !update_restart
            .accepting_checkpoints
            .load(std::sync::atomic::Ordering::Acquire),
        "the atomic restart transition must leave checkpoint admission closed"
    );

    let error_shutdown = ActorDaemonCoordinator::new();
    error_shutdown.request_shutdown();
    assert!(
        error_shutdown.begin_checkpoint_admission().is_err(),
        "every shutdown path must reject new checkpoint admission"
    );
    assert!(
        !error_shutdown
            .accepting_checkpoints
            .load(std::sync::atomic::Ordering::Acquire),
        "request_shutdown must close checkpoint admission"
    );
}

#[tokio::test]
async fn sync_await_and_shutdown_wait_for_inflight_family_effects() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let init = std::process::Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(init.status.success());

    coord.begin_family_effect("family-sync").unwrap();
    let mut sync = {
        let coord = Arc::clone(&coord);
        let repo = repo.to_string_lossy().to_string();
        tokio::spawn(async move { coord.sync_family(repo).await })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut sync)
            .await
            .is_err()
    );
    coord.end_family_effect("family-sync").unwrap();
    tokio::time::timeout(Duration::from_secs(1), sync)
        .await
        .expect("sync should finish after the effect fence clears")
        .expect("sync task should not panic")
        .expect("sync should succeed");

    coord.begin_family_effect("family-await").unwrap();
    let mut await_completion = {
        let coord = Arc::clone(&coord);
        tokio::spawn(async move { coord.await_completion(2).await })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut await_completion)
            .await
            .is_err()
    );
    coord.end_family_effect("family-await").unwrap();
    let awaited = tokio::time::timeout(Duration::from_secs(1), await_completion)
        .await
        .expect("await should finish after the effect fence clears")
        .expect("await task should not panic");
    assert!(awaited.done, "await should report a fully drained daemon");

    coord.begin_family_effect("family-shutdown").unwrap();
    let mut shutdown = {
        let coord = Arc::clone(&coord);
        tokio::spawn(async move { coord.handle_control_request(ControlRequest::Shutdown).await })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut shutdown)
            .await
            .is_err()
    );
    coord.end_family_effect("family-shutdown").unwrap();
    let response = tokio::time::timeout(Duration::from_secs(1), shutdown)
        .await
        .expect("shutdown should finish after the effect fence clears")
        .expect("shutdown task should not panic");
    assert!(
        response.ok,
        "graceful shutdown should succeed: {response:?}"
    );
    assert!(
        !coord
            .accepting_checkpoints
            .load(std::sync::atomic::Ordering::Acquire),
        "checkpoint admission must remain closed after a successful shutdown drain"
    );
}

#[test]
fn failed_graceful_shutdown_response_keeps_daemon_running() {
    struct DuplexBuffer {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl std::io::Read for DuplexBuffer {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.input, buffer)
        }
    }

    impl std::io::Write for DuplexBuffer {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            std::io::Write::write(&mut self.output, buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let coord = {
        let _runtime_guard = runtime.enter();
        Arc::new(ActorDaemonCoordinator::new())
    };
    let poison_target = Arc::clone(&coord);
    assert!(
        std::thread::spawn(move || {
            let _guard = poison_target.family_sequencers_by_family.lock().unwrap();
            panic!("poison the shutdown admission lock");
        })
        .join()
        .is_err()
    );

    let mut request = serde_json::to_vec(&ControlRequest::Shutdown).unwrap();
    request.push(b'\n');
    let io = DuplexBuffer {
        input: std::io::Cursor::new(request),
        output: Vec::new(),
    };
    let mut reader = std::io::BufReader::new(io);
    handle_control_connection_actor_reader(
        &mut reader,
        Arc::clone(&coord),
        runtime.handle().clone(),
    )
    .unwrap();

    let response: ControlResponse =
        serde_json::from_slice(&reader.into_inner().output).expect("parse shutdown response");
    assert!(
        !response.ok,
        "the poisoned admission lock must fail shutdown"
    );
    assert!(
        !coord.is_shutting_down(),
        "a failed graceful drain must keep the daemon running"
    );
    assert!(
        coord
            .accepting_checkpoints
            .load(std::sync::atomic::Ordering::Acquire),
        "failed shutdown must leave checkpoint admission open"
    );
}

#[test]
fn successful_shutdown_stops_daemon_when_response_write_fails() {
    struct FailingWriteBuffer {
        input: std::io::Cursor<Vec<u8>>,
    }

    impl std::io::Read for FailingWriteBuffer {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.input, buffer)
        }
    }

    impl std::io::Write for FailingWriteBuffer {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "client disconnected",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let coord = {
        let _runtime_guard = runtime.enter();
        Arc::new(ActorDaemonCoordinator::new())
    };
    let mut request = serde_json::to_vec(&ControlRequest::Shutdown).unwrap();
    request.push(b'\n');
    let mut reader = std::io::BufReader::new(FailingWriteBuffer {
        input: std::io::Cursor::new(request),
    });

    let result = handle_control_connection_actor_reader(
        &mut reader,
        Arc::clone(&coord),
        runtime.handle().clone(),
    );

    assert!(result.is_err(), "the disconnected client write must fail");
    assert!(
        coord.is_shutting_down(),
        "successful graceful drain must stop even if its response cannot be written"
    );
}
