use super::{ExpectedLineExt, TestRepo, fs};

/// Reset then re-edit and squash: AI lines in the middle must not fall into gaps.
#[test]
fn test_reset_reedit_squash_no_attribution_gaps() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit with mixed content
    fs::write(&file_path, "aaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit: add more AI lines
    fs::write(&file_path, "aaa\nbbb\nccc\nddd\neee\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("add more").unwrap();

    // Reset to initial
    repo.git(&["reset", "--mixed", "HEAD~1"]).unwrap();

    // Re-edit: human prepends, then AI appends
    fs::write(&file_path, "human-top\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    fs::write(
        &file_path,
        "human-top\naaa\nbbb\nccc\nai-bot\nai-bot\nai-bot\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("re-edit after reset").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "human-top".human(),
        "aaa".ai(),
        "bbb".ai(),
        "ccc".ai(),
        "ai-bot".ai(),
        "ai-bot".ai(),
        "ai-bot".ai(),
    ]);
}

// =============================================================================
// Category C: Reset sequencing before subsequent checkpoints
//
// A checkpoint immediately after reset must see working-log state after reset,
// not stale state from the commit that reset just removed.
// =============================================================================

#[test]
fn test_hard_reset_then_ai_checkpoint_preserves_new_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    fs::write(&file_path, "base\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    fs::write(&file_path, "new-1\nnew-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("after reset").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["new-1".ai(), "new-2".ai(),]);
}

/// Simpler test: does overwriting all content work without a reset?
#[test]
fn test_overwrite_all_content_ai() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    fs::write(&file_path, "new-1\nnew-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["new-1".ai(), "new-2".ai(),]);
}

/// Same as above but with --mixed reset to see if bug is --hard specific.
#[test]
fn test_mixed_reset_then_ai_checkpoint() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit
    fs::write(&file_path, "base\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    // Mixed reset back to initial
    repo.git(&["reset", "--mixed", "HEAD~1"]).unwrap();

    // New AI edits after mixed reset (same content as hard reset test)
    fs::write(&file_path, "new-ai-1\nnew-ai-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("after mixed reset").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["new-ai-1".ai(), "new-ai-2".ai(),]);
}

/// Hard reset then mixed AI and human checkpoints — both must be correctly attributed.
#[test]
fn test_hard_reset_mixed_checkpoint_types() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit to create something to reset
    fs::write(&file_path, "init\nmore\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    // Hard reset
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // Human edits first
    fs::write(&file_path, "human-line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    // Then AI appends
    fs::write(&file_path, "human-line\nai-line\nai-line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("post-reset mixed").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "human-line".human(),
        "ai-line".ai(),
        "ai-line".ai(),
    ]);
}

/// Hard reset THEN overwrite+human pattern — simple variant.
#[test]
fn test_overbroad_after_hard_reset_overwrite_human() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "line-1\nline-2\nline-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit (something to reset from)
    fs::write(&file_path, "line-1\nline-2\nline-3\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    // Hard reset back, then checkpoint immediately.
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // AI OverwriteAll
    fs::write(&file_path, "Y-ai\nY-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human Append
    fs::write(&file_path, "Y-ai\nY-ai\nZ-human\nZ-human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("overwrite-and-rollback").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "Y-ai".ai(),
        "Y-ai".ai(),
        "Z-human".human(),
        "Z-human".human(),
    ]);
}
