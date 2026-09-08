use super::super::*;

#[test]
#[cfg(unix)]
#[serial]
fn daemon_symlink_repo_path_trace_and_status_use_same_family() {
    let unique = format!(
        "git-ai-symlink-family-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let real_path = std::env::temp_dir().join(format!("{unique}-real"));
    let alias_path = std::env::temp_dir().join(format!("{unique}-alias"));
    fs::create_dir_all(&real_path).expect("failed to create real test repo path");
    std::os::unix::fs::symlink(&real_path, &alias_path).expect("failed to create repo symlink");

    let repo = TestRepo::new_at_path_with_daemon_scope(&alias_path, DaemonTestScope::Dedicated);
    assert_ne!(
        repo.path(),
        &repo.canonical_path(),
        "test must exercise an alias path distinct from its canonical path"
    );

    let completion_baseline = repo.daemon_total_completion_count();
    fs::write(repo.path().join("alias.txt"), "alias\n").expect("failed writing aliased file");
    repo.git(&["add", "alias.txt"])
        .expect("aliased path git add should succeed");
    repo.wait_for_daemon_total_completion_count(
        completion_baseline,
        completion_baseline.saturating_add(1),
    );

    let status = send_control_request(
        &daemon_control_socket_path(&repo),
        &ControlRequest::StatusFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
    )
    .expect("daemon status request should succeed for aliased path");
    assert!(status.ok, "aliased path daemon status should be ok");

    let checkpoint_baseline = repo.daemon_total_completion_count();
    fs::write(repo.path().join("alias.txt"), "alias\nhuman\n")
        .expect("failed writing human aliased file");
    repo.git_ai(&["checkpoint", "human"])
        .expect("aliased path human checkpoint should succeed");
    repo.wait_for_next_daemon_checkpoint_completion(checkpoint_baseline);

    let watermark_for = |path: &Path| {
        let response = send_control_request(
            &daemon_control_socket_path(&repo),
            &ControlRequest::SnapshotWatermarks {
                repo_working_dir: path.to_string_lossy().to_string(),
            },
        )
        .expect("daemon watermark request should succeed");
        assert!(
            response.ok,
            "daemon watermark response should be ok for {}: {:?}",
            path.display(),
            response.error
        );
        response
            .data
            .as_ref()
            .and_then(|data| data.get("worktree_watermark"))
            .and_then(serde_json::Value::as_u64)
    };

    assert!(
        watermark_for(repo.path()).is_some(),
        "aliased worktree path should see full-checkpoint watermark"
    );
    assert!(
        watermark_for(&repo.canonical_path()).is_some(),
        "canonical worktree path should see same full-checkpoint watermark"
    );

    let _ = fs::remove_file(&alias_path);
}

#[test]
fn daemon_trace_current_dir_commands_reserve_order_from_def_repo() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();

    fs::write(repo.path().join("base.txt"), "base\n").expect("failed to write base");
    repo.git_og(&["add", "base.txt"])
        .expect("base add should succeed");
    repo.git_og(&["commit", "-m", "base"])
        .expect("base commit should succeed");

    fs::write(repo.path().join("a.txt"), "a ai\n").expect("failed to write a.txt");
    repo.git_ai(&["checkpoint", "mock_ai", "a.txt"])
        .expect("a checkpoint should succeed");
    repo.git_og(&["add", "a.txt"])
        .expect("a add should succeed");
    repo.git_og(&["commit", "-m", "commit A"])
        .expect("commit A should succeed");
    let commit_a = repo
        .git_og(&["rev-parse", "HEAD"])
        .expect("rev-parse A should succeed")
        .trim()
        .to_string();

    fs::write(repo.path().join("b.txt"), "b ai\n").expect("failed to write b.txt");
    repo.git_ai(&["checkpoint", "mock_ai", "b.txt"])
        .expect("b checkpoint should succeed");
    repo.git_og(&["add", "b.txt"])
        .expect("b add should succeed");
    repo.git_og(&["commit", "-m", "commit B"])
        .expect("commit B should succeed");
    let commit_b = repo
        .git_og(&["rev-parse", "HEAD"])
        .expect("rev-parse B should succeed")
        .trim()
        .to_string();

    let session_a = repos::test_repo::new_daemon_test_sync_session_id();
    let session_b = repos::test_repo::new_daemon_test_sync_session_id();
    let session_arg_a = format!("git-ai.testSyncSession={session_a}");
    let session_arg_b = format!("git-ai.testSyncSession={session_b}");

    send_trace_frames(
        &trace_socket,
        &[
            json!({
                "event": "start",
                "sid": "current-dir-a",
                "argv": ["git", "-c", session_arg_a, "commit", "-m", "commit A"],
                "time_ns": 1_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "current-dir-a",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 1_001u64,
            }),
            json!({
                "event": "start",
                "sid": "current-dir-b",
                "argv": ["git", "-c", session_arg_b, "commit", "-m", "commit B"],
                "time_ns": 2_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "current-dir-b",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 2_001u64,
            }),
            json!({
                "event": "exit",
                "sid": "current-dir-b",
                "code": 0,
                "time_ns": 2_100u64,
            }),
            trace_atexit_frame("current-dir-b", 0, 2_101u64),
            json!({
                "event": "exit",
                "sid": "current-dir-a",
                "code": 0,
                "time_ns": 1_100u64,
            }),
            trace_atexit_frame("current-dir-a", 0, 1_101u64),
        ],
    );
    repo.sync_daemon_external_completion_sessions(&[session_a, session_b]);

    assert!(
        repo.read_authorship_note(&commit_a).is_some(),
        "commit A should retain a note even when its trace exit is delivered after commit B"
    );
    assert!(
        repo.read_authorship_note(&commit_b).is_some(),
        "commit B should have a note"
    );
    let mut file_a = repo.filename("a.txt");
    file_a.assert_committed_lines(lines!["a ai".ai()]);
    let mut file_b = repo.filename("b.txt");
    file_b.assert_committed_lines(lines!["b ai".ai()]);
}

