use super::*;

fn checkpoint_fixture(repo: &TestRepo, checkpoint_status: &str) {
    let entries = [
        serde_json::json!({"kind": "command", "sync_tracked": true, "status": "ok"}),
        serde_json::json!({"kind": "checkpoint", "sync_tracked": false, "status": "ok"}),
        serde_json::json!({"kind": "command", "sync_tracked": false, "status": "ok"}),
        serde_json::json!({"kind": "checkpoint", "sync_tracked": true,
            "status": checkpoint_status, "error": "checkpoint failed"}),
    ];
    let log = entries
        .iter()
        .map(|entry| format!("{entry}\n"))
        .collect::<String>();
    repo.write_daemon_completion_log_fixture(&repo.daemon_family_key(), &log);
}

#[test]
fn next_checkpoint_wait_counts_only_tracked_checkpoints() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    assert_eq!(repo.daemon_checkpoint_completion_count(), 0);
    checkpoint_fixture(&repo, "ok");

    assert_eq!(repo.daemon_checkpoint_completion_count(), 1);
    assert_eq!(repo.wait_for_next_daemon_checkpoint_completion(0), 1);

    let baseline = repo.daemon_checkpoint_completion_count();
    let path = repo.daemon_completion_log_path_for_family(&repo.daemon_family_key());
    let mut log = fs::read_to_string(&path).unwrap();
    log.push_str("{\"kind\":\"checkpoint\",\"sync_tracked\":true,\"status\":\"ok\"}\n");
    repo.write_daemon_completion_log_fixture(&repo.daemon_family_key(), &log);
    assert_eq!(repo.daemon_checkpoint_completion_count(), 2);
    assert_eq!(repo.wait_for_next_daemon_checkpoint_completion(baseline), 2);
}

#[test]
fn next_checkpoint_wait_ignores_unrelated_command_errors() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    checkpoint_fixture(&repo, "ok");
    let path = repo.daemon_completion_log_path_for_family(&repo.daemon_family_key());
    let log = fs::read_to_string(&path)
        .unwrap()
        .replacen("\"ok\"", "\"error\"", 1);
    repo.write_daemon_completion_log_fixture(&repo.daemon_family_key(), &log);

    assert_eq!(repo.wait_for_next_daemon_checkpoint_completion(0), 1);
}

#[test]
fn next_checkpoint_wait_reports_checkpoint_errors() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    checkpoint_fixture(&repo, "error");

    let panic = std::panic::catch_unwind(|| repo.wait_for_next_daemon_checkpoint_completion(0))
        .expect_err("a failed checkpoint must not satisfy the wait");
    let message = panic.downcast_ref::<String>().expect("panic message");
    assert!(message.contains("daemon checkpoint completion reported an error"));
    assert!(message.contains("checkpoint failed"));
}
