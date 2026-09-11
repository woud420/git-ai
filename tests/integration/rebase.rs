use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
#[cfg(not(target_os = "windows"))]
use crate::repos::write_executable_script;
use git_ai::model::authorship_log::PromptRecord;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::model::working_log::AgentId;
use git_ai::operations::git::notes_api::write_note;
use std::collections::HashMap;

fn leading_dropped_commits_before_first_match(range_diff: &str) -> usize {
    let mut dropped = 0;
    for line in range_diff.lines() {
        let mut parts = line.split_whitespace();
        let (_ordinal, _old_sha, Some(status)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        match status {
            "<" => dropped += 1,
            "=" | "!" => break,
            _ => {}
        }
    }
    dropped
}

mod conflict_recovery;

mod historical_attribution;
mod interactive_edits;

mod merge_and_stash;
mod note_metadata;
mod squash_authorship;

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

/// Test simple rebase with no conflicts where trees are identical - multiple commits
#[test]
fn test_rebase_no_conflicts_identical_trees() {
    let repo = TestRepo::new();

    // Create initial commit (on default branch, usually master)
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main line 1", "main line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get the default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with multiple AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines![
        "// AI generated feature 1".ai(),
        "feature line 1".ai()
    ]);
    repo.stage_all_and_commit("AI feature 1").unwrap();

    // Second AI commit
    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines![
        "// AI generated feature 2".ai(),
        "feature line 2".ai()
    ]);
    repo.stage_all_and_commit("AI feature 2").unwrap();

    // Advance default branch (non-conflicting)
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other content"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch (hooks will handle authorship tracking)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify authorship was preserved for both files after rebase
    feature1.assert_lines_and_blame(crate::lines![
        "// AI generated feature 1".ai(),
        "feature line 1".ai()
    ]);
    feature2.assert_lines_and_blame(crate::lines![
        "// AI generated feature 2".ai(),
        "feature line 2".ai()
    ]);
}

/// Test rebase where trees differ (parent changes result in different tree IDs) - multiple commits
#[test]
fn test_rebase_with_different_trees() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with multiple AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines!["// AI added feature 1".ai()]);
    repo.stage_all_and_commit("AI changes 1").unwrap();

    // Second AI commit
    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines!["// AI added feature 2".ai()]);
    repo.stage_all_and_commit("AI changes 2").unwrap();

    // Go back to default branch and add a different file (non-conflicting)
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Main changes").unwrap();

    // Rebase feature onto default branch (no conflicts, but trees will differ - hooks handle authorship)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify authorship was preserved for both files after rebase
    feature1.assert_lines_and_blame(crate::lines!["// AI added feature 1".ai()]);
    feature2.assert_lines_and_blame(crate::lines!["// AI added feature 2".ai()]);
}