#[test]
#[cfg(not(windows))]
fn daemon_sync_for_unrelated_family_ignores_open_mutating_root_of_other_family() {
    let repo = TestRepo::new_dedicated_daemon();
    let trace_socket = daemon_trace_socket_path(&repo);
    let control_socket = daemon_control_socket_path(&repo);
    let repo_working_dir = repo_workdir_string(&repo);

    // A second repository family: the daemon resolves families from the
    // filesystem alone, so a minimal worktree with a .git dir suffices.
    let other = tempfile::tempdir().expect("failed to create other family dir");
    let other_worktree = other.path().join("other-repo");
    let other_git_dir = other_worktree.join(".git");
    fs::create_dir_all(&other_git_dir).expect("failed to create other .git dir");
    fs::write(other_git_dir.join("HEAD"), "ref: refs/heads/main\n")
        .expect("failed to write other HEAD");
    let other_working_dir = other_worktree
        .canonicalize()
        .expect("failed to canonicalize other worktree")
        .to_string_lossy()
        .to_string();

    // Hold open a mutating trace root attributed to the other family: start +
    // def_repo frames, no exit/atexit, connection kept open.
    let mut open_root_stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open trace socket for the long-running root");
    write_trace_frames_to_stream(
        &mut open_root_stream,
        &[
            json!({
                "event": "start",
                "sid": "other-family-open-root",
                "argv": ["git", "commit", "-m", "long-running commit"],
                "time_ns": 1_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "other-family-open-root",
                "worktree": other_worktree.to_string_lossy(),
                "repo": other_git_dir.to_string_lossy(),
                "time_ns": 1_001u64,
            }),
        ],
    );

    // Wait until the daemon has registered the open root: syncs of the
    // originating family start hitting the drain fence (client timeout).
    let registered_deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let probe = send_control_request_with_timeout(
            &control_socket,
            &ControlRequest::SyncFamily {
                repo_working_dir: other_working_dir.clone(),
            },
            Duration::from_millis(250),
        );
        if probe.is_err() {
            break;
        }
        assert!(
            std::time::Instant::now() < registered_deadline,
            "the open mutating root never started fencing syncs of its own family"
        );
        thread::sleep(Duration::from_millis(25));
    }

    // An unrelated family must sync promptly while the other family's
    // mutating root stays open.
    let response = send_control_request_with_timeout(
        &control_socket,
        &ControlRequest::SyncFamily {
            repo_working_dir: repo_working_dir.clone(),
        },
        Duration::from_secs(10),
    )
    .expect("sync of an unrelated family must not wait on another family's open mutating root");
    assert!(
        response.ok,
        "unrelated family sync should succeed: {:?}",
        response.error
    );

    // Guard: the originating family stays fenced until its root closes.
    let (done_tx, done_rx) = mpsc::channel();
    let waiter_control_socket = control_socket.clone();
    let waiter_working_dir = other_working_dir.clone();
    let waiter = thread::spawn(move || {
        let result = send_control_request_with_timeout(
            &waiter_control_socket,
            &ControlRequest::SyncFamily {
                repo_working_dir: waiter_working_dir,
            },
            Duration::from_secs(30),
        );
        let _ = done_tx.send(result.is_ok());
    });
    assert!(
        done_rx.recv_timeout(Duration::from_secs(1)).is_err(),
        "the originating family must stay fenced while its mutating root is open"
    );

    drop(open_root_stream);
    let completed = done_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the originating family sync should complete once the root closes");
    assert!(
        completed,
        "the originating family sync should get a response after the root closes"
    );
    waiter.join().expect("waiter thread should not panic");
}

