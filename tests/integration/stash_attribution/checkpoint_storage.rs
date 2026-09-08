use super::{ExpectedLineExt, TestRepo, fs, joke_lines, lines_to_content, stash_v2_dir};

#[test]
fn test_stash_apply_reset_apply_again() {
    // Test that AI attributions survive multiple apply/reset cycles
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI content
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["AI line 1".ai(), "AI line 2".ai(), "AI line 3".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes (using regular stash, not apply, so we can test the workflow)
    repo.git(&["stash"]).expect("stash should succeed");
    assert!(repo.read_file("example.txt").is_none());

    // Apply the stash (NOT pop, so it stays in the stash list)
    repo.git(&["stash", "apply", "stash@{0}"])
        .expect("stash apply should succeed");
    assert!(repo.read_file("example.txt").is_some());

    // Reset to undo the apply
    repo.git(&["reset", "--hard"])
        .expect("reset should succeed");
    assert!(repo.read_file("example.txt").is_none());

    // Apply the same stash again
    repo.git(&["stash", "apply", "stash@{0}"])
        .expect("second stash apply should succeed");
    assert!(repo.read_file("example.txt").is_some());

    // Commit the changes
    let commit = repo
        .stage_all_and_commit("apply stash after reset")
        .expect("commit should succeed");

    // Verify AI attribution is preserved after multiple apply/reset cycles
    example.assert_lines_and_blame(vec!["AI line 1".ai(), "AI line 2".ai(), "AI line 3".ai()]);

    // Check authorship log has AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log after multiple apply/reset cycles"
    );
}

#[test]
fn test_stash_apply_shift_uses_final_commit_tree_after_later_edit() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.txt");

    fs::write(&file_path, "root\nanchor\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    fs::write(&file_path, "root\nAI stashed\nanchor\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();
    repo.git(&["stash", "push", "-m", "ai stash"])
        .expect("stash should succeed");

    repo.human_edit("example.txt", "root\nanchor\ntarget human\n");
    repo.stage_all_and_commit("target head change").unwrap();

    repo.git(&["stash", "apply"])
        .expect("stash apply should succeed");
    fs::write(
        &file_path,
        "root\nAI stashed\nanchor\ntarget human\nlate untracked\n",
    )
    .unwrap();
    repo.git(&["add", "example.txt"]).unwrap();
    repo.commit("commit applied stash with later edit").unwrap();

    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(crate::lines![
        "root".unattributed_human(),
        "AI stashed".ai(),
        "anchor".unattributed_human(),
        "target human".human(),
        "late untracked".unattributed_human(),
    ]);
}

/// Regression: on case-insensitive filesystems (macOS/Windows), the shift-path
/// reconstruction (`reconstruct_stash_applied_contents`) checked out the target
/// tree with `checkout-index -a` (no `-f`). If that tree contained a case-colliding
/// pair (e.g. `README.md` and `readme.md`), git aborted the second checkout with
/// "already exists, no checkout" (exit 1), and `require_success` zeroed out the
/// entire stash attribution restore -- silently dropping the AI note.
#[test]
fn test_stash_apply_shift_survives_case_colliding_target_tree() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.txt");

    fs::write(&file_path, "root\nanchor\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Stash an AI change against the current base.
    fs::write(&file_path, "root\nAI stashed\nanchor\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();
    repo.git(&["stash", "push", "-m", "ai stash"])
        .expect("stash should succeed");

    // Advance HEAD (so base_commit != current_head => shift path) and give the
    // target tree a case-colliding pair via plumbing. The working tree can't hold
    // both casings on a case-insensitive FS, so build the extra index entry from
    // the existing README blob and commit it.
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("add README").unwrap();

    let readme_blob = repo
        .git_og(&["rev-parse", "HEAD:README.md"])
        .unwrap()
        .trim()
        .to_string();
    repo.git_og(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{readme_blob},readme.md"),
    ])
    .unwrap();
    repo.git_og(&["commit", "-m", "add case-colliding readme.md"])
        .unwrap();

    // Apply the stash onto the new HEAD and commit.
    repo.git(&["stash", "apply"])
        .expect("stash apply should succeed");
    repo.git(&["add", "example.txt"]).unwrap();
    let commit = repo.commit("apply stash onto case-colliding tree").unwrap();

    // The AI attribution must survive despite the case-colliding target tree.
    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(crate::lines![
        "root".unattributed_human(),
        "AI stashed".ai(),
        "anchor".unattributed_human(),
    ]);
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log - stash attribution lost on case-colliding target tree"
    );
}

