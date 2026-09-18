#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;

fn checkpointed_paths() -> TestRepo {
    checkpointed_paths_in(TestRepo::new())
}

fn checkpointed_paths_in(repo: TestRepo) -> TestRepo {
    fs::write(repo.path().join("selected.txt"), "base selected\n").unwrap();
    fs::write(repo.path().join("survivor.txt"), "base survivor\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("selected.txt")
        .assert_committed_lines(lines!["base selected".unattributed_human()]);
    repo.filename("survivor.txt")
        .assert_committed_lines(lines!["base survivor".unattributed_human()]);
    repo.git_ai(&["checkpoint", "human", "selected.txt", "survivor.txt"])
        .unwrap();
    fs::write(repo.path().join("selected.txt"), "selected AI\n").unwrap();
    fs::write(repo.path().join("survivor.txt"), "surviving AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "selected.txt", "survivor.txt"])
        .unwrap();
    repo
}

fn finish_and_assert(repo: &TestRepo, selected_is_ai: bool) {
    repo.stage_all_and_commit("after workspace operation")
        .unwrap();
    let expected = if selected_is_ai {
        "selected AI".ai()
    } else {
        "selected AI".unattributed_human()
    };
    repo.filename("selected.txt")
        .assert_committed_lines(vec![expected]);
    repo.filename("survivor.txt")
        .assert_committed_lines(lines!["surviving AI".ai()]);
}

fn run_mutation(repo: &TestRepo, args: &[&str]) {
    let before = repo.daemon_total_completion_count();
    repo.git_without_test_sync_for_test(args, &[]).unwrap();
    repo.wait_for_daemon_total_completion_count(before, before + 1);
}

#[test]
fn restore_staged_retains_checkpoint_for_surviving_worktree_text() {
    let repo = checkpointed_paths();
    repo.git(&["add", "selected.txt"]).unwrap();
    run_mutation(&repo, &["restore", "--staged", "--", "selected.txt"]);
    assert_eq!(
        fs::read_to_string(repo.path().join("selected.txt")).unwrap(),
        "selected AI\n"
    );
    finish_and_assert(&repo, true);
}

#[test]
fn restore_worktree_from_ai_staged_index_keeps_ai_evidence() {
    let repo = checkpointed_paths();
    repo.git(&["add", "selected.txt"]).unwrap();
    run_mutation(&repo, &["restore", "--worktree", "--", "selected.txt"]);
    assert_eq!(
        fs::read_to_string(repo.path().join("selected.txt")).unwrap(),
        "selected AI\n"
    );
    finish_and_assert(&repo, true);
}

#[test]
fn rm_root_literal_does_not_reattribute_recreated_discarded_text() {
    let repo = checkpointed_paths();
    run_mutation(&repo, &["rm", "-f", "--", ":(top,literal)selected.txt"]);
    assert!(!repo.path().join("selected.txt").exists());
    fs::write(repo.path().join("selected.txt"), "selected AI\n").unwrap();
    finish_and_assert(&repo, false);
}

#[test]
fn rm_cached_preserves_checkpoint_for_surviving_worktree_text() {
    let repo = checkpointed_paths();
    repo.git(&["add", "selected.txt"]).unwrap();
    run_mutation(
        &repo,
        &["rm", "--cached", "--", ":(top,literal)selected.txt"],
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("selected.txt")).unwrap(),
        "selected AI\n"
    );
    finish_and_assert(&repo, true);
}

#[test]
fn rm_dry_run_preserves_checkpoint_for_surviving_worktree_text() {
    let repo = checkpointed_paths();
    run_mutation(
        &repo,
        &["rm", "-n", "-f", "--", ":(top,literal)selected.txt"],
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("selected.txt")).unwrap(),
        "selected AI\n"
    );
    finish_and_assert(&repo, true);
}

#[test]
fn rm_root_literal_from_subdirectory_discards_only_the_named_root_file() {
    let repo = checkpointed_paths();
    fs::create_dir(repo.path().join("sub")).unwrap();
    run_mutation(
        &repo,
        &["-C", "sub", "rm", "-f", "--", ":(top,literal)selected.txt"],
    );
    fs::write(repo.path().join("selected.txt"), "selected AI\n").unwrap();
    finish_and_assert(&repo, false);
}

#[test]
fn rm_root_literal_in_linked_worktree_discards_old_evidence() {
    let repo = checkpointed_paths_in(TestRepo::new_worktree());
    run_mutation(&repo, &["rm", "-f", "--", ":(top,literal)selected.txt"]);
    fs::write(repo.path().join("selected.txt"), "selected AI\n").unwrap();
    finish_and_assert(&repo, false);
}

#[test]
fn failed_rm_root_literal_preserves_checkpoint() {
    let repo = checkpointed_paths();
    let before = repo.daemon_total_completion_count();
    assert!(
        repo.git_without_test_sync_for_test(&["rm", "--", ":(top,literal)selected.txt"], &[])
            .is_err()
    );
    repo.wait_for_daemon_total_completion_count(before, before + 1);
    finish_and_assert(&repo, true);
}

#[test]
fn rm_root_literal_preserves_a_later_ai_checkpoint() {
    let repo = checkpointed_paths();
    run_mutation(&repo, &["rm", "-f", "--", ":(top,literal)selected.txt"]);
    repo.git_ai(&["checkpoint", "human", "selected.txt"])
        .unwrap();
    fs::write(repo.path().join("selected.txt"), "selected AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "selected.txt"])
        .unwrap();
    finish_and_assert(&repo, true);
}

#[test]
fn delayed_rm_root_literal_uses_ordered_head_after_git_commits_again() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("rm-gate");
    fs::write(&gate, "").unwrap();
    let spec = format!("rm={}", gate.display());
    let repo = checkpointed_paths_in(TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND",
        &spec,
    )]));
    let before = repo.daemon_total_completion_count();
    repo.git_without_test_sync_for_test(&["rm", "-f", "--", ":(top,literal)selected.txt"], &[])
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "removal did not reach side-effect gate"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::write(repo.path().join("selected.txt"), "selected AI\n").unwrap();
    repo.git_without_test_sync_for_test(&["add", "."], &[])
        .unwrap();
    repo.git_without_test_sync_for_test(
        &["commit", "-m", "commit while removal processing is delayed"],
        &[],
    )
    .unwrap();
    fs::remove_file(&gate).unwrap();
    repo.wait_for_daemon_total_completion_count(before, before + 2);
    repo.filename("selected.txt")
        .assert_committed_lines(lines!["selected AI".unattributed_human()]);
    repo.filename("survivor.txt")
        .assert_committed_lines(lines!["surviving AI".ai()]);
}