#[test]
#[serial]
#[cfg(not(windows))]
fn daemon_reaps_idle_mutating_root_after_activity_stops() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_DAEMON_SOCKET_HEALTH_CHECK_SECS", "1"),
            ("GIT_AI_TEST_TRACE_ROOT_IDLE_TIMEOUT_MS", "750"),
            ("GIT_AI_DAEMON_UPDATE_CHECK_INTERVAL", "86400"),
            ("GIT_AI_DAEMON_MAX_UPTIME_SECS", "86400"),
        ],
    );
    let trace_socket = daemon_trace_socket_path(&repo);
    let control_socket = daemon_control_socket_path(&repo);
    let repo_working_dir = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();
    let root_sid = "idle-mutating-root";

    let mut open_root_stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open trace socket for the stalled root");
    write_trace_frames_to_stream(
        &mut open_root_stream,
        &[
            json!({
                "event": "start",
                "sid": root_sid,
                "argv": ["git", "commit", "-m", "stalled commit"],
                "time_ns": 1_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": root_sid,
                "worktree": repo_working_dir,
                "repo": git_dir,
                "time_ns": 1_001u64,
            }),
        ],
    );

    let registered_deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let probe = send_control_request_with_timeout(
            &control_socket,
            &ControlRequest::SyncFamily {
                repo_working_dir: repo_workdir_string(&repo),
            },
            Duration::from_millis(200),
        );
        if probe.is_err() {
            break;
        }
        assert!(
            std::time::Instant::now() < registered_deadline,
            "the mutating root never started fencing its family"
        );
        thread::sleep(Duration::from_millis(25));
    }

    for sequence in 0..6u64 {
        thread::sleep(Duration::from_millis(300));
        write_trace_frames_to_stream(
            &mut open_root_stream,
            &[json!({
                "event": "data",
                "sid": root_sid,
                "key": "progress",
                "value": sequence,
                "time_ns": 2_000u64 + sequence,
            })],
        );
    }

    assert!(
        send_control_request_with_timeout(
            &control_socket,
            &ControlRequest::SyncFamily {
                repo_working_dir: repo_workdir_string(&repo),
            },
            Duration::from_millis(200),
        )
        .is_err(),
        "activity must refresh the idle deadline for a long-running root"
    );

    let response = send_control_request_with_timeout(
        &control_socket,
        &ControlRequest::SyncFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
        Duration::from_secs(5),
    )
    .expect("the family fence should pass once the mutating root becomes idle");
    assert!(
        response.ok,
        "the family sync should succeed after idle-root reclamation: {:?}",
        response.error
    );
}
