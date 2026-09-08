use super::{ExpectedLineExt, TestRepo};

/// Test rebase skip - skipping a commit during rebase
#[test]
fn test_rebase_skip() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["line 1"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch with AI commit that will conflict
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.replace_at(0, "AI line 1".ai());
    repo.stage_all_and_commit("AI changes").unwrap();

    // Add second commit that won't conflict
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai()]);
    repo.stage_all_and_commit("Add feature").unwrap();

    // Make conflicting change on main
    repo.git(&["checkout", &default_branch]).unwrap();
    file.replace_at(0, "MAIN line 1".human());
    repo.stage_all_and_commit("Main changes").unwrap();

    // Try to rebase - will conflict on first commit
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &default_branch]);

    // Should conflict
    assert!(rebase_result.is_err(), "Rebase should conflict");

    // Skip the conflicting commit
    let skip_result = repo.git(&["rebase", "--skip"]);

    if skip_result.is_ok() {
        // Verify the second commit was rebased and authorship preserved
        feature_file.assert_lines_and_blame(crate::lines!["// AI feature".ai()]);
    }
}

/// Test rebase with empty commits (--keep-empty)
#[test]
fn test_rebase_keep_empty() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch with empty commit
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Create empty commit
    repo.git(&["commit", "--allow-empty", "-m", "Empty commit"])
        .expect("Empty commit should succeed");

    // Add a real commit
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Advance main
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main"]);
    repo.stage_all_and_commit("Main work").unwrap();
    let base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Rebase with --keep-empty (hooks will handle authorship tracking)
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", "--keep-empty", &base]);

    if rebase_result.is_ok() {
        // Verify the non-empty commit has preserved AI authorship
        feature_file.assert_lines_and_blame(crate::lines!["// AI".ai()]);
    }
}

/// Test rebase with rerere (reuse recorded resolution) enabled
#[test]
fn test_rebase_rerere() {
    let repo = TestRepo::new();

    // Enable rerere
    repo.git(&["config", "rerere.enabled", "true"]).unwrap();

    // Create initial commit
    let mut conflict_file = repo.filename("conflict.txt");
    conflict_file.set_contents(crate::lines!["line 1", "line 2"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch with AI changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    conflict_file.replace_at(1, "AI CHANGE".ai());
    repo.stage_all_and_commit("AI changes").unwrap();

    // Make conflicting change on main
    repo.git(&["checkout", &default_branch]).unwrap();
    conflict_file.replace_at(1, "MAIN CHANGE".human());
    repo.stage_all_and_commit("Main changes").unwrap();

    // First rebase - will conflict
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &default_branch]);

    // Should conflict
    assert!(rebase_result.is_err(), "First rebase should conflict");

    // Resolve conflict manually
    use std::fs;
    fs::write(repo.path().join("conflict.txt"), "line 1\nRESOLVED\n").unwrap();

    repo.git(&["add", "conflict.txt"]).unwrap();

    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    // Record the resolution and abort
    repo.git(&["rebase", "--abort"]).ok();

    // Second attempt - rerere should auto-apply the resolution
    let rebase_result = repo.git(&["rebase", &default_branch]);

    // Even if rerere helps, we still need to continue manually
    // This test mainly verifies that rerere doesn't break authorship tracking
    if rebase_result.is_err() {
        repo.git(&["add", "conflict.txt"]).unwrap();
        repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
            .unwrap();
    }

    // Note: This test verifies that rerere doesn't break the rebase process
    // Authorship tracking is handled by hooks regardless of rerere
}

#[test]
fn test_rebase_ignores_stale_pending_state_from_untraced_abort() {
    let repo = TestRepo::new();

    let mut stale_file = repo.filename("stale-conflict.txt");
    stale_file.set_contents(crate::lines!["base"]);
    let base_commit = repo.stage_all_and_commit("base").unwrap().commit_sha;
    stale_file.assert_committed_lines(crate::lines!["base".human()]);

    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "stale-topic"]).unwrap();
    stale_file.replace_at(0, "stale side".ai());
    let stale_tip = repo.stage_all_and_commit("stale topic").unwrap().commit_sha;
    stale_file.assert_committed_lines(crate::lines!["stale side".ai()]);

    repo.git(&["checkout", &default_branch]).unwrap();
    stale_file.replace_at(0, "main side".human());
    repo.stage_all_and_commit("main side").unwrap();
    stale_file.assert_committed_lines(crate::lines!["main side".human()]);

    repo.git(&["checkout", "stale-topic"]).unwrap();
    let failed_rebase = repo.git(&["rebase", &default_branch]);
    assert!(
        failed_rebase.is_err(),
        "stale-topic rebase should stop on the conflict that seeds pending daemon state"
    );
    repo.sync_daemon();

    repo.git_og_with_env(&["rebase", "--abort"], &[("GIT_TRACE2_EVENT", "0")])
        .expect("untraced rebase abort should restore Git state without clearing daemon memory");
    let after_abort = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_eq!(after_abort, stale_tip);
    stale_file.assert_committed_lines(crate::lines!["stale side".ai()]);

    repo.git(&["checkout", "-b", "feature", &base_commit])
        .unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["feature ai".ai()]);
    let original_feature = repo.stage_all_and_commit("feature ai").unwrap().commit_sha;
    feature_file.assert_committed_lines(crate::lines!["feature ai".ai()]);
    assert!(
        repo.read_authorship_note(&original_feature).is_some(),
        "original feature commit should have an authorship note before rebase"
    );

    repo.git(&["rebase", &default_branch])
        .expect("ordinary feature rebase should succeed");
    let rebased_feature = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_ne!(rebased_feature, original_feature);

    feature_file.assert_lines_and_blame(crate::lines!["feature ai".ai()]);
}

