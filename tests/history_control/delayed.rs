use super::*;
use std::time::{Duration, Instant};

fn continued_resolution_uses_captured_commit(operation: &str) {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("gate");
    let entered = gate.with_extension("entered");
    let spec = format!("{operation}={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    let repo = conflicting_repo_from(repo, operation);
    write_ai(&repo, "conflict.txt", "delayed resolution\n");
    repo.git(&["add", "conflict.txt"]).unwrap();
    fs::remove_file(&entered).unwrap();
    fs::write(&gate, "hold").unwrap();
    repo.git_without_test_sync_for_test(&[operation, "--continue"], &[("GIT_EDITOR", "true")])
        .unwrap();
    let resolved = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !entered.exists() {
        assert!(Instant::now() < deadline, "continuation did not reach gate");
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(repo.path().join("later.txt"), "later\n").unwrap();
    repo.git_og(&["add", "later.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "later untraced commit"])
        .unwrap();
    let later = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    assert_ne!(resolved, later);
    fs::remove_file(gate).unwrap();
    repo.sync_daemon_force();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["delayed resolution".ai()]);
    repo.filename("later.txt")
        .assert_committed_lines(lines!["later".unattributed_human()]);
    assert!(repo.read_authorship_note(later.trim()).is_none());
    repo.git_og(&["checkout", "--detach", resolved.trim()])
        .unwrap();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["delayed resolution".ai()]);
}

#[test]
fn delayed_merge_continue_attributes_its_commit_after_head_moves() {
    continued_resolution_uses_captured_commit("merge");
}

#[test]
fn delayed_revert_continue_attributes_its_commit_after_head_moves() {
    continued_resolution_uses_captured_commit("revert");
}
