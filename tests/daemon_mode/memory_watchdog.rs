use super::*;

/// The watchdog must abort at 85% of the configured limit, flush a durable
/// stderr diagnostic, and give the emergency daemon-log upload a bounded
/// chance to land — all without draining in-flight work. An open mutating
/// trace root (which a graceful drain would wait on indefinitely) proves the
/// abort does not drain.
#[test]
#[serial]
#[cfg(not(windows))]
fn daemon_memory_threshold_logs_uploads_and_aborts_without_draining() {
    assert_emergency_shutdown_without_drain("25");
}

#[test]
#[serial]
#[cfg(not(windows))]
fn daemon_memory_slow_polling_does_not_count_toward_shutdown_deadline() {
    assert_emergency_shutdown_without_drain("150");
}

#[cfg(not(windows))]
fn assert_emergency_shutdown_without_drain(poll_ms: &str) {
    let mut mock_api = MockApiServer::start();
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.patch_git_ai_config(|patch| {
        patch.daemon_memory_limit_mb = Some(1024);
        patch.telemetry = Some("on".to_string());
    });

    let mut samples = vec!["100"; 40];
    samples.push("900");
    let sample_sequence = samples.join(",");
    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            (
                "GIT_AI_TEST_DAEMON_CURRENT_RSS_MB_SEQUENCE",
                sample_sequence.as_str(),
            ),
            ("GIT_AI_TEST_DAEMON_MEMORY_POLL_MS", poll_ms),
            ("GIT_AI_API_BASE_URL", mock_api.base_url()),
            ("GIT_AI_API_KEY", "test-api-key"),
        ],
    );

    // Hold a mutating trace root open: a graceful shutdown would wait for the
    // root to close, so a prompt abort proves nothing was drained.
    let mut open_root_stream =
        open_local_socket_stream_with_timeout(&daemon.trace_socket_path, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to open trace socket for the long-running root");
    write_trace_frames_to_stream(
        &mut open_root_stream,
        &[json!({
            "event": "start",
            "sid": "memory-emergency-open-root",
            "argv": ["git", "commit", "-m", "long-running commit"],
            "worktree": repo.path().to_string_lossy(),
            "time_ns": 1_000u64,
        })],
    );

    // The threshold is reached only after the scripted RSS samples have been
    // polled. Measure shutdown from its diagnostic, not from before those polls.
    let threshold_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if daemon
            .diagnostic_contents()
            .contains("memory emergency threshold reached")
        {
            break;
        }
        assert!(
            Instant::now() < threshold_deadline,
            "memory emergency threshold was not reached"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let shutdown_deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        assert!(
            Instant::now() < shutdown_deadline,
            "memory emergency shutdown waited on in-flight work"
        );
        if let Some(status) = daemon.child.try_wait().expect("poll emergency daemon stop") {
            break status;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(!status.success(), "85% threshold should abort the daemon");
    let requests = mock_api.collect_requests();
    assert!(
        requests
            .iter()
            .filter(|request| request["path"] == "/worker/logs/upload")
            .flat_map(|request| request["body"]["events"].as_array().into_iter().flatten())
            .any(|event| event["message"] == "daemon memory emergency threshold reached"),
        "emergency diagnostic was not uploaded before shutdown: {requests:?}"
    );
    drop(open_root_stream);
}

#[test]
#[serial]
fn daemon_memory_limit_below_startup_usage_aborts_without_restart_loop() {
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.patch_git_ai_config(|patch| {
        patch.daemon_memory_limit_mb = Some(1024);
    });

    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_TEST_DAEMON_CURRENT_RSS_MB_SEQUENCE", "1024"),
            ("GIT_AI_TEST_DAEMON_MEMORY_POLL_MS", "500"),
        ],
    );
    let status = daemon.child.wait().expect("wait for daemon stop");

    assert!(!status.success(), "startup-over-limit should abort");
    thread::sleep(Duration::from_millis(300));
    assert!(
        send_control_request(&daemon.control_socket_path, &ControlRequest::Ping).is_err(),
        "startup-over-limit daemon must not respawn"
    );
    let logs = daemon.diagnostic_contents();
    assert!(
        logs.contains("memory emergency threshold reached"),
        "missing startup-limit diagnostic:\n{logs}"
    );
}

#[test]
#[serial]
fn daemon_memory_hard_limit_aborts_without_restart() {
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.patch_git_ai_config(|patch| {
        patch.daemon_memory_limit_mb = Some(1024);
    });

    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_TEST_DAEMON_CURRENT_RSS_MB_SEQUENCE", "100,100,1024"),
            ("GIT_AI_TEST_DAEMON_PEAK_RSS_MB_SEQUENCE", "2048"),
            ("GIT_AI_TEST_DAEMON_MEMORY_POLL_MS", "100"),
        ],
    );
    let status = daemon.child.wait().expect("wait for daemon abort");

    assert!(!status.success(), "hard memory limit must abort the daemon");
    thread::sleep(Duration::from_millis(300));
    assert!(
        send_control_request(&daemon.control_socket_path, &ControlRequest::Ping).is_err(),
        "hard-aborted daemon must not respawn"
    );
    let logs = daemon.diagnostic_contents();
    assert!(logs.contains("current RSS 1073741824 bytes"), "{logs}");
    assert!(logs.contains("peak RSS 2147483648 bytes"), "{logs}");
    assert!(
        logs.contains("memory emergency threshold reached"),
        "missing hard-limit diagnostic:\n{logs}"
    );
}
