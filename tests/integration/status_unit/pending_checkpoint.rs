use super::*;
use crate::repos::test_repo::DaemonTestScope;

#[test]
fn pending_checkpoint_status_is_best_effort_when_the_daemon_is_unavailable() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    repo.git(&["add", "base.txt"]).unwrap();
    repo.git(&["commit", "-m", "base for unavailable-daemon status"])
        .unwrap();
    repo.filename("base.txt")
        .assert_committed_lines(crate::lines!["base".unattributed_human()]);
    let output = repo
        .git_ai_command_without_pre_sync_for_test(&["status", "--json"], &[])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["checkpoint_processing_pending"], false);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Checkpoint processing is pending"));
}
