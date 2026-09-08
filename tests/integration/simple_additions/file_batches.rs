use super::{ExpectedLineExt, TestRepo, fs};

/// Regression test for issue #356
/// When AI edits multiple files in the same session, but they are committed
/// in separate batches, the second batch loses AI attribution.
/// See: https://github.com/git-ai-project/git-ai/issues/356
#[test]
fn test_multi_file_batch_commits_preserve_attribution() {
    // This test reproduces the exact scenario from issue #356:
    // 1. AI edits two files (file_a.txt and file_b.txt)
    // 2. User commits file_a.txt first -> AI attribution correct ✓
    // 3. User commits file_b.txt second -> AI attribution should be preserved
    use std::fs;

    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates two new files in the same session
    let file_a_path = repo.path().join("file_a.txt");
    let file_b_path = repo.path().join("file_b.txt");

    fs::write(
        &file_a_path,
        "AI content for file A\nLine 2 from AI\nLine 3 from AI\n",
    )
    .unwrap();
    fs::write(
        &file_b_path,
        "AI content for file B\nLine 2 from AI\nLine 3 from AI\n",
    )
    .unwrap();

    // Single AI checkpoint covers both files (same AI session)
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    // First commit: only file_a.txt
    repo.git(&["add", "file_a.txt"]).unwrap();
    repo.commit("Add file A").unwrap();

    // Second commit: file_b.txt (this is where attribution is lost in issue #356)
    repo.git(&["add", "file_b.txt"]).unwrap();
    repo.commit("Add file B").unwrap();

    // Verify file_a.txt has correct AI attribution (this works)
    let mut file_a = repo.filename("file_a.txt");
    file_a.assert_lines_and_blame(crate::lines![
        "AI content for file A".ai(),
        "Line 2 from AI".ai(),
        "Line 3 from AI".ai(),
    ]);

    // Verify file_b.txt ALSO has correct AI attribution (this fails in issue #356)
    let mut file_b = repo.filename("file_b.txt");
    file_b.assert_lines_and_blame(crate::lines![
        "AI content for file B".ai(),
        "Line 2 from AI".ai(),
        "Line 3 from AI".ai(),
    ]);
}

/// Additional test for issue #356 with modifications instead of new files
#[test]
fn test_multi_file_batch_commits_modifications() {
    // Similar to above, but with modifications to existing files
    use std::fs;

    let repo = TestRepo::new();

    // Create initial files (human-authored)
    let file_a_path = repo.path().join("file_a.txt");
    let file_b_path = repo.path().join("file_b.txt");

    fs::write(&file_a_path, "Original content A\n").unwrap();
    fs::write(&file_b_path, "Original content B\n").unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial commit with both files")
        .unwrap();

    // AI modifies both files in the same session
    fs::write(&file_a_path, "Original content A\nAI added line A\n").unwrap();
    fs::write(&file_b_path, "Original content B\nAI added line B\n").unwrap();

    // Single AI checkpoint covers both modifications
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    // First commit: only file_a.txt
    repo.git(&["add", "file_a.txt"]).unwrap();
    repo.commit("Modify file A").unwrap();

    // Second commit: file_b.txt
    repo.git(&["add", "file_b.txt"]).unwrap();
    repo.commit("Modify file B").unwrap();

    // Verify both files have correct AI attribution
    let mut file_a = repo.filename("file_a.txt");
    file_a.assert_lines_and_blame(crate::lines![
        "Original content A".human(),
        "AI added line A".ai(),
    ]);

    let mut file_b = repo.filename("file_b.txt");
    file_b.assert_lines_and_blame(crate::lines![
        "Original content B".human(),
        "AI added line B".ai(), // This fails in issue #356 - shows as human
    ]);
}

#[test]
fn test_ai_edits_file_with_spaces_in_filename() {
    // Test that AI authorship tracking works correctly for files with spaces in the filename
    // This is a potential edge case that could fail if paths aren't properly quoted
    use std::fs;

    let repo = TestRepo::new();
    let file_path = repo.path().join("my test file.txt");

    // Initial commit: Create file with spaces in name
    fs::write(&file_path, "Line 1\nLine 2\nLine 3\n").unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial commit with spaced filename")
        .unwrap();

    // AI adds new lines to the file
    fs::write(&file_path, "Line 1\nLine 2\nAI Line 1\nAI Line 2\nLine 3\n").unwrap();

    // Mark the AI-authored content with mock_ai checkpoint
    repo.git_ai(&["checkpoint", "mock_ai", "my test file.txt"])
        .unwrap();

    repo.stage_all_and_commit("AI adds lines to file with spaces")
        .unwrap();

    // Verify line-by-line attribution
    let mut file = repo.filename("my test file.txt");
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "Line 2".human(),
        "AI Line 1".ai(),
        "AI Line 2".ai(),
        "Line 3".human(),
    ]);
}

/// Reproduces fuzz_chaos_99: multi-file commit followed by soft-reset-recommit.
/// The secondary file's attribution must survive the reset+recommit cycle.
#[test]
fn test_soft_reset_recommit_preserves_secondary_file_attribution() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let secondary_path = repo.path().join("secondary.txt");

    // Initial commit with untracked content
    fs::write(&main_path, "base\n").unwrap();
    fs::write(&secondary_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Edit secondary file with multiple checkpoints (like the fuzzer does)
    // KnownHuman edit
    repo.git_ai(&["checkpoint", "human", "secondary.txt"])
        .unwrap();
    fs::write(&secondary_path, "base\nHH1\nHH2\nHH3\nHH4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "secondary.txt"])
        .unwrap();

    // AI append
    repo.git_ai(&["checkpoint", "human", "secondary.txt"])
        .unwrap();
    fs::write(&secondary_path, "base\nHH1\nHH2\nHH3\nHH4\nAI1\nAI2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();

    // AI prepend (shifts existing lines down)
    repo.git_ai(&["checkpoint", "human", "secondary.txt"])
        .unwrap();
    fs::write(
        &secondary_path,
        "P1\nP2\nP3\nP4\nbase\nHH1\nHH2\nHH3\nHH4\nAI1\nAI2\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();

    // Also edit main file
    repo.git_ai(&["checkpoint", "human", "main.txt"]).unwrap();
    fs::write(&main_path, "base\nmain_ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Commit both files
    repo.stage_all_and_commit("commit with both files").unwrap();

    // Verify attribution before reset
    let mut secondary = repo.filename("secondary.txt");
    secondary.assert_committed_lines(crate::lines![
        "P1".ai(),
        "P2".ai(),
        "P3".ai(),
        "P4".ai(),
        "base".unattributed_human(),
        "HH1".human(),
        "HH2".human(),
        "HH3".human(),
        "HH4".human(),
        "AI1".ai(),
        "AI2".ai(),
    ]);

    // Now do soft-reset-recommit: undo the commit, edit only main.txt, recommit
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();

    // Edit main.txt further and checkpoint
    repo.git_ai(&["checkpoint", "human", "main.txt"]).unwrap();
    fs::write(&main_path, "base\nmain_ai\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Recommit everything
    repo.stage_all_and_commit("recommit after soft reset")
        .unwrap();

    // Secondary file's attribution should be preserved through the reset+recommit
    secondary.assert_committed_lines(crate::lines![
        "P1".ai(),
        "P2".ai(),
        "P3".ai(),
        "P4".ai(),
        "base".unattributed_human(),
        "HH1".human(),
        "HH2".human(),
        "HH3".human(),
        "HH4".human(),
        "AI1".ai(),
        "AI2".ai(),
    ]);
}
