use super::*;

fn bg_status(repo: &TestRepo, repo_path: &Path) -> Value {
    let mut command = Command::new(get_binary_path());
    command
        .arg("bg")
        .arg("status")
        .arg("--repo")
        .arg(repo_path)
        .current_dir(repo.path())
        .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
        .env("GITAI_TEST_DB_PATH", repo.test_db_path());
    configure_test_home_env(&mut command, repo.test_home_path());
    configure_test_daemon_env(
        &mut command,
        &repo.daemon_home_path(),
        &daemon_control_socket_path(repo),
        &daemon_trace_socket_path(repo),
    );
    let output = command.output().expect("failed to invoke bg status");
    assert!(
        output.status.success(),
        "bg status failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "bg status must print JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[cfg(not(windows))]
fn unix_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must be after the Unix epoch")
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[test]
#[cfg(not(windows))]
fn daemon_health_bg_status_reports_fenced_pipeline_without_mutating_it() {
    let repo = TestRepo::new_dedicated_daemon();
    let repo_path = repo.path().to_path_buf();
    let sid = "daemon-health-open-root";
    let mut open_root = open_local_socket_stream_with_timeout(
        &daemon_trace_socket_path(&repo),
        DAEMON_TEST_PROBE_TIMEOUT,
    )
    .expect("failed to connect trace socket");
    let started_at_ns = unix_nanos();
    write_trace_frames_to_stream(
        &mut open_root,
        &[
            json!({
                "event": "start",
                "sid": sid,
                "argv": ["git", "commit", "-m", "long-running commit"],
                "time_ns": started_at_ns,
            }),
            json!({
                "event": "def_repo",
                "sid": sid,
                "worktree": repo.path().to_string_lossy(),
                "repo": repo.path().join(".git").to_string_lossy(),
                "time_ns": started_at_ns.saturating_add(1),
            }),
        ],
    );

    let registered_deadline = Instant::now() + Duration::from_secs(5);
    let open = loop {
        let status = bg_status(&repo, &repo_path);
        if status["daemon"]["trace_roots_open_mutating"] == json!(1) {
            break status;
        }
        assert!(
            Instant::now() < registered_deadline,
            "daemon health never reported the open root: {status}"
        );
        thread::sleep(Duration::from_millis(25));
    };
    let latest_seq_before_probe = open["data"]["latest_seq"].clone();

    fs::write(repo.path().join("fenced.txt"), "fenced ai\n").unwrap();
    let mut checkpoint = repo
        .git_ai_command_without_pre_sync_for_test(&["checkpoint", "mock_ai", "fenced.txt"], &[]);
    let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
    let checkpoint_thread = thread::spawn(move || {
        let result = checkpoint.output();
        let _ = checkpoint_tx.send(result);
    });

    let checkpoint_deadline = Instant::now() + Duration::from_secs(5);
    let fenced = loop {
        assert!(
            checkpoint_rx.try_recv().is_err(),
            "checkpoint completed before its causal trace root closed"
        );
        let status = bg_status(&repo, &repo_path);
        if status["daemon"]["checkpoints_unadmitted"] == json!(1) {
            break status;
        }
        assert!(
            Instant::now() < checkpoint_deadline,
            "daemon health never reported the unadmitted checkpoint: {status}"
        );
        thread::sleep(Duration::from_millis(25));
    };

    assert_eq!(fenced["ok"], json!(true), "{fenced}");
    assert!(fenced["data"]["family_key"].is_string(), "{fenced}");
    assert_eq!(
        fenced["data"]["latest_seq"], latest_seq_before_probe,
        "status must not advance family ordering: {fenced}"
    );
    let health = &fenced["daemon"];
    assert_eq!(health["snapshot_partial"], json!(false), "{health}");
    assert!(health["uptime_seconds"].is_u64(), "{health}");
    assert_eq!(health["checkpoints_outstanding"], json!(1), "{health}");
    assert_eq!(health["checkpoints_unadmitted"], json!(1), "{health}");
    assert!(health["trace_payloads_queued"].is_u64(), "{health}");
    assert!(health["trace_ingest_seq_lag"].is_u64(), "{health}");
    assert_eq!(health["trace_roots_open_mutating"], json!(1), "{health}");
    assert!(health["trace_root_oldest_idle_ms"].is_u64(), "{health}");
    assert_eq!(
        health["sequencer_entries_pending_roots"],
        json!(1),
        "{health}"
    );
    assert!(health["sequencer_entries_commands"].is_u64(), "{health}");
    assert!(health["sequencer_entries_checkpoints"].is_u64(), "{health}");
    assert!(health["sequencer_entries_canceled"].is_u64(), "{health}");
    assert!(health["sequencer_oldest_entry_age_ms"].is_u64(), "{health}");
    assert_eq!(health["sequencer_fenced_families"], json!(1), "{health}");
    assert!(health["sequencer_stall_threshold_ms"].is_u64(), "{health}");
    assert!(health["effects_inflight_total"].is_u64(), "{health}");
    assert!(health["side_effect_errors_total"].is_u64(), "{health}");
    assert!(
        health["trace_payloads_dropped_queue_full"].is_u64(),
        "{health}"
    );
    assert!(
        health["trace_ingest_worker_disconnects"].is_u64(),
        "{health}"
    );
    assert!(health["checkpoint_requests_rejected"].is_u64(), "{health}");
    assert!(health["sequencer_stalled"].is_boolean(), "{health}");
    assert!(health["families"].is_array(), "{health}");

    let second_probe = bg_status(&repo, &repo_path);
    assert_eq!(
        second_probe["data"]["latest_seq"], latest_seq_before_probe,
        "a second status probe must not advance family ordering: {second_probe}"
    );
    assert_eq!(
        second_probe["daemon"]["checkpoints_unadmitted"],
        json!(1),
        "a status probe must not release the checkpoint fence: {second_probe}"
    );
    assert!(
        checkpoint_rx
            .recv_timeout(Duration::from_millis(250))
            .is_err(),
        "status must not release a fenced checkpoint"
    );

    write_trace_frames_to_stream(
        &mut open_root,
        &[
            json!({
                "event": "exit",
                "sid": sid,
                "code": 0,
                "time_ns": unix_nanos(),
            }),
            trace_atexit_frame(sid, 0, unix_nanos()),
        ],
    );
    let checkpoint_output = checkpoint_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("checkpoint should complete after the root closes")
        .expect("failed to run checkpoint");
    assert!(
        checkpoint_output.status.success(),
        "checkpoint failed: stdout={} stderr={}",
        String::from_utf8_lossy(&checkpoint_output.stdout),
        String::from_utf8_lossy(&checkpoint_output.stderr)
    );
    checkpoint_thread.join().unwrap();
    repo.sync_daemon();

    let drained_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let drained = bg_status(&repo, &repo_path);
        let health = &drained["daemon"];
        if health["trace_roots_open_mutating"] == json!(0)
            && health["checkpoints_outstanding"] == json!(0)
            && health["checkpoints_unadmitted"] == json!(0)
            && health["sequencer_entries_total"] == json!(0)
            && health["sequencer_fenced_families"] == json!(0)
        {
            break;
        }
        assert!(
            Instant::now() < drained_deadline,
            "daemon health never returned to idle: {drained}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn daemon_health_bg_status_works_outside_a_repository() {
    let repo = TestRepo::new_dedicated_daemon();
    let outside = tempfile::tempdir().expect("failed to create non-repository directory");
    let status = bg_status(&repo, outside.path());

    assert_eq!(status["ok"], json!(true), "{status}");
    assert_eq!(status["git_repo"], json!(false), "{status}");
    assert_eq!(status["daemon_running"], json!(true), "{status}");
    assert!(status.get("data").is_none(), "{status}");
    assert!(status["daemon"]["uptime_seconds"].is_u64(), "{status}");
}
