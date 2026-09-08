use super::{
    ExpectedLineExt, TestRepo, assert_ci_rewrite_succeeded, authorship_files, direct_test_repo,
    run_ci_local_merge, setup_main, squash_feature_with_raw_git,
};

#[test]
fn test_ci_rebase_merge_commit_order_pairing() {
    let repo = TestRepo::new();
    let base_sha = setup_main(&repo);

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["ai content in file_a".ai()]);
    let feature_sha1 = repo.stage_all_and_commit("add file_a").unwrap().commit_sha;

    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["ai content in file_b".ai()]);
    let feature_sha2 = repo.stage_all_and_commit("add file_b").unwrap().commit_sha;

    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_only = repo.filename("main_only.txt");
    main_only.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "advance main"]).unwrap();

    repo.git_og(&["checkout", "feature"]).unwrap();
    repo.git_og(&["rebase", "main"]).unwrap();
    let new_sha2 = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let new_sha1 = repo
        .git_og(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    assert_ne!(new_sha1, feature_sha1);
    assert_ne!(new_sha2, feature_sha2);

    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--ff-only", "feature"]).unwrap();

    let output = run_ci_local_merge(&repo, &new_sha2, &feature_sha2, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    let files1 = authorship_files(&repo, &new_sha1);
    let files2 = authorship_files(&repo, &new_sha2);

    assert!(
        files1.iter().any(|file| file.contains("file_a")),
        "rebased commit 1 should reference file_a.txt, got: {files1:?}"
    );
    assert!(
        !files1.iter().any(|file| file.contains("file_b")),
        "rebased commit 1 should not reference file_b.txt, got: {files1:?}"
    );
    assert!(
        files2.iter().any(|file| file.contains("file_b")),
        "rebased commit 2 should reference file_b.txt, got: {files2:?}"
    );
    assert!(
        !files2.iter().any(|file| file.contains("file_a")),
        "rebased commit 2 should not reference file_a.txt, got: {files2:?}"
    );
}

/// Multi-commit feature (AI + AI + human) squashed into one merge commit: the
/// squashed commit splits attribution across authors. Originally exercised the
/// removed engine directly.
#[test]
fn test_ci_rebase_merge_multiple_commits() {
    let repo = direct_test_repo();
    let mut file = repo.filename("app.js");

    file.set_contents(crate::lines!["// App v1", ""]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(
        1,
        crate::lines!["// AI function 1".ai(), "function ai1() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 1").unwrap();
    file.insert_at(
        3,
        crate::lines!["// AI function 2".ai(), "function ai2() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 2").unwrap();
    file.insert_at(
        5,
        crate::lines!["// Human function", "function human() { }"],
    );
    let head_sha = repo
        .stage_all_and_commit("Add human function")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature branch (squashed)");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    file.assert_lines_and_blame(crate::lines![
        "// App v1".human(),
        "// AI function 1".ai(),
        "function ai1() { }".ai(),
        "// AI function 2".ai(),
        "function ai2() { }".ai(),
        "// Human function".human(),
        "function human() { }".human()
    ]);
}

/// Standard-human variant of `test_ci_rebase_merge_multiple_commits`.
#[test]
fn test_ci_rebase_merge_multiple_commits_standard_human() {
    let repo = direct_test_repo();
    let mut file = repo.filename("app.js");

    file.set_contents(crate::lines![
        "// App v1".unattributed_human(),
        "".unattributed_human()
    ]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(
        1,
        crate::lines!["// AI function 1".ai(), "function ai1() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 1").unwrap();
    file.insert_at(
        3,
        crate::lines!["// AI function 2".ai(), "function ai2() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 2").unwrap();
    file.insert_at(
        5,
        crate::lines![
            "// Human function".unattributed_human(),
            "function human() { }".unattributed_human()
        ],
    );
    let head_sha = repo
        .stage_all_and_commit("Add human function")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature branch (squashed)");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    file.assert_lines_and_blame(crate::lines![
        "// App v1".unattributed_human(),
        "// AI function 1".ai(),
        "function ai1() { }".ai(),
        "// AI function 2".ai(),
        "function ai2() { }".ai(),
        "// Human function".unattributed_human(),
        "function human() { }".unattributed_human()
    ]);
}

crate::reuse_tests_in_worktree!(
    test_ci_rebase_merge_commit_order_pairing,
    test_ci_rebase_merge_multiple_commits,
    test_ci_rebase_merge_multiple_commits_standard_human,
);