#[test]
fn test_repeated_stash_pop_does_not_duplicate_checkpoints() {
    let repo = TestRepo::new();
    for file_idx in 0..10 {
        fs::write(repo.path().join(format!("jokes_{file_idx}.txt")), "base\n").unwrap();
    }
    repo.stage_all_and_commit("initial jokes").unwrap();

    let expected_first_file = joke_lines(0, 300);
    for file_idx in 0..10 {
        let lines = joke_lines(file_idx, 300);
        fs::write(
            repo.path().join(format!("jokes_{file_idx}.txt")),
            lines_to_content(&lines),
        )
        .unwrap();
    }
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    let initial_working_log = repo.current_working_logs();
    let initial_checkpoint_count = initial_working_log
        .read_all_checkpoints()
        .expect("read checkpoints before stash")
        .len();
    let initial_size = fs::metadata(initial_working_log.dir.join("checkpoints.jsonl"))
        .expect("checkpoints file exists before stash")
        .len();
    assert!(
        initial_checkpoint_count > 0,
        "test setup should create at least one checkpoint"
    );

    for round in 0..5 {
        repo.git(&["stash", "push", "-m", &format!("round {round}")])
            .expect("stash push should succeed");
        repo.git(&["stash", "pop"])
            .expect("stash pop should succeed");
    }

    let final_working_log = repo.current_working_logs();
    let final_checkpoints = final_working_log
        .read_all_checkpoints()
        .expect("read checkpoints after repeated stash");
    let final_size = fs::metadata(final_working_log.dir.join("checkpoints.jsonl"))
        .map(|metadata| metadata.len())
        .unwrap_or(0);

    assert!(
        final_checkpoints.len() <= initial_checkpoint_count,
        "stash/pop should not duplicate checkpoint history: initial={}, final={}",
        initial_checkpoint_count,
        final_checkpoints.len()
    );
    assert!(
        final_size <= initial_size.max(1),
        "checkpoints.jsonl should not grow across stash/pop cycles: initial={} final={}",
        initial_size,
        final_size
    );

    repo.stage_all_and_commit("commit repeated stash result")
        .expect("commit should succeed");
    let mut file = repo.filename("jokes_0.txt");
    file.assert_committed_lines(
        expected_first_file
            .into_iter()
            .map(|line| line.ai())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn test_stash_operation_deletes_legacy_stashes_dir() {
    let repo = TestRepo::new();
    fs::write(repo.path().join("example.txt"), "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    fs::write(repo.path().join("example.txt"), "base\nai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();

    let legacy_dir = repo.path().join(".git").join("ai").join("stashes");
    fs::create_dir_all(legacy_dir.join("old_stash_worklog")).unwrap();
    fs::write(
        legacy_dir
            .join("old_stash_worklog")
            .join("checkpoints.jsonl"),
        "legacy checkpoint data\n".repeat(1024),
    )
    .unwrap();

    repo.git(&["stash", "push", "-m", "legacy cleanup"])
        .expect("stash push should succeed");
    repo.sync_daemon_force();

    assert!(
        !legacy_dir.exists(),
        "legacy .git/ai/stashes must be deleted instead of read or appended"
    );
    assert!(
        stash_v2_dir(&repo).exists(),
        "new stash data should be stored under stashes_v2"
    );
}

crate::reuse_tests_in_worktree!(
    test_stash_apply_reset_apply_again,
    test_stash_apply_shift_uses_final_commit_tree_after_later_edit,
);
