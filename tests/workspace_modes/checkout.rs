use super::*;

#[test]
fn checkout_force_create_reset_carries_pending_ai() {
    let repo = repo_with_pending_ai();
    repo.git(&["branch", "target"]).unwrap();
    repo.git(&["checkout", "-B", "target"]).unwrap();
    commit_and_assert_pending(&repo, "checkout -B");
}

#[test]
fn checkout_detach_carries_pending_ai() {
    let repo = repo_with_pending_ai();
    repo.git(&["checkout", "--detach", "HEAD"]).unwrap();
    commit_and_assert_pending(&repo, "detached commit");
}

#[test]
fn checkout_orphan_carries_pending_ai_to_root_commit() {
    let repo = repo_with_pending_ai();
    run_workspace_command(&repo, &["checkout", "--orphan", "orphan-checkout"]);
    commit_and_assert_pending(&repo, "orphan checkout root");
}

#[test]
fn failed_checkout_track_does_not_corrupt_pending_ai() {
    let repo = repo_with_pending_ai();
    assert!(
        repo.git(&["checkout", "--track", "missing/branch"])
            .is_err()
    );
    commit_and_assert_pending(&repo, "after failed checkout track");
}

#[test]
fn switch_create_carries_pending_ai() {
    let repo = repo_with_pending_ai();
    repo.git(&["switch", "-c", "switch-created"]).unwrap();
    commit_and_assert_pending(&repo, "switch -c");
}

#[test]
fn switch_force_create_reset_carries_pending_ai() {
    let repo = repo_with_pending_ai();
    repo.git(&["branch", "switch-target"]).unwrap();
    repo.git(&["switch", "-C", "switch-target"]).unwrap();
    commit_and_assert_pending(&repo, "switch -C");
}

#[test]
fn switch_detach_carries_pending_ai() {
    let repo = repo_with_pending_ai();
    repo.git(&["switch", "--detach", "HEAD"]).unwrap();
    commit_and_assert_pending(&repo, "switch detached commit");
}

#[test]
fn failed_switch_track_does_not_corrupt_pending_ai() {
    let repo = repo_with_pending_ai();
    assert!(repo.git(&["switch", "--track", "missing/branch"]).is_err());
    commit_and_assert_pending(&repo, "after failed switch track");
}

fn repo_with_remote_tracking_branch_and_pending_ai() -> (TestRepo, String) {
    let repo = repo_with_pending_ai();
    let remote_ref = "refs/remotes/origin/remote-feature";
    let repo_path = repo.path().to_string_lossy().to_string();
    repo.git_og(&["remote", "add", "origin", &repo_path])
        .unwrap();
    repo.git_og(&["update-ref", remote_ref, "HEAD"]).unwrap();
    (repo, "origin/remote-feature".to_string())
}

#[test]
fn checkout_track_success_carries_pending_ai() {
    let (repo, remote) = repo_with_remote_tracking_branch_and_pending_ai();
    repo.git(&["checkout", "--track", &remote]).unwrap();
    assert_eq!(repo.current_branch(), "remote-feature");
    commit_and_assert_pending(&repo, "checkout tracked branch");
}

#[test]
fn switch_track_success_carries_pending_ai() {
    let (repo, remote) = repo_with_remote_tracking_branch_and_pending_ai();
    repo.git(&["switch", "--track", &remote]).unwrap();
    assert_eq!(repo.current_branch(), "remote-feature");
    commit_and_assert_pending(&repo, "switch tracked branch");
}

#[test]
fn checkout_orphan_then_return_preserves_pending_ai() {
    let repo = repo_with_pending_ai();
    run_workspace_command(&repo, &["checkout", "--orphan", "temporary-orphan"]);
    run_workspace_command(&repo, &["checkout", default_branchname()]);
    commit_and_assert_pending(&repo, "returned from orphan");
}

#[test]
fn checkout_orphan_preserves_a_later_checkpoint() {
    let repo = repo_with_pending_ai();
    run_workspace_command(&repo, &["checkout", "--orphan", "later-checkpoint"]);
    write_ai(&repo, "later.txt", "later AI\n");
    commit_and_assert_pending(&repo, "orphan with later checkpoint");
    repo.filename("later.txt")
        .assert_committed_lines(lines!["later AI".ai()]);
}

