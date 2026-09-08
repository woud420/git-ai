#[cfg(not(windows))]
use super::super::*;

#[test]
#[cfg(not(windows))]
fn daemon_trace_listener_stalled_connection_does_not_block_later_trace_connections() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();

    let _stalled_stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open stalled trace socket");

    let session = repos::test_repo::new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");

    send_trace_frames(
        &trace_socket,
        &[
            json!({
                "event": "start",
                "sid": "stalled-listener-followup",
                "argv": [
                    "git",
                    "-c",
                    session_arg,
                    "commit",
                    "--dry-run",
                    "-m",
                    "synthetic",
                ],
                "time_ns": 10_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "stalled-listener-followup",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 10_001u64,
            }),
            json!({
                "event": "exit",
                "sid": "stalled-listener-followup",
                "code": 0,
                "time_ns": 10_100u64,
            }),
            trace_atexit_frame("stalled-listener-followup", 0, 10_101u64),
        ],
    );

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if repo
            .daemon_completion_entries()
            .iter()
            .any(|entry| entry.test_sync_session.as_deref() == Some(session.as_str()))
        {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }

    panic!(
        "daemon did not process a later trace connection while an earlier trace socket was stalled"
    );
}

#[test]
#[cfg(not(windows))]
fn daemon_stalled_unidentified_trace_connection_does_not_block_checkpoint_control_request() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let control_socket = daemon_control_socket_path(&repo);

    let _stalled_stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open stalled trace socket");
    thread::sleep(Duration::from_millis(150));

    let file_path = repo.path().join("checkpoint-after-stalled-trace.txt");
    fs::write(&file_path, "checkpoint content\n").unwrap();

    let request = CheckpointRequest {
        trace_id: "checkpoint-after-stalled-trace".to_string(),
        checkpoint_kind: CheckpointKind::Human,
        agent_id: None,
        files: vec![CheckpointFile {
            path: PathBuf::from("checkpoint-after-stalled-trace.txt"),
            content: Some("checkpoint content\n".to_string()),
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Initial,
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: Default::default(),
        delivery_id: None,
    };

    let response = send_control_request_with_timeout(
        &control_socket,
        &ControlRequest::CheckpointRun {
            request: Box::new(request),
        },
        Duration::from_millis(500),
    )
    .expect("checkpoint control request should not block on unidentified trace sockets");

    assert!(
        response.ok,
        "checkpoint control request should succeed: {:?}",
        response
    );
}

#[test]
#[cfg(not(windows))]
fn daemon_stalled_unidentified_trace_connection_does_not_block_sync_control_request() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let control_socket = daemon_control_socket_path(&repo);

    let _stalled_stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open stalled trace socket");
    thread::sleep(Duration::from_millis(150));

    let response = send_control_request_with_timeout(
        &control_socket,
        &ControlRequest::SyncFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
        Duration::from_millis(500),
    )
    .expect("sync control request should not block on unidentified trace sockets");

    assert!(
        response.ok,
        "sync control request should succeed: {:?}",
        response
    );
}

