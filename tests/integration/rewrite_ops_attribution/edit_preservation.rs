use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_rebase_autostash_preserves_uncommitted_ai_worktree_attribution() {
    let repo = TestRepo::new();
    let committed_path = repo.path().join("committed.csv");
    let worktree_path = repo.path().join("worktree.csv");

    fs::write(&committed_path, "id,value\nbase,seed\n").unwrap();
    fs::write(&worktree_path, "id,value\nwork,seed\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(&committed_path, "id,value\nbase,seed\nfeature,rebase\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "committed.csv"])
        .unwrap();
    repo.stage_all_and_commit("feature committed line").unwrap();
    let mut committed = repo.filename("committed.csv");
    committed.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "feature,rebase".ai(),
    ]);

    fs::write(&worktree_path, "id,value\nwork,seed\nworktree,ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "worktree.csv"])
        .unwrap();
    let mut worktree = repo.filename("worktree.csv");

    repo.git(&["stash", "push", "-m", "temporary-worktree"])
        .unwrap();
    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(repo.path().join("main.csv"), "id,value\nmain,rebase\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.csv"]).unwrap();
    repo.stage_all_and_commit("main unrelated line").unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["stash", "pop"]).unwrap();

    repo.git(&["rebase", &main_branch, "--autostash"]).unwrap();

    committed.assert_lines_and_blame(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "feature,rebase".ai(),
    ]);

    repo.git(&["add", "worktree.csv"]).unwrap();
    repo.commit("commit autostashed worktree line").unwrap();
    worktree.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "work,seed".unattributed_human(),
        "worktree,ai".ai(),
    ]);
}

/// Simpler version: interleaved human and AI edits, note must not lump them together.
#[test]
fn test_interleaved_human_ai_edits_not_lumped() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Human edits: prepend 3 lines
    fs::write(&file_path, "human1\nhuman2\nhuman3\ninit\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    // AI edits: prepend 1 line
    fs::write(&file_path, "ai-top\nhuman1\nhuman2\nhuman3\ninit\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("mixed commit").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-top".ai(),
        "human1".human(),
        "human2".human(),
        "human3".human(),
        "init".ai(),
    ]);
}

/// Rebase then commit: notes should transfer through rebase for rebased commits.
#[test]
fn test_rebase_preserves_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let main_branch = repo.current_branch();

    // Feature branch: AI commit
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(&file_path, "base\nfeature-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature").unwrap();

    // Advance main with a non-conflicting change
    repo.git(&["checkout", &main_branch]).unwrap();
    let other_path = repo.path().join("other.txt");
    fs::write(&other_path, "main-work\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "other.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("advance main").unwrap();

    // Rebase feature onto main (through daemon)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // Merge back (fast-forward)
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["merge", "feature"]).unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["base".ai(), "feature-ai".ai()]);
}

/// Amend: amending a commit should preserve attribution for unchanged lines
/// and correctly attribute new lines.
#[test]
fn test_amend_preserves_existing_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "first\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit with AI
    fs::write(&file_path, "first\nsecond-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    // Amend: add a human line
    fs::write(&file_path, "first\nsecond-ai\nthird-human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "--amend", "-m", "second amended"])
        .unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "first".ai(),
        "second-ai".ai(),
        "third-human".human(),
    ]);
}

// =============================================================================
// Category H: Selective staging attribution carryover
// =============================================================================

/// When committing only one of multiple checkpointed files, the dirty file's
/// attribution must survive to the next commit via INITIAL carryover.
#[test]
fn test_selective_commit_preserves_dirty_file_attribution() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("secondary.txt");

    // Initial commit
    fs::write(&main_path, "main-init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Edit BOTH files with attribution
    fs::write(&main_path, "main-init\nmain-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    fs::write(&sec_path, "sec-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();

    // Commit ONLY main, leave secondary dirty
    repo.git(&["add", "main.txt"]).unwrap();
    repo.commit("main only").unwrap();

    // Now commit secondary
    repo.git(&["add", "secondary.txt"]).unwrap();
    repo.commit("secondary").unwrap();

    // Secondary must retain its AI attribution
    let mut sec_file = repo.filename("secondary.txt");
    sec_file.assert_committed_lines(crate::lines!["sec-ai".ai(),]);
}
