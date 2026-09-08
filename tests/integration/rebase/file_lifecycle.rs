use super::{ExpectedLineExt, TestRepo};

/// Regression test: attributions should survive a delete-recreate cycle within a rebase.
/// If a file is deleted in commit N and recreated in commit N+1, the recreated file
/// should inherit attributions from the pre-deletion state via positional diff transfer.
#[test]
fn test_rebase_file_delete_recreate_preserves_attribution() {
    let repo = TestRepo::new();
    let default_branch = repo.current_branch();

    // Initial setup
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Create feature branch with AI file
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut ai_file = repo.filename("feature.txt");
    ai_file.set_contents(crate::lines!["line1".ai(), "line2".ai(), "line3".ai()]);
    repo.stage_all_and_commit("Add AI file").unwrap();

    // Delete the file
    repo.git(&["rm", "feature.txt"]).unwrap();
    repo.stage_all_and_commit("Delete AI file").unwrap();

    // Recreate the file with same content
    ai_file.set_contents(crate::lines!["line1".ai(), "line2".ai(), "line3".ai()]);
    let recreate_commit = repo.stage_all_and_commit("Recreate AI file").unwrap();

    // Verify pre-rebase: recreated file has attributions
    let pre_log = repo.require_authorship_log(&recreate_commit.commit_sha);
    assert!(
        !pre_log.attestations.is_empty(),
        "precondition: recreated file should have attestations"
    );

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Check rebased tip (the recreate commit)
    let rebased_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let rebased_log = repo.require_authorship_log(&rebased_sha);

    assert!(
        !rebased_log.attestations.is_empty(),
        "regression: file recreated after deletion should still have attestations after rebase"
    );

    // Verify the AI attribution itself survived
    ai_file.assert_lines_and_blame(crate::lines!["line1".ai(), "line2".ai(), "line3".ai()]);
}

/// Regression test: file deleted then recreated with DIFFERENT content preserves attribution.
///
/// This tests a subtle bug where:
/// 1. first_appearance_blobs: seen_files must be cleared on deletion so the
///    new blob OID is read on recreation.
/// 2. files_with_synced_state: must be cleared on deletion so recreation
///    uses content-diff (not stale hunk-based transfer).
#[test]
fn test_rebase_file_delete_recreate_different_content_preserves_attribution() {
    let repo = TestRepo::new();
    let default_branch = repo.current_branch();

    // Initial setup
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Create feature branch with AI file (original content)
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut ai_file = repo.filename("feature.txt");
    ai_file.set_contents(crate::lines!["old_line1".ai(), "old_line2".ai()]);
    repo.stage_all_and_commit("Add AI file").unwrap();

    // Delete the file
    repo.git(&["rm", "feature.txt"]).unwrap();
    repo.stage_all_and_commit("Delete AI file").unwrap();

    // Recreate the file with DIFFERENT content
    ai_file.set_contents(crate::lines![
        "new_line1".ai(),
        "new_line2".ai(),
        "new_line3".ai()
    ]);
    let recreate_commit = repo
        .stage_all_and_commit("Recreate AI file different")
        .unwrap();

    // Verify pre-rebase: recreated file has attributions
    let pre_log = repo.require_authorship_log(&recreate_commit.commit_sha);
    assert!(
        !pre_log.attestations.is_empty(),
        "precondition: recreated file should have attestations"
    );

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Check rebased tip (the recreate commit)
    let rebased_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let rebased_log = repo.require_authorship_log(&rebased_sha);

    assert!(
        !rebased_log.attestations.is_empty(),
        "regression: file recreated with different content should have attestations after rebase"
    );

    // Verify the new AI attribution (different content) survived
    ai_file.assert_lines_and_blame(crate::lines![
        "new_line1".ai(),
        "new_line2".ai(),
        "new_line3".ai()
    ]);
}

