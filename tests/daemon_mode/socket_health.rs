#[cfg(unix)]
use super::*;

#[test]
#[cfg(unix)]
fn trace_liveness_sentinel_closes_without_creating_work_and_preserves_attribution() {
    let repo = TestRepo::new_dedicated_daemon();
    let before = repo.daemon_total_completion_count();
    let mut stream =
        std::os::unix::net::UnixStream::connect(daemon_trace_socket_path(&repo)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(b"{\"event\":\"git_ai_health_ping\"}\n")
        .unwrap();
    assert_eq!(
        stream
            .read(&mut [0])
            .expect("sentinel should close its connection"),
        0
    );
    let response = send_control_request_with_timeout(
        &daemon_control_socket_path(&repo),
        &ControlRequest::Ping,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(response.ok);
    assert_eq!(repo.daemon_total_completion_count(), before);

    fs::write(repo.path().join("health.txt"), "known base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "health.txt"])
        .unwrap();
    repo.stage_all_and_commit("known base").unwrap();
    repo.filename("health.txt")
        .assert_committed_lines(lines!["known base".human()]);
    repo.git_ai(&["checkpoint", "human", "health.txt"]).unwrap();
    fs::write(repo.path().join("health.txt"), "known base\nAI addition\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "health.txt"])
        .unwrap();
    repo.stage_all_and_commit("AI addition after sentinel")
        .unwrap();
    repo.filename("health.txt")
        .assert_committed_lines(lines!["known base".human(), "AI addition".ai()]);
}
