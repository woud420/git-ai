use super::{
    ExpectedLineExt, TestRepo, assert_no_ai_authorship_for_commit, cold_repo, raw_commit_file,
    raw_git, raw_git_result, raw_head, read_file, run_traced_git, run_traced_git_without_sync,
    start_cold_daemon, traced_ai_commit_file,
};

#[test]
fn test_cold_repo_mid_cherry_pick_continue_preserves_ai_conflict_resolution() {
    let mut repo = TestRepo::new_dedicated_daemon();
    traced_ai_commit_file(&repo, "conflict.txt", "base\n", "ai base");
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "source"]).unwrap();
    let source_commit = traced_ai_commit_file(&repo, "conflict.txt", "source\n", "ai source");

    repo.git(&["checkout", "main"]).unwrap();
    traced_ai_commit_file(&repo, "conflict.txt", "main\n", "ai main");

    let cherry_pick = raw_git_result(&repo, &["cherry-pick", &source_commit]);
    assert!(
        cherry_pick.is_err(),
        "raw trace-disabled cherry-pick should stop for conflict, got: {:?}",
        cherry_pick
    );

    repo.restart_dedicated_daemon_for_test();
    repo.git_ai(&["checkpoint", "human", "conflict.txt"])
        .unwrap();
    repo.write_file("conflict.txt", "resolved by ai\n");
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.txt"])
        .unwrap();
    repo.git(&["add", "conflict.txt"]).unwrap();
    repo.git_with_env(
        &["cherry-pick", "--continue"],
        &[("GIT_EDITOR", "true")],
        None,
    )
    .unwrap();
    repo.sync_daemon_force();

    let picked = raw_head(&repo);
    assert_ne!(picked, source_commit);
    let mut file = repo.filename("conflict.txt");
    file.assert_committed_lines(crate::lines!["resolved by ai".ai()]);
}

#[test]
fn test_cold_repo_mid_merge_commit_preserves_ai_conflict_resolution() {
    let mut repo = TestRepo::new_dedicated_daemon();
    traced_ai_commit_file(&repo, "conflict.txt", "base\n", "ai base");
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    traced_ai_commit_file(&repo, "conflict.txt", "feature\n", "ai feature");

    repo.git(&["checkout", "main"]).unwrap();
    traced_ai_commit_file(&repo, "conflict.txt", "main\n", "ai main");

    let merge = raw_git_result(&repo, &["merge", "feature"]);
    assert!(
        merge.is_err(),
        "raw trace-disabled merge should stop for conflict, got: {:?}",
        merge
    );

    repo.restart_dedicated_daemon_for_test();
    repo.git_ai(&["checkpoint", "human", "conflict.txt"])
        .unwrap();
    repo.write_file("conflict.txt", "resolved by ai\n");
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.txt"])
        .unwrap();
    repo.git(&["add", "conflict.txt"]).unwrap();
    repo.git(&["commit", "-m", "merge resolved"]).unwrap();
    repo.sync_daemon_force();

    let mut file = repo.filename("conflict.txt");
    file.assert_committed_lines(crate::lines!["resolved by ai".ai()]);
}

#[test]
fn test_cold_repo_first_traced_squash_merge_is_processed() {
    let mut repo = cold_repo();
    raw_commit_file(&repo, "base.txt", "base\n", "raw base");
    raw_git(&repo, &["branch", "-M", "main"]);
    raw_git(&repo, &["checkout", "-b", "feature"]);
    raw_commit_file(
        &repo,
        "feature.txt",
        "feature squash\n",
        "raw squash source",
    );
    raw_git(&repo, &["checkout", "main"]);
    raw_commit_file(&repo, "main.txt", "main\n", "raw main advance");

    start_cold_daemon(&mut repo);
    run_traced_git_without_sync(&repo, &["merge", "--squash", "feature"]);
    let staged = raw_git(&repo, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.lines().any(|line| line == "feature.txt"),
        "squash merge should stage feature.txt, got: {}",
        staged
    );
    run_traced_git(&repo, &["commit", "-m", "first traced squash commit"]);

    let squash_commit = raw_head(&repo);
    assert_eq!(read_file(&repo, "feature.txt"), "feature squash\n");
    assert_no_ai_authorship_for_commit(&repo, &squash_commit);
}

