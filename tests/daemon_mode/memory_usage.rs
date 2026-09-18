use super::*;

#[test]
#[serial]
fn daemon_memory_watchdog_ignores_a_released_peak() {
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.patch_git_ai_config(|patch| patch.daemon_memory_limit_mb = Some(1024));
    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_TEST_DAEMON_PEAK_RSS_MB_SEQUENCE", "1024"),
            ("GIT_AI_TEST_DAEMON_CURRENT_RSS_MB_SEQUENCE", "100"),
            ("GIT_AI_TEST_DAEMON_MEMORY_POLL_MS", "100"),
        ],
    );

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        assert!(
            daemon
                .child
                .try_wait()
                .expect("check daemon process")
                .is_none(),
            "released memory must not trigger an abort: {}",
            daemon.diagnostic_contents()
        );
        thread::sleep(Duration::from_millis(20));
    }
    let response = send_control_request(&daemon.control_socket_path, &ControlRequest::Ping)
        .expect("daemon should remain responsive after multiple memory samples");
    assert!(response.ok, "{response:?}");
}