/// Test rebase with multiple commits
#[test]
fn test_rebase_multiple_commits() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with multiple commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines!["// AI feature 1".ai()]);
    repo.stage_all_and_commit("AI feature 1").unwrap();

    // Second AI commit
    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines!["// AI feature 2".ai()]);
    repo.stage_all_and_commit("AI feature 2").unwrap();

    // Third AI commit
    let mut feature3 = repo.filename("feature3.txt");
    feature3.set_contents(crate::lines!["// AI feature 3".ai()]);
    repo.stage_all_and_commit("AI feature 3").unwrap();

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main2_file = repo.filename("main2.txt");
    main2_file.set_contents(crate::lines!["more main content"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch (hooks will handle authorship tracking)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify all files have preserved AI authorship after rebase
    feature1.assert_lines_and_blame(crate::lines!["// AI feature 1".ai()]);
    feature2.assert_lines_and_blame(crate::lines!["// AI feature 2".ai()]);
    feature3.assert_lines_and_blame(crate::lines!["// AI feature 3".ai()]);
}

/// Test rebase where only some commits have authorship logs
#[test]
fn test_rebase_mixed_authorship() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Human commit (no AI authorship)
    let mut human_file = repo.filename("human.txt");
    human_file.set_contents(crate::lines!["human work"]);
    repo.stage_all_and_commit("Human work").unwrap();

    // AI commit
    let mut ai_file = repo.filename("ai.txt");
    ai_file.set_contents(crate::lines!["// AI work".ai()]);
    repo.stage_all_and_commit("AI work").unwrap();

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main2_file = repo.filename("main2.txt");
    main2_file.set_contents(crate::lines!["more main"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch (hooks will handle authorship tracking)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify authorship was preserved correctly
    human_file.assert_lines_and_blame(crate::lines!["human work".human()]);
    ai_file.assert_lines_and_blame(crate::lines!["// AI work".ai()]);
}

#[test]
fn test_rebase_preserves_exact_mixed_line_attribution_in_single_file() {
    let repo = TestRepo::new();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut app_file = repo.filename("app.js");
    app_file.set_contents(crate::lines![
        "const version = 1;".human(),
        "function compute() {".ai(),
        "  return 1;".ai(),
        "}".ai()
    ]);
    repo.stage_all_and_commit("Add mixed app").unwrap();

    app_file.insert_at(2, crate::lines!["  // AI docs".ai()]);
    repo.stage_all_and_commit("Add docs").unwrap();

    app_file.insert_at(5, crate::lines!["// AI footer".ai()]);
    repo.stage_all_and_commit("Add footer").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main advance"]);
    repo.stage_all_and_commit("Main advance").unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    app_file.assert_lines_and_blame(crate::lines![
        "const version = 1;".human(),
        "function compute() {".ai(),
        "  // AI docs".ai(),
        "  return 1;".ai(),
        "}".ai(),
        "// AI footer".ai()
    ]);
}

#[test]
fn test_rebase_with_human_only_commit_between_ai_commits_preserves_exact_lines() {
    let repo = TestRepo::new();

    let mut base_file = repo.filename("base.txt");
    let mut app_file = repo.filename("app.js");
    base_file.set_contents(crate::lines!["base"]);
    app_file.set_contents(crate::lines!["const base = 0;".human()]);
    repo.stage_all_and_commit("Initial").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();

    app_file.insert_at(1, crate::lines!["// AI block 1".ai()]);
    repo.stage_all_and_commit("AI block 1").unwrap();

    let mut notes_file = repo.filename("notes.txt");
    notes_file.set_contents(crate::lines!["human notes line"]);
    repo.stage_all_and_commit("Human-only notes").unwrap();

    let mut generated_file = repo.filename("generated.js");
    generated_file.set_contents(crate::lines!["const generated = 42;".ai()]);
    repo.stage_all_and_commit("AI block 2").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main advance"]);
    repo.stage_all_and_commit("Main advance").unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    app_file.assert_lines_and_blame(crate::lines!["const base = 0;".ai(), "// AI block 1".ai()]);
    generated_file.assert_lines_and_blame(crate::lines!["const generated = 42;".ai()]);
    notes_file.assert_lines_and_blame(crate::lines!["human notes line".human()]);
}

crate::reuse_tests_in_worktree!(
    test_rebase_fast_forward,
    test_rebase_with_explicit_branch_argument_preserves_authorship,
    test_rebase_root_with_explicit_branch_argument_preserves_authorship,
    test_rebase_patch_stack,
    test_rebase_already_up_to_date,
    test_rebase_file_delete_recreate_preserves_attribution,
    test_rebase_file_delete_recreate_different_content_preserves_attribution,
    test_rebase_file_delete_recreate_after_hunk_modification,
    test_rebase_no_conflicts_identical_trees,
    test_rebase_with_different_trees,
    test_rebase_multiple_commits,
    test_rebase_mixed_authorship,
    test_rebase_preserves_exact_mixed_line_attribution_in_single_file,
    test_rebase_with_human_only_commit_between_ai_commits_preserves_exact_lines,
);
