use super::super::*;

#[test]
#[serial]
fn daemon_pure_trace_socket_commit_after_ai_checkpoint_preserves_ai_replacement_attribution() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let file_path = repo.path().join("daemon-ai-replace.txt");
    let completion_baseline = repo.daemon_total_completion_count();
    let mut expected_top_level_completions = 0u64;

    fs::write(&file_path, "old line\n").expect("failed to write base contents");
    traced_git_with_env(
        &repo,
        &["add", "daemon-ai-replace.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("base add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "base"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("base commit should succeed");

    fs::write(&file_path, "new line from ai\n").expect("failed to write ai contents");
    expected_top_level_completions += 1;
    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", "daemon-ai-replace.txt"],
        &[("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")],
    )
    .expect("ai checkpoint should succeed");
    traced_git_with_env(
        &repo,
        &["add", "daemon-ai-replace.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "commit ai replacement"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("commit should succeed");

    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    let mut file = repo.filename("daemon-ai-replace.txt");
    file.assert_lines_and_blame(lines!["new line from ai".ai()]);
}

#[test]
#[cfg(not(windows))]
fn daemon_checkpoint_resolution_applies_total_content_budget() {
    let mut repo = TestRepo::new_dedicated_daemon();
    repo.patch_git_ai_config(|p| {
        p.max_checkpoint_file_size_bytes = Some(1024);
        p.max_checkpoint_total_size_bytes = Some(96);
        p.max_checkpoint_total_lines = Some(1000);
    });

    let control_socket = daemon_control_socket_path(&repo);
    fs::write(repo.path().join("a_kept.txt"), "a".repeat(48)).unwrap();
    fs::write(repo.path().join("z_skipped.txt"), "z".repeat(64)).unwrap();

    let request = CheckpointRequest {
        trace_id: "daemon-checkpoint-budget".to_string(),
        checkpoint_kind: CheckpointKind::Human,
        agent_id: None,
        files: vec![
            CheckpointFile {
                path: PathBuf::from("a_kept.txt"),
                content: Some("a".repeat(48)),
                repo_work_dir: repo.path().to_path_buf(),
                base_commit: BaseCommit::Initial,
            },
            CheckpointFile {
                path: PathBuf::from("z_skipped.txt"),
                content: Some("z".repeat(64)),
                repo_work_dir: repo.path().to_path_buf(),
                base_commit: BaseCommit::Initial,
            },
        ],
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
        Duration::from_secs(5),
    )
    .expect("checkpoint control request should succeed");

    assert!(
        response.ok,
        "checkpoint control request should succeed: {:?}",
        response
    );

    let checkpoints = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    assert_eq!(checkpoints.len(), 1, "expected exactly one checkpoint");
    let checkpoint = checkpoints.last().unwrap();
    assert_eq!(
        checkpoint.entries.len(),
        1,
        "expected daemon resolver to apply aggregate content budget"
    );
    assert_eq!(checkpoint.entries[0].file, "a_kept.txt");
}

#[test]
#[cfg(windows)]
fn daemon_windows_control_pipe_worker_exhaustion_does_not_block_later_control_requests() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_TEST_WINDOWS_CONTROL_PIPE_WORKERS", "2"),
            ("GIT_AI_DAEMON_UPDATE_CHECK_INTERVAL", "86400"),
            ("GIT_AI_DAEMON_MAX_UPTIME_SECS", "86400"),
        ],
    );
    let control_socket = daemon_control_socket_path(&repo);

    let _stalled_streams = (0..2)
        .map(|_| {
            open_local_socket_stream_with_timeout(&control_socket, DAEMON_TEST_PROBE_TIMEOUT)
                .expect("failed to open stalled control pipe")
        })
        .collect::<Vec<_>>();
    thread::sleep(Duration::from_millis(100));

    let response = send_control_request(
        &control_socket,
        &ControlRequest::StatusFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
    )
    .expect("control request should complete after every original pipe worker is stalled");

    assert!(
        response.ok,
        "later control request should return an ok response: {:?}",
        response
    );
    daemon.shutdown();
}

#[test]
#[cfg(windows)]
fn daemon_windows_trace_pipe_worker_exhaustion_does_not_block_later_trace_connections() {
    let repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_WINDOWS_TRACE_PIPE_WORKERS", "2"),
        ("GIT_AI_DAEMON_UPDATE_CHECK_INTERVAL", "86400"),
        ("GIT_AI_DAEMON_MAX_UPTIME_SECS", "86400"),
    ]);
    let trace_socket = daemon_trace_socket_path(&repo);
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();

    let _stalled_streams = (0..2)
        .map(|_| {
            open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
                .expect("failed to open stalled trace pipe")
        })
        .collect::<Vec<_>>();
    thread::sleep(Duration::from_millis(100));

    let session = repos::test_repo::new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");
    send_trace_frames(
        &trace_socket,
        &[
            json!({
                "event": "start",
                "sid": "windows-exhaustion-followup",
                "argv": ["git", "-c", session_arg, "commit", "--dry-run", "-m", "synthetic"],
                "time_ns": 15_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "windows-exhaustion-followup",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 15_001u64,
            }),
            json!({
                "event": "exit",
                "sid": "windows-exhaustion-followup",
                "code": 0,
                "time_ns": 15_100u64,
            }),
            trace_atexit_frame("windows-exhaustion-followup", 0, 15_101u64),
        ],
    );

    repo.sync_daemon_external_completion_sessions(&[session]);
}

#[test]
#[serial]
#[cfg(not(windows))]
fn daemon_trace_ingest_backpressure_shuts_down_without_blocking_listener() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_TEST_TRACE_INGEST_QUEUE_CAPACITY", "1"),
            ("GIT_AI_TEST_TRACE_INGEST_WORKER_START_DELAY_MS", "5000"),
            ("GIT_AI_DAEMON_UPDATE_CHECK_INTERVAL", "86400"),
            ("GIT_AI_DAEMON_MAX_UPTIME_SECS", "86400"),
        ],
    );
    let trace_socket = daemon_trace_socket_path(&repo);
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().to_string();

    let mut stream =
        open_local_socket_stream_with_timeout(&trace_socket, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to connect trace socket");
    write_trace_frames_to_stream(
        &mut stream,
        &[
            json!({
                "event": "start",
                "sid": "backpressure-root",
                "argv": ["git", "commit", "-m", "synthetic"],
                "time_ns": 20_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "backpressure-root",
                "worktree": worktree,
                "repo": git_dir,
                "time_ns": 20_001u64,
            }),
        ],
    );

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if daemon
            .child
            .try_wait()
            .expect("failed to poll daemon")
            .is_some()
        {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }

    panic!("daemon did not fail closed within 2s when trace ingest queue capacity was exhausted");
}