#[test]
fn switch_orphan_preserves_a_new_staged_file() {
    let repo = repo_with_pending_ai();
    repo.git(&["add", "pending.txt"]).unwrap();
    run_workspace_command(&repo, &["switch", "--orphan", "preserve-staged"]);
    assert_eq!(
        fs::read_to_string(repo.path().join("pending.txt")).unwrap(),
        "pending AI\n"
    );
    repo.stage_all_and_commit("surviving staged addition")
        .unwrap();
    assert!(repo.git(&["show", "HEAD:seed.txt"]).is_err());
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

#[test]
fn switch_orphan_preserves_an_untracked_ai_file() {
    let repo = repo_with_pending_ai();
    run_workspace_command(&repo, &["switch", "--orphan", "preserve-untracked"]);
    repo.stage_all_and_commit("surviving untracked addition")
        .unwrap();
    assert!(repo.git(&["show", "HEAD:seed.txt"]).is_err());
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

#[test]
fn switch_orphan_does_not_revive_discarded_tracked_checkpoint_text() {
    let repo = repo_with_pending_ai();
    write_ai(&repo, "seed.txt", "discarded AI\n");
    repo.git_og(&["restore", "--worktree", "--", "seed.txt"])
        .unwrap();
    run_workspace_command(&repo, &["switch", "--orphan", "discard-tracked"]);
    assert!(!repo.path().join("seed.txt").exists());
    fs::write(repo.path().join("seed.txt"), "discarded AI\n").unwrap();
    repo.stage_all_and_commit("recreated after tracked path removal")
        .unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["discarded AI".unattributed_human()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

#[test]
fn checkout_orphan_preserves_a_tracked_ai_edit() {
    let repo = repo_with_pending_ai();
    write_ai(&repo, "seed.txt", "tracked AI\n");
    run_workspace_command(&repo, &["checkout", "--orphan", "tracked-edit"]);
    repo.stage_all_and_commit("tracked edit on orphan branch")
        .unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["tracked AI".ai()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

#[test]
fn delayed_checkout_orphan_migrates_before_the_new_root_commit() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("checkout-gate");
    fs::write(&gate, "").unwrap();
    let spec = format!("checkout={}", gate.display());
    let repo = repo_with_pending_ai_in(TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND",
        &spec,
    )]));
    let before = repo.daemon_total_completion_count();
    repo.git_without_test_sync_for_test(&["checkout", "--orphan", "delayed-orphan"], &[])
        .unwrap();
    wait_for_side_effect_gate(&gate);
    repo.git_without_test_sync_for_test(&["add", "."], &[])
        .unwrap();
    repo.git_without_test_sync_for_test(
        &["commit", "-m", "root before checkout processing finishes"],
        &[],
    )
    .unwrap();
    fs::remove_file(&gate).unwrap();
    repo.wait_for_daemon_total_completion_count(before, before + 2);
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["seed".unattributed_human()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

#[test]
fn switch_orphan_missing_old_tree_does_not_revive_unverified_checkpoints() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("switch-gate");
    fs::write(&gate, "").unwrap();
    let spec = format!("switch={}", gate.display());
    let repo = repo_with_pending_ai_in(TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND",
        &spec,
    )]));
    let old_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    write_ai(&repo, "seed.txt", "discarded AI\n");
    repo.git_og(&["restore", "--worktree", "--", "seed.txt"])
        .unwrap();
    let before = repo.daemon_total_completion_count();
    repo.git_without_test_sync_for_test(&["switch", "--orphan", "missing-tree"], &[])
        .unwrap();
    wait_for_side_effect_gate(&gate);
    repo.git_og(&["branch", "-D", default_branchname()])
        .unwrap();
    repo.git_og(&["reflog", "expire", "--expire=now", "--all"])
        .unwrap();
    repo.git_og(&["gc", "--prune=now"]).unwrap();
    assert!(repo.git_og(&["cat-file", "-e", &old_head]).is_err());
    fs::remove_file(&gate).unwrap();
    repo.wait_for_daemon_total_completion_count(before, before + 1);
    fs::write(repo.path().join("seed.txt"), "discarded AI\n").unwrap();
    repo.stage_all_and_commit("root without old tree evidence")
        .unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["discarded AI".unattributed_human()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".unattributed_human()]);
}
