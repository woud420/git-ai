use super::*;

fn assert_oversized_trace_frame_stops_daemon(identify_root: bool) {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_DAEMON_UPDATE_CHECK_INTERVAL", "86400"),
            ("GIT_AI_DAEMON_MAX_UPTIME_SECS", "86400"),
        ],
    );
    let mut stream = open_local_socket_stream_with_timeout(
        &daemon_trace_socket_path(&repo),
        DAEMON_TEST_PROBE_TIMEOUT,
    )
    .expect("connect trace socket");
    if identify_root {
        write_trace_frames_to_stream(
            &mut stream,
            &[
                json!({"event": "start", "sid": "oversized-root", "argv": ["git", "commit"]}),
                json!({
                    "event": "def_repo", "sid": "oversized-root",
                    "worktree": repo_workdir_string(&repo),
                    "repo": repo.path().join(".git"),
                }),
            ],
        );
    }
    // Keep the connection open without a newline: rejection must not wait for
    // the sender to finish or allocate the unbounded remainder of the frame.
    let frame = format!("{{\"payload\":\"{}", "x".repeat(4 * 1024 * 1024));
    let _ = stream.write_all(frame.as_bytes());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if daemon.child.try_wait().expect("poll daemon").is_some() {
            let diagnostics = daemon.diagnostic_contents();
            assert!(
                diagnostics.contains("daemon trace frame exceeds 4194304 bytes"),
                "daemon stopped without the frame-limit diagnostic: {diagnostics}"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "daemon continued with incomplete trace evidence"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn daemon_rejects_oversized_trace_before_root_identification() {
    assert_oversized_trace_frame_stops_daemon(false);
}

#[test]
fn daemon_rejects_oversized_trace_after_root_identification() {
    assert_oversized_trace_frame_stops_daemon(true);
}