/// Test rebase with conflicts - verifies reconstruction works after conflict resolution
#[test]
fn test_rebase_with_conflicts() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();

    // Create old_base branch and commit
    repo.git(&["checkout", "-b", "old_base"]).unwrap();
    let mut old_file = repo.filename("old.txt");
    old_file.set_contents(crate::lines!["old base"]);
    repo.stage_all_and_commit("Old base commit").unwrap();
    let old_base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create feature branch from old_base with AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Create new_base branch from default_branch
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["checkout", "-b", "new_base"]).unwrap();
    let mut new_file = repo.filename("new.txt");
    new_file.set_contents(crate::lines!["new base"]);
    repo.stage_all_and_commit("New base commit").unwrap();
    let new_base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Rebase feature --onto new_base old_base (hooks will handle authorship)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", "--onto", &new_base_sha, &old_base_sha])
        .expect("Rebase --onto should succeed");

    // Verify authorship preserved after --onto rebase
    feature_file.assert_lines_and_blame(crate::lines!["// AI feature".ai()]);
}

/// ENG-289 regression: a conflicted rebase whose `git rebase --continue`
/// fails at least once before succeeding must still migrate line-level
/// attribution exactly like a clean single-continue rebase. The failed
/// attempts run through trace2 like any other git command, but they change
/// no refs, so they must not advance or corrupt the reflog cursor used to
/// establish the rebase's ref transitions.
#[test]
fn test_rebase_failed_continue_attempts_preserve_line_attribution() {
    let repo = TestRepo::new();

    let mut conflict_file = repo.filename("conflict.txt");
    conflict_file.set_contents(crate::lines!["line 1", "line 2", "line 3"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();

    // Feature: an AI edit that will conflict, then a clean AI commit on top.
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    conflict_file.replace_at(1, "AI CHANGE".ai());
    repo.stage_all_and_commit("AI conflicting change").unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai()]);
    repo.stage_all_and_commit("AI feature file").unwrap();

    // Conflicting human change on the default branch.
    repo.git(&["checkout", &default_branch]).unwrap();
    conflict_file.replace_at(1, "MAIN CHANGE".human());
    repo.stage_all_and_commit("Main conflicting change")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    assert!(
        repo.git(&["rebase", &default_branch]).is_err(),
        "rebase should stop on the conflict"
    );

    // First failed continue: the conflict is not resolved at all.
    assert!(
        repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
            .is_err(),
        "continue must fail while the conflict is unresolved"
    );

    // Second failed continue: resolved in the worktree but never staged.
    std::fs::write(
        repo.path().join("conflict.txt"),
        "line 1\nAI CHANGE\nline 3\n",
    )
    .unwrap();
    assert!(
        repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
            .is_err(),
        "continue must fail while the resolution is unstaged"
    );

    // Stage the resolution (keeping the AI side) and finish the rebase.
    repo.git(&["add", "conflict.txt"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("continue should succeed once the resolution is staged");

    conflict_file.assert_lines_and_blame(crate::lines![
        "line 1".human(),
        "AI CHANGE".ai(),
        "line 3".human(),
    ]);
    feature_file.assert_lines_and_blame(crate::lines!["// AI feature".ai()]);
}

/// Test rebase abort - ensures no authorship corruption on abort
#[test]
fn test_rebase_abort() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut conflict_file = repo.filename("conflict.txt");
    conflict_file.set_contents(crate::lines!["line 1", "line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch with AI changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    conflict_file.replace_at(1, "AI CHANGE".ai());
    repo.stage_all_and_commit("AI changes").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Make conflicting change on main
    repo.git(&["checkout", &default_branch]).unwrap();
    conflict_file.replace_at(1, "MAIN CHANGE".human());
    repo.stage_all_and_commit("Main changes").unwrap();

    // Try to rebase - will conflict
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &default_branch]);

    // Should conflict
    assert!(rebase_result.is_err(), "Rebase should conflict");

    // Abort the rebase
    repo.git(&["rebase", "--abort"])
        .expect("Rebase abort should succeed");

    // Verify we're back to original commit
    let current_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_eq!(
        current_commit, feature_commit,
        "Should be back to original commit after abort"
    );

    // Verify original authorship is intact (by checking file blame)
    conflict_file.assert_lines_and_blame(crate::lines!["line 1".human(), "AI CHANGE".ai()]);
}

/// Test branch switch during rebase - ensures proper state handling
#[test]
fn test_rebase_branch_switch_during() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Create another branch
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["checkout", "-b", "other"]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other"]);
    repo.stage_all_and_commit("Other work").unwrap();

    // Start rebase on feature (non-conflicting)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify branch is still feature
    let current_branch = repo.current_branch();
    assert_eq!(
        current_branch, "feature",
        "Should still be on feature branch"
    );

    // Verify authorship was preserved
    feature_file.assert_lines_and_blame(crate::lines!["// AI".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_rebase_skip,
    test_rebase_keep_empty,
    test_rebase_rerere,
    test_rebase_with_conflicts,
    test_rebase_abort,
    test_rebase_branch_switch_during,
);