#[test]
#[cfg(not(windows))]
fn daemon_partial_trace_line_does_not_block_checkpoint_control_request() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let control_socket = daemon_control_socket_path(&repo);

    let mut stalled_stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open stalled trace socket");
    stalled_stream
        .write_all(br#"{"event":"start""#)
        .expect("failed to write partial trace frame");
    stalled_stream
        .flush()
        .expect("failed to flush partial trace frame");
    thread::sleep(Duration::from_millis(150));

    let file_path = repo.path().join("checkpoint-after-partial-trace.txt");
    fs::write(&file_path, "checkpoint content\n").unwrap();

    let request = CheckpointRequest {
        trace_id: "checkpoint-after-partial-trace".to_string(),
        checkpoint_kind: CheckpointKind::Human,
        agent_id: None,
        files: vec![CheckpointFile {
            path: PathBuf::from("checkpoint-after-partial-trace.txt"),
            content: Some("checkpoint content\n".to_string()),
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Initial,
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: Default::default(),
        delivery_id: None,
    };

    let response = send_control_request_with_timeout(
        &control_socket,
        &ControlRequest::CheckpointRun {
            request: Box::new(request),
        },
        Duration::from_millis(500),
    )
    .expect("checkpoint control request should not block on incomplete trace frames");

    assert!(
        response.ok,
        "checkpoint control request should succeed: {:?}",
        response
    );
}

#[test]
#[cfg(not(windows))]
fn daemon_trace_listener_partial_line_does_not_block_later_trace_connections() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();

    let mut stalled_stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open stalled trace socket");
    stalled_stream
        .write_all(br#"{"event":"start""#)
        .expect("failed to write partial trace frame");
    stalled_stream
        .flush()
        .expect("failed to flush partial trace frame");
    thread::sleep(Duration::from_millis(200));

    let session = repos::test_repo::new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");

    send_trace_frames(
        &trace_socket,
        &[
            json!({
                "event": "start",
                "sid": "partial-listener-followup",
                "argv": [
                    "git",
                    "-c",
                    session_arg,
                    "commit",
                    "--dry-run",
                    "-m",
                    "synthetic",
                ],
                "time_ns": 10_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "partial-listener-followup",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 10_001u64,
            }),
            json!({
                "event": "exit",
                "sid": "partial-listener-followup",
                "code": 0,
                "time_ns": 10_100u64,
            }),
            trace_atexit_frame("partial-listener-followup", 0, 10_101u64),
        ],
    );

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if repo
            .daemon_completion_entries()
            .iter()
            .any(|entry| entry.test_sync_session.as_deref() == Some(session.as_str()))
        {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }

    panic!(
        "daemon did not process a later trace connection while an earlier trace socket held a partial line"
    );
}

#[test]
#[cfg(not(windows))]
fn daemon_trace_connection_close_without_atexit_does_not_block_later_trace() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();

    send_trace_frames(
        &trace_socket,
        &[
            json!({
                "event": "start",
                "sid": "closed-before-atexit",
                "argv": ["git", "commit", "--dry-run", "-m", "incomplete"],
                "time_ns": 9_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "closed-before-atexit",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 9_001u64,
            }),
            json!({
                "event": "exit",
                "sid": "closed-before-atexit",
                "code": 0,
                "time_ns": 9_100u64,
            }),
        ],
    );

    let session = repos::test_repo::new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();

    send_trace_frames(
        &trace_socket,
        &[
            json!({
                "event": "start",
                "sid": "complete-after-closed-root",
                "argv": [
                    "git",
                    "-c",
                    session_arg,
                    "commit",
                    "--dry-run",
                    "-m",
                    "synthetic",
                ],
                "time_ns": 10_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "complete-after-closed-root",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 10_001u64,
            }),
            json!({
                "event": "exit",
                "sid": "complete-after-closed-root",
                "code": 0,
                "time_ns": 10_100u64,
            }),
            trace_atexit_frame("complete-after-closed-root", 0, 10_101u64),
        ],
    );

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if repo
            .daemon_completion_entries()
            .iter()
            .any(|entry| entry.test_sync_session.as_deref() == Some(session.as_str()))
        {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }

    panic!("daemon did not process a later trace after a mutating root closed before atexit");
}

#[test]
#[cfg(not(windows))]
fn daemon_control_listener_stalled_connection_does_not_block_later_control_requests() {
    let repo = TestRepo::new_dedicated_daemon();
    let control_socket = daemon_control_socket_path(&repo);
    let _stalled_stream =
        open_local_socket_stream_with_timeout(&control_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open stalled control socket");
    thread::sleep(Duration::from_millis(50));

    let response = send_control_request(
        &control_socket,
        &ControlRequest::StatusFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
    )
    .expect("later control request should complete while an earlier control socket is stalled");

    assert!(
        response.ok,
        "later control request should return an ok response: {:?}",
        response
    );
}