#[test]
fn test_cold_daemon_first_traced_squash_merge_preserves_source_ai_authorship() {
    let mut repo = TestRepo::new_dedicated_daemon();
    let mut file = repo.filename("document.txt");

    file.set_contents(crate::lines![
        "section 1".unattributed_human(),
        "section 2".unattributed_human(),
        "section 3".unattributed_human()
    ]);
    repo.stage_all_and_commit("initial document").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(3, crate::lines!["// AI feature addition at end".ai()]);
    repo.stage_all_and_commit("AI adds feature").unwrap();

    repo.git(&["checkout", "main"]).unwrap();
    let mut file = repo.filename("document.txt");
    file.insert_at(
        0,
        crate::lines!["// Master update at top".unattributed_human()],
    );
    repo.stage_all_and_commit("out-of-band main update")
        .unwrap();

    repo.restart_dedicated_daemon_for_test();
    repo.git(&["merge", "--squash", "feature"]).unwrap();
    repo.stage_all_and_commit("squashed feature").unwrap();

    let mut file = repo.filename("document.txt");
    file.assert_committed_lines(crate::lines![
        "// Master update at top".human(),
        "section 1".human(),
        "section 2".human(),
        "section 3".ai(),
        "// AI feature addition at end".ai()
    ]);
}

#[test]
fn test_cold_repo_first_traced_merge_is_processed() {
    let mut repo = cold_repo();
    raw_commit_file(&repo, "base.txt", "base\n", "raw base");
    raw_git(&repo, &["branch", "-M", "main"]);
    raw_git(&repo, &["checkout", "-b", "feature"]);
    raw_commit_file(&repo, "feature.txt", "feature\n", "raw feature");
    raw_git(&repo, &["checkout", "main"]);
    raw_commit_file(&repo, "main.txt", "main\n", "raw main advance");

    start_cold_daemon(&mut repo);
    run_traced_git(
        &repo,
        &["merge", "--no-ff", "feature", "-m", "first traced merge"],
    );

    let merge_commit = raw_head(&repo);
    let parents = raw_git(&repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "merge commit should have two parents, got: {}",
        parents
    );
    assert_eq!(read_file(&repo, "feature.txt"), "feature\n");
    assert_no_ai_authorship_for_commit(&repo, &merge_commit);
}

#[test]
fn test_cold_repo_traced_stash_after_raw_stash_history_preserves_current_ai_attribution() {
    let mut repo = cold_repo();
    raw_commit_file(&repo, "stash.txt", "base\n", "raw base");
    repo.write_file("stash.txt", "base\nold raw stash\n");
    raw_git(&repo, &["stash", "push"]);
    assert_eq!(read_file(&repo, "stash.txt"), "base\n");

    start_cold_daemon(&mut repo);
    repo.write_file("stash.txt", "base\ncurrent ai stash\n");
    repo.git_ai(&["checkpoint", "mock_ai", "stash.txt"])
        .unwrap_or_else(|error| panic!("mock_ai checkpoint failed: {}", error));
    run_traced_git_without_sync(&repo, &["stash", "push"]);
    assert_eq!(read_file(&repo, "stash.txt"), "base\n");

    run_traced_git_without_sync(&repo, &["stash", "pop"]);
    repo.stage_all_and_commit("apply current ai stash")
        .expect("apply current ai stash commit should succeed");

    let mut file = repo.filename("stash.txt");
    file.assert_lines_and_blame(crate::lines!["base".human(), "current ai stash".ai(),]);
}

crate::reuse_tests_in_worktree!(
    test_cold_repo_mid_cherry_pick_continue_preserves_ai_conflict_resolution,
    test_cold_repo_mid_merge_commit_preserves_ai_conflict_resolution,
    test_cold_repo_first_traced_squash_merge_is_processed,
    test_cold_daemon_first_traced_squash_merge_preserves_source_ai_authorship,
    test_cold_repo_first_traced_merge_is_processed,
    test_cold_repo_traced_stash_after_raw_stash_history_preserves_current_ai_attribution,
);
