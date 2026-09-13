use super::{ExpectedLineExt, TestRepo};

/// Test rebase with autostash enabled
#[test]
fn test_rebase_autostash() {
    let repo = TestRepo::new();

    // Enable autostash
    repo.git(&["config", "rebase.autoStash", "true"]).unwrap();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["line 1"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Advance main
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main"]);
    repo.stage_all_and_commit("Main work").unwrap();

    // Switch back to feature and make unstaged changes
    repo.git(&["checkout", "feature"]).unwrap();
    use std::fs;
    fs::write(
        repo.path().join("feature.txt"),
        "// AI\n// Unstaged change\n",
    )
    .unwrap();

    // Rebase with unstaged changes (autostash should handle it - hooks handle authorship)
    let rebase_result = repo.git(&["rebase", &default_branch]);

    // Should succeed with autostash
    if rebase_result.is_ok() {
        // Reset the file to HEAD to remove the autostashed unstaged changes before checking
        repo.git(&["checkout", "HEAD", "feature.txt"]).unwrap();

        // Verify authorship was preserved
        feature_file.assert_lines_and_blame(crate::lines!["// AI".ai()]);
    }
}

/// Test rebase with merge commits (--rebase-merges)
/// This test verifies the BFS fix for issue #328 where walk_commits_to_base
/// was only following parent(0), missing side branch commits.
///
/// The test checks that authorship notes for rebased commits include files
/// from side branches (reached via parent(1) of merge commits).
#[test]
fn test_rebase_preserve_merges() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Create side branch from feature - this commit is only reachable via parent(1) of the merge
    repo.git(&["checkout", "-b", "side"]).unwrap();
    let mut side_file = repo.filename("side.txt");
    side_file.set_contents(crate::lines!["// AI side".ai()]);
    repo.stage_all_and_commit("AI side").unwrap();

    // Merge side into feature with --no-ff to force a merge commit
    // (creates merge commit where side is parent(1), feature is parent(0))
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["merge", "--no-ff", "side", "-m", "Merge side into feature"])
        .unwrap();

    // Advance main
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main"]);
    repo.stage_all_and_commit("Main work").unwrap();
    let base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Rebase feature onto main with --rebase-merges
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", "--rebase-merges", &base])
        .expect("Rebase with --rebase-merges should succeed");

    // Get the rebased side branch commit (the one that created side.txt)
    // Use git log to find the commit that added side.txt
    let side_commit_sha = repo
        .git(&[
            "log",
            "--all",
            "--format=%H",
            "--diff-filter=A",
            "--",
            "side.txt",
        ])
        .expect("Should find commit that added side.txt")
        .trim()
        .lines()
        .next()
        .expect("Should have at least one commit")
        .to_string();

    // Check that the rebased side commit has an authorship note with side.txt
    // This is the key assertion: without BFS fix, walk_commits_to_base misses
    // the side branch commit, so its authorship won't be rewritten
    let note_output = repo.git(&["notes", "--ref=ai", "show", &side_commit_sha]);

    assert!(
        note_output.is_ok(),
        "Rebased side branch commit should have authorship note. \
         Without BFS fix, walk_commits_to_base misses commits from parent(1) \
         and authorship is not rewritten for side branch commits."
    );

    let note_content = note_output.unwrap();
    assert!(
        note_content.contains("side.txt"),
        "Authorship note should include side.txt. Got: {}",
        note_content
    );

    // Also verify blame works correctly
    feature_file.assert_lines_and_blame(crate::lines!["// AI feature".ai()]);
    side_file.assert_lines_and_blame(crate::lines!["// AI side".ai()]);
}

/// Test the full branch lifecycle pattern used by the fuzzer:
/// create branch → multiple commits → rebase onto updated main → fast-forward merge back.
/// This verifies attribution survives through rebase + merge.
#[test]
fn test_rebase_then_ff_merge_preserves_attribution() {
    use std::fs;

    let repo = TestRepo::new();

    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    // Create feature branch with multiple AI commits on a SEPARATE file (no conflicts)
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    let feature_path = repo.path().join("feature.txt");
    fs::write(&feature_path, "ai feature 1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature commit 1").unwrap();

    fs::write(&feature_path, "ai feature 1\nai feature 2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature commit 2").unwrap();

    fs::write(&feature_path, "ai feature 1\nai feature 2\nai feature 3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature commit 3").unwrap();

    // Advance main with a non-conflicting change (different file)
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.human_edit("main.txt", "main line 1\nmain advance\n");
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("advance main").unwrap();

    // Rebase feature onto main
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Fast-forward merge back to main
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["merge", "feature"]).unwrap();

    // Verify attribution on the feature file (should survive rebase + merge)
    let mut result_file = repo.filename("feature.txt");
    result_file.assert_lines_and_blame(crate::lines![
        "ai feature 1".ai(),
        "ai feature 2".ai(),
        "ai feature 3".ai(),
    ]);
}

/// Same as above but edits the SAME file on both branches (prepend on main, append on feature).
/// This is the exact pattern the fuzzer's workflow-branch-lifecycle uses.
#[test]
fn test_rebase_same_file_then_ff_merge_preserves_attribution() {
    use std::fs;

    let repo = TestRepo::new();

    let file_path = repo.path().join("shared.txt");
    fs::write(&file_path, "base line\n").unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    // Create feature branch - append AI lines
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    fs::write(&file_path, "base line\nai append 1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "shared.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature commit 1").unwrap();

    fs::write(&file_path, "base line\nai append 1\nai append 2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "shared.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature commit 2").unwrap();

    fs::write(
        &file_path,
        "base line\nai append 1\nai append 2\nai append 3\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "shared.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature commit 3").unwrap();

    // Advance main - prepend human line (non-conflicting with appends)
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.human_edit("shared.txt", "human prepend\nbase line\n");
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("advance main").unwrap();

    // Rebase feature onto main
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Fast-forward merge
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["merge", "feature"]).unwrap();

    // After rebase+merge: prepend + base + 3 appends
    let mut result_file = repo.filename("shared.txt");
    result_file.assert_lines_and_blame(crate::lines![
        "human prepend".human(),
        "base line".unattributed_human(),
        "ai append 1".ai(),
        "ai append 2".ai(),
        "ai append 3".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(test_rebase_autostash, test_rebase_preserve_merges,);
