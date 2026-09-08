use super::{
    ExpectedLineExt, TestRepo, assert_no_ai_authorship_for_commit, cold_repo, raw_commit_file,
    raw_git, raw_git_result, raw_head, read_file,
    run_cold_repo_first_traced_pull_rebase_preserves_rebased_ai_authorship, run_traced_git,
    start_cold_daemon, traced_ai_commit_file,
};

#[test]
fn test_cold_repo_first_traced_pull_rebase_preserves_rebased_ai_authorship() {
    run_cold_repo_first_traced_pull_rebase_preserves_rebased_ai_authorship();
}

#[test]
#[ignore = "stress test for nondeterministic cold pull-rebase reflog timing"]
fn stress_cold_repo_first_traced_pull_rebase_preserves_rebased_ai_authorship() {
    std::thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    for _ in 0..3 {
                        run_cold_repo_first_traced_pull_rebase_preserves_rebased_ai_authorship();
                    }
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle
                .join()
                .expect("cold pull-rebase stress worker panicked");
        }
    });
}

#[test]
fn test_cold_repo_first_traced_rebase_is_processed() {
    let mut repo = cold_repo();
    raw_commit_file(&repo, "base.txt", "base\n", "raw base");
    raw_git(&repo, &["branch", "-M", "main"]);
    raw_git(&repo, &["checkout", "-b", "feature"]);
    let old_feature = raw_commit_file(&repo, "feature.txt", "feature\n", "raw feature");
    raw_git(&repo, &["checkout", "main"]);
    let main_tip = raw_commit_file(&repo, "main.txt", "main\n", "raw main advance");
    raw_git(&repo, &["checkout", "feature"]);

    start_cold_daemon(&mut repo);
    run_traced_git(&repo, &["rebase", "main"]);

    let rebased = raw_head(&repo);
    assert_ne!(rebased, old_feature);
    raw_git(&repo, &["merge-base", "--is-ancestor", &main_tip, "HEAD"]);
    assert_eq!(read_file(&repo, "feature.txt"), "feature\n");
    assert_no_ai_authorship_for_commit(&repo, &rebased);
}

#[test]
fn test_cold_repo_first_traced_conflict_rebase_ignores_stale_rebase_reflog_history() {
    let mut repo = TestRepo::new_dedicated_daemon();
    traced_ai_commit_file(&repo, "base.txt", "base\n", "ai base");
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "old-topic"]).unwrap();
    traced_ai_commit_file(&repo, "old.txt", "old topic\n", "ai old topic");
    repo.git(&["checkout", "main"]).unwrap();
    traced_ai_commit_file(&repo, "main.txt", "main advance\n", "ai main advance");
    repo.git(&["checkout", "old-topic"]).unwrap();
    repo.git(&["rebase", "main"]).unwrap();
    repo.git(&["checkout", "main"]).unwrap();

    traced_ai_commit_file(
        &repo,
        "jokes-animals.csv",
        "setup,punchline\nWhat do you call a bear with no teeth?,A gummy bear\n",
        "ai initial jokes",
    );
    repo.git(&["checkout", "-b", "scenario-3-multi-file-conflict"])
        .unwrap();
    let feature_tip = traced_ai_commit_file(
        &repo,
        "jokes-animals.csv",
        "setup,punchline\nWhat do you call a bear with no teeth?,A gummy bear\nWhat do you call a sleeping bull?,A dozer\n",
        "ai bull joke",
    );
    repo.git(&["checkout", "main"]).unwrap();
    traced_ai_commit_file(
        &repo,
        "jokes-animals.csv",
        "setup,punchline\nWhat do you call a bear with no teeth?,A gummy bear\nWhat's a cat's favorite color?,Purr-ple\n",
        "ai cat joke",
    );

    repo.restart_dedicated_daemon_for_test();
    let rebase = repo.git(&["rebase", "main", "scenario-3-multi-file-conflict"]);
    assert!(
        rebase.is_err(),
        "rebase should stop for a conflict, got: {:?}",
        rebase
    );
    repo.write_file("jokes-animals.csv",
        "setup,punchline\nWhat do you call a bear with no teeth?,A gummy bear\nWhat's a cat's favorite color?,Purr-ple\nWhat do you call a sleeping bull?,A dozer\n",
    );
    repo.git(&["add", "jokes-animals.csv"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();
    repo.sync_daemon_force();

    let rebased = raw_head(&repo);
    assert_ne!(rebased, feature_tip);
    let mut file = repo.filename("jokes-animals.csv");
    file.assert_committed_lines(crate::lines![
        "setup,punchline".ai(),
        "What do you call a bear with no teeth?,A gummy bear".ai(),
        "What's a cat's favorite color?,Purr-ple".ai(),
        "What do you call a sleeping bull?,A dozer".ai(),
    ]);
}

#[test]
fn test_cold_repo_mid_rebase_continue_preserves_ai_conflict_resolution() {
    let mut repo = TestRepo::new_dedicated_daemon();
    traced_ai_commit_file(&repo, "conflict.txt", "base\n", "ai base");
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let feature_tip = traced_ai_commit_file(&repo, "conflict.txt", "feature\n", "ai feature");

    repo.git(&["checkout", "main"]).unwrap();
    traced_ai_commit_file(&repo, "conflict.txt", "main\n", "ai main");

    raw_git(&repo, &["checkout", "feature"]);
    let rebase = raw_git_result(&repo, &["rebase", "main"]);
    assert!(
        rebase.is_err(),
        "raw trace-disabled rebase should stop for conflict, got: {:?}",
        rebase
    );

    repo.restart_dedicated_daemon_for_test();
    repo.git_ai(&["checkpoint", "human", "conflict.txt"])
        .unwrap();
    repo.write_file("conflict.txt", "resolved by ai\n");
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.txt"])
        .unwrap();
    repo.git(&["add", "conflict.txt"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();
    repo.sync_daemon_force();

    let rebased = raw_head(&repo);
    assert_ne!(rebased, feature_tip);
    let mut file = repo.filename("conflict.txt");
    file.assert_committed_lines(crate::lines!["resolved by ai".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_cold_repo_first_traced_rebase_is_processed,
    test_cold_repo_mid_rebase_continue_preserves_ai_conflict_resolution,
);
