use super::{ExpectedLineExt, TestRepo};

/// Test empty rebase (fast-forward)
#[test]
fn test_rebase_fast_forward() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Add commit on feature
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Rebase onto default branch (should be fast-forward, no changes - hooks handle authorship)
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify authorship is still correct after fast-forward rebase
    feature_file.assert_lines_and_blame(crate::lines!["// AI feature".ai()]);
}

/// Test `git rebase <upstream> <branch>` when invoked from another branch.
/// We should capture original_head from `<branch>`, not from the currently checked-out branch.
#[test]
fn test_rebase_with_explicit_branch_argument_preserves_authorship() {
    let repo = TestRepo::new();

    // Base commit
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    // Feature branch with AI-authored content
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai(), "fn feature() {}".ai()]);
    repo.stage_all_and_commit("add feature").unwrap();

    // Advance main branch
    repo.git(&["checkout", &main_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("main advances").unwrap();

    // Invoke rebase with explicit branch arg while currently on main.
    repo.git(&["rebase", &main_branch, "feature"]).unwrap();

    // HEAD should now be on feature after the rebase operation; verify AI blame survived.
    feature_file
        .assert_lines_and_blame(crate::lines!["// AI feature".ai(), "fn feature() {}".ai()]);

    // Verify the rebased commit carries an authorship note via git notes.
    let rebased_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert!(
        repo.read_authorship_note(&rebased_head).is_some(),
        "Rebased commit should have an authorship note"
    );
}

/// Test `git rebase --root --onto <base> <branch>` when invoked from another branch.
/// We should resolve original_head from `<branch>`, not from the currently checked-out branch.
#[test]
fn test_rebase_root_with_explicit_branch_argument_preserves_authorship() {
    let repo = TestRepo::new();

    // Base commit
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    // Feature branch with AI-authored content
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai(), "fn feature() {}".ai()]);
    let original_feature_head = repo.stage_all_and_commit("add feature").unwrap().commit_sha;

    // Advance main branch
    repo.git(&["checkout", &main_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("main advances").unwrap();

    // Invoke root rebase with explicit branch arg while currently on main.
    repo.git(&["rebase", "--root", "--onto", &main_branch, "feature"])
        .unwrap();

    let rebased_feature_head = repo.git(&["rev-parse", "HEAD"]).unwrap();
    assert_ne!(
        rebased_feature_head.trim(),
        original_feature_head,
        "Feature head should be rewritten by root rebase"
    );

    // HEAD should now be on feature after the rebase operation; verify AI blame survived.
    feature_file
        .assert_lines_and_blame(crate::lines!["// AI feature".ai(), "fn feature() {}".ai()]);

    // Verify the rebased commit carries an authorship note via git notes.
    assert!(
        repo.read_authorship_note(rebased_feature_head.trim())
            .is_some(),
        "Rebased commit should have an authorship note"
    );
}

/// Test dependent branch stack (patch-stack workflow)
#[test]
fn test_rebase_patch_stack() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create topic-1 branch
    repo.git(&["checkout", "-b", "topic-1"]).unwrap();
    let mut topic1_file = repo.filename("topic1.txt");
    topic1_file.set_contents(crate::lines!["// AI topic 1".ai()]);
    repo.stage_all_and_commit("Topic 1").unwrap();

    // Create topic-2 branch on top of topic-1
    repo.git(&["checkout", "-b", "topic-2"]).unwrap();
    let mut topic2_file = repo.filename("topic2.txt");
    topic2_file.set_contents(crate::lines!["// AI topic 2".ai()]);
    repo.stage_all_and_commit("Topic 2").unwrap();

    // Create topic-3 branch on top of topic-2
    repo.git(&["checkout", "-b", "topic-3"]).unwrap();
    let mut topic3_file = repo.filename("topic3.txt");
    topic3_file.set_contents(crate::lines!["// AI topic 3".ai()]);
    repo.stage_all_and_commit("Topic 3").unwrap();

    // Advance main
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("Main work").unwrap();

    // Rebase the stack: topic-1, then topic-2, then topic-3 (hooks will handle authorship)
    repo.git(&["checkout", "topic-1"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    repo.git(&["checkout", "topic-2"]).unwrap();
    repo.git(&["rebase", "topic-1"]).unwrap();

    repo.git(&["checkout", "topic-3"]).unwrap();
    repo.git(&["rebase", "topic-2"]).unwrap();

    // Verify all files have preserved AI authorship after rebasing the stack
    repo.git(&["checkout", "topic-1"]).unwrap();
    topic1_file.assert_lines_and_blame(crate::lines!["// AI topic 1".ai()]);

    repo.git(&["checkout", "topic-2"]).unwrap();
    topic1_file.assert_lines_and_blame(crate::lines!["// AI topic 1".ai()]);
    topic2_file.assert_lines_and_blame(crate::lines!["// AI topic 2".ai()]);

    repo.git(&["checkout", "topic-3"]).unwrap();
    topic1_file.assert_lines_and_blame(crate::lines!["// AI topic 1".ai()]);
    topic2_file.assert_lines_and_blame(crate::lines!["// AI topic 2".ai()]);
    topic3_file.assert_lines_and_blame(crate::lines!["// AI topic 3".ai()]);
}

/// Test rebase with no changes (already up to date)
#[test]
fn test_rebase_already_up_to_date() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI".ai()]);
    let feature_commit_before = repo.stage_all_and_commit("AI feature").unwrap().commit_sha;

    // Try to rebase onto itself (should be no-op)
    repo.git(&["rebase", "feature"])
        .expect("Rebase onto self should succeed");

    // Verify commit unchanged
    let current_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_eq!(
        current_commit, feature_commit_before,
        "Commit should be unchanged"
    );

    // Verify authorship still intact
    feature_file.assert_lines_and_blame(crate::lines!["// AI".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_rebase_fast_forward,
    test_rebase_with_explicit_branch_argument_preserves_authorship,
    test_rebase_root_with_explicit_branch_argument_preserves_authorship,
    test_rebase_patch_stack,
    test_rebase_already_up_to_date,
);