/// Regression test: file modified via hunk path, then deleted, then recreated.
///
/// This exercises a bug where `current_file_contents` becomes stale after hunk-based
/// attribution transfer (which updates attributions but not the file content cache).
/// When the file is later deleted and recreated, the slow content-diff path would use
/// stale content with shifted line numbers, producing corrupt attributions.
///
/// Trigger sequence:
/// 1. Commit 1: create file (slow path sets current_file_contents)
/// 2. Commit 2: modify file (hunk path shifts attrs but leaves current_file_contents stale)
/// 3. Commit 3: delete file
/// 4. Commit 4: recreate file with new content
///
/// Main branch must also modify the same file to force the slow reconstruction path.
#[test]
fn test_rebase_file_delete_recreate_after_hunk_modification() {
    let repo = TestRepo::new();
    let default_branch = repo.current_branch();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Feature branch: 4 commits exercising hunk→delete→recreate
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Commit 1: create file
    let mut ai_file = repo.filename("feature.txt");
    ai_file.set_contents(crate::lines!["line1".ai(), "line2".ai(), "line3".ai()]);
    repo.stage_all_and_commit("Create AI file").unwrap();

    // Commit 2: modify file (will use hunk-based path on rebase)
    ai_file.set_contents(crate::lines![
        "line1".ai(),
        "line2".ai(),
        "inserted".ai(),
        "line3".ai()
    ]);
    repo.stage_all_and_commit("Modify AI file").unwrap();

    // Commit 3: delete the file
    repo.git(&["rm", "feature.txt"]).unwrap();
    repo.stage_all_and_commit("Delete AI file").unwrap();

    // Commit 4: recreate with different content
    ai_file.set_contents(crate::lines![
        "recreated_a".ai(),
        "recreated_b".ai(),
        "recreated_c".ai(),
        "recreated_d".ai()
    ]);
    repo.stage_all_and_commit("Recreate AI file").unwrap();

    // Advance default branch — must touch the same file to force slow path
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut conflict_file = repo.filename("feature.txt");
    conflict_file.set_contents(crate::lines!["main_content"]);
    repo.stage_all_and_commit("Main touches same file").unwrap();
    // Delete it so rebase doesn't conflict
    repo.git(&["rm", "feature.txt"]).unwrap();
    repo.stage_all_and_commit("Main deletes file").unwrap();

    // Rebase
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Check the final commit (recreate) has correct attributions
    let rebased_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let rebased_log = repo.require_authorship_log(&rebased_sha);

    assert!(
        !rebased_log.attestations.is_empty(),
        "regression: file recreated after hunk-modify+delete should have attestations"
    );

    ai_file.assert_lines_and_blame(crate::lines![
        "recreated_a".ai(),
        "recreated_b".ai(),
        "recreated_c".ai(),
        "recreated_d".ai()
    ]);
}

/// Regression test for issue #919: daemon panics on multi-byte UTF-8 characters
/// during rebase authorship tracking. The `→` character (U+2192, 3 bytes in UTF-8)
/// placed so that byte index 40 falls inside its encoding triggers a panic in
/// `run_diff_tree_with_hunks` when `&line[..40]` is used instead of `line.get(..40)`.
#[test]
fn test_rebase_preserves_authorship_with_multibyte_utf8_in_diff_context() {
    let repo = TestRepo::new();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Create a file with multi-byte UTF-8 characters (→ is 3 bytes: 0xE2 0x86 0x92).
    // The content is crafted so that a diff context line will contain multi-byte chars
    // near the 40-byte boundary that previously caused the panic.
    let mut utf8_file = repo.filename("rules.py");
    utf8_file.set_contents(crate::lines![
        "def test_rules():".ai(),
        "    \"\"\"98 rules high, 2 rules low → with threshold 90, low should be trimmed.\"\"\""
            .ai(),
        "    pass".ai()
    ]);
    repo.stage_all_and_commit("Add rules with arrow char")
        .unwrap();

    // Second commit modifying the same file to ensure diff hunks include the UTF-8 context
    utf8_file.set_contents(crate::lines![
        "def test_rules():".ai(),
        "    \"\"\"98 rules high, 2 rules low → with threshold 90, low should be trimmed.\"\"\""
            .ai(),
        "    result = run_rules()".ai(),
        "    assert result".ai()
    ]);
    repo.stage_all_and_commit("Expand rules test").unwrap();

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main advance"]);
    repo.stage_all_and_commit("Main advance").unwrap();

    // Rebase — this previously panicked with:
    // byte index 40 is not a char boundary; it is inside '→' (bytes 39..42)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify authorship preserved through the rebase
    utf8_file.assert_lines_and_blame(crate::lines![
        "def test_rules():".ai(),
        "    \"\"\"98 rules high, 2 rules low → with threshold 90, low should be trimmed.\"\"\""
            .ai(),
        "    result = run_rules()".ai(),
        "    assert result".ai()
    ]);
}

crate::reuse_tests_in_worktree!(
    test_rebase_file_delete_recreate_preserves_attribution,
    test_rebase_file_delete_recreate_different_content_preserves_attribution,
    test_rebase_file_delete_recreate_after_hunk_modification,
);
