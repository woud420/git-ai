use super::{
    TestRepo, compute_file_line_stats, find_repository_in_path, setup_repo_with_base_commit,
};

#[test]
fn test_compute_line_stats_ignores_whitespace_only_lines() {
    let (repo, _lines_file, _alphabet_file) = setup_repo_with_base_commit();

    let gitai_repo =
        find_repository_in_path(repo.path().to_str().unwrap()).expect("Repository should exist");

    let base_commit = gitai_repo
        .head()
        .ok()
        .and_then(|head| head.target().ok())
        .unwrap_or_else(|| "initial".to_string());
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();

    std::fs::write(repo.path().join("whitespace.txt"), "Seed line\n").unwrap();
    repo.git(&["add", "whitespace.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "whitespace.txt"])
        .expect("Setup checkpoint should succeed");

    let file_path = repo.path().join("whitespace.txt");
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("\n\n   \nVisible line one\n\n\t\nVisible line two\n  \n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", "whitespace.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "whitespace.txt"])
        .expect("First checkpoint should succeed");

    let after_add_stats = working_log
        .read_all_checkpoints()
        .expect("Should read checkpoints after addition");
    let after_add_last = after_add_stats
        .last()
        .expect("At least one checkpoint expected")
        .line_stats
        .clone();

    assert_eq!(
        after_add_last.additions, 8,
        "Additions includes empty lines"
    );
    assert_eq!(after_add_last.deletions, 0, "No deletions expected yet");
    assert_eq!(
        after_add_last.additions_sloc, 2,
        "Only visible lines counted"
    );
    assert_eq!(
        after_add_last.deletions_sloc, 0,
        "No deletions expected yet"
    );

    let cleaned_content = std::fs::read_to_string(&file_path).unwrap();
    let cleaned_lines: Vec<&str> = cleaned_content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let cleaned_body = format!("{}\n", cleaned_lines.join("\n"));
    std::fs::write(&file_path, &cleaned_body).unwrap();
    repo.git(&["add", "whitespace.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "whitespace.txt"])
        .expect("Second checkpoint should succeed");

    let after_delete_stats = working_log
        .read_all_checkpoints()
        .expect("Should read checkpoints after deletion");
    let latest_stats = after_delete_stats
        .last()
        .expect("At least one checkpoint expected")
        .line_stats
        .clone();

    assert_eq!(
        latest_stats.additions, 0,
        "No additions in cleanup checkpoint"
    );
    assert_eq!(latest_stats.deletions, 6, "Deletions includes empty lines");
    assert_eq!(
        latest_stats.additions_sloc, 0,
        "No additions in cleanup checkpoint"
    );
    assert_eq!(
        latest_stats.deletions_sloc, 0,
        "Whitespace deletions ignored"
    );
}

// ====================================================================
// CRLF / LF normalization tests for compute_file_line_stats
// ====================================================================

#[test]
fn test_compute_file_line_stats_crlf_to_lf_no_changes() {
    // Same content, only line endings differ (CRLF → LF).
    // Stats should show 0 additions and 0 deletions.
    let old = "line1\r\nline2\r\nline3\r\n";
    let new = "line1\nline2\nline3\n";

    let stats = compute_file_line_stats(old, new);

    assert_eq!(
        stats.additions, 0,
        "CRLF→LF with identical content should show 0 additions"
    );
    assert_eq!(
        stats.deletions, 0,
        "CRLF→LF with identical content should show 0 deletions"
    );
}

#[test]
fn test_compute_file_line_stats_lf_to_crlf_no_changes() {
    let old = "line1\nline2\nline3\n";
    let new = "line1\r\nline2\r\nline3\r\n";

    let stats = compute_file_line_stats(old, new);

    assert_eq!(
        stats.additions, 0,
        "LF→CRLF with identical content should show 0 additions"
    );
    assert_eq!(
        stats.deletions, 0,
        "LF→CRLF with identical content should show 0 deletions"
    );
}

#[test]
fn test_compute_file_line_stats_crlf_to_lf_with_additions() {
    // Reproduces the user-reported bug: file with CRLF, AI adds lines with LF.
    // Old: 3 CRLF lines. New: same 3 lines (LF) + 2 new lines.
    // Should show exactly 2 additions and 0 deletions.
    let old = "line1\r\nline2\r\nline3\r\n";
    let new = "line1\nline2\nline3\nnew_a\nnew_b\n";

    let stats = compute_file_line_stats(old, new);

    assert_eq!(
        stats.additions, 2,
        "Should have exactly 2 additions (the new lines)"
    );
    assert_eq!(
        stats.deletions, 0,
        "Should have 0 deletions (no lines removed)"
    );
}

#[test]
fn test_compute_file_line_stats_crlf_large_file_user_reported_bug() {
    // Exact scenario from user report:
    // 100-line CRLF file, AI adds 5 lines (with LF).
    // Should show +5 -0, NOT +105 -100.
    let mut old = String::new();
    for i in 1..=100 {
        old.push_str(&format!("line number {}\r\n", i));
    }

    let mut new = String::new();
    for i in 1..=100 {
        new.push_str(&format!("line number {}\n", i));
    }
    for i in 1..=5 {
        new.push_str(&format!("new ai line {}\n", i));
    }

    let stats = compute_file_line_stats(&old, &new);

    assert_eq!(
        stats.additions, 5,
        "Should have exactly 5 additions (AI-added lines), not {}",
        stats.additions
    );
    assert_eq!(
        stats.deletions, 0,
        "Should have 0 deletions, not {}",
        stats.deletions
    );
}

// ====================================================================
// End-to-end CRLF test: blob has CRLF, working tree has LF
// Simulates the real-world scenario where git stores CRLF (or autocrlf
// converts on checkout) and an AI tool writes LF.
// ====================================================================

#[test]
fn test_checkpoint_crlf_blob_vs_lf_working_tree_stats_not_inflated() {
    // Step 1: Create a repo and commit a file with CRLF line endings.
    // On Linux without autocrlf, the blob stores CRLF verbatim.
    let repo = TestRepo::new();
    let crlf_content = "line1\r\nline2\r\nline3\r\nline4\r\nline5\r\n";
    std::fs::write(repo.path().join("test.txt"), crlf_content).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.stage_all_and_commit("initial commit with CRLF")
        .unwrap();

    // Step 2: Overwrite the file with LF endings + one new line,
    // simulating an AI tool that writes LF on a Windows repo.
    let lf_content_with_addition = "line1\nline2\nline3\nline4\nline5\nnew_ai_line\n";
    std::fs::write(repo.path().join("test.txt"), lf_content_with_addition).unwrap();

    // Step 3: Run a checkpoint
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Step 4: Read back checkpoint stats
    let gitai_repo =
        find_repository_in_path(repo.path().to_str().unwrap()).expect("Repository should exist");
    let base_commit = gitai_repo
        .head()
        .ok()
        .and_then(|head| head.target().ok())
        .unwrap_or_else(|| "initial".to_string());
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints
        .last()
        .expect("Should have at least one checkpoint");

    // The key assertion: stats should reflect only the actual addition,
    // NOT inflate every line because of CRLF→LF conversion.
    assert_eq!(
        latest.line_stats.additions, 1,
        "Should have 1 addition (the new AI line), not {} (which would mean CRLF→LF inflated the count)",
        latest.line_stats.additions
    );
    assert_eq!(
        latest.line_stats.deletions, 0,
        "Should have 0 deletions, not {} (which would mean CRLF→LF caused all old lines to appear deleted)",
        latest.line_stats.deletions
    );
}

#[test]
fn test_checkpoint_crlf_blob_vs_lf_working_tree_no_changes_skipped() {
    // When the only difference is CRLF→LF (no actual content change),
    // the checkpoint should skip the file entirely — normalized comparison
    // detects they're equal and returns None.
    let repo = TestRepo::new();
    let crlf_content = "line1\r\nline2\r\nline3\r\n";
    std::fs::write(repo.path().join("test.txt"), crlf_content).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.stage_all_and_commit("initial commit with CRLF")
        .unwrap();

    // Overwrite with LF-only — same text content, different line endings
    let lf_content = "line1\nline2\nline3\n";
    std::fs::write(repo.path().join("test.txt"), lf_content).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    let gitai_repo =
        find_repository_in_path(repo.path().to_str().unwrap()).expect("Repository should exist");
    let base_commit = gitai_repo
        .head()
        .ok()
        .and_then(|head| head.target().ok())
        .unwrap_or_else(|| "initial".to_string());
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints = working_log.read_all_checkpoints().unwrap();

    // The checkpoint may be empty (no entries) or absent entirely,
    // because normalized comparison correctly detected no real change.
    if let Some(latest) = checkpoints.last() {
        let test_entry = latest.entries.iter().find(|e| e.file == "test.txt");
        assert!(
            test_entry.is_none(),
            "test.txt should be skipped when only line endings differ"
        );
    }
    // If no checkpoints at all, that's also correct — nothing changed.
}
