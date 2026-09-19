#![cfg(not(windows))]
use super::*;

fn ingest_progress(repo: &TestRepo) -> u64 {
    let response = send_control_request_with_timeout(
        &daemon_control_socket_path(repo),
        &ControlRequest::StatusDaemon,
        Duration::from_secs(2),
    )
    .unwrap();
    response.data.unwrap()["trace_ingest_seq_processed"]
        .as_u64()
        .unwrap()
}

fn wait_for_ingest(repo: &TestRepo, target: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while ingest_progress(repo) < target {
        assert!(
            Instant::now() < deadline,
            "trace reader did not process fixture frames"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn completed_roots_detached_child_does_not_block_family_sync_or_attribution() {
    let repo = TestRepo::new_dedicated_daemon();
    let mut file = repo.filename("feature.txt");
    file.set_contents(lines!["first AI line".ai()]);
    repo.stage_all_and_commit("Initial feature").unwrap();
    file.assert_committed_lines(lines!["first AI line".ai()]);
    let socket = daemon_trace_socket_path(&repo);
    let worktree = repo_workdir_string(&repo);
    let git_dir = repo.path().join(".git").to_string_lossy().into_owned();
    let frames = |sid: &str, time| {
        TraceCommandFrames::new(
            sid,
            &["git", "commit", "--dry-run"],
            &worktree,
            &git_dir,
            time,
        )
        .into_frames()
    };
    let before = ingest_progress(&repo);
    send_trace_frames(&socket, &frames("finished-parent", 1000));
    wait_for_ingest(&repo, before + 4);
    repo.sync_daemon();

    let before = ingest_progress(&repo);
    let mut child =
        open_local_socket_stream_with_timeout(&socket, DAEMON_TEST_PROBE_TIMEOUT).unwrap();
    write_trace_frames_to_stream(
        &mut child,
        &[json!({"event":"version", "sid":"finished-parent/maintenance", "time_ns":2000})],
    );
    // A complete marker on the same stream proves the preceding child frame
    // reached the reader without relying on a sleep or closing the child.
    write_trace_frames_to_stream(&mut child, &frames("reader-marker", 3000));
    wait_for_ingest(&repo, before + 4);
    let response = send_control_request_with_timeout(
        &daemon_control_socket_path(&repo),
        &ControlRequest::SyncFamily {
            repo_working_dir: worktree,
        },
        Duration::from_secs(2),
    );
    assert!(
        response.as_ref().is_ok_and(|response| response.ok),
        "completed child fenced sync: {response:?}"
    );

    file.set_contents(lines!["first AI line".ai(), "second AI line".ai()]);
    repo.stage_all_and_commit("Feature after detached maintenance")
        .unwrap();
    file.assert_committed_lines(lines!["first AI line".ai(), "second AI line".ai()]);
    drop(child);
}
