use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::attribution_tracker::LineAttribution;
use git_ai::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use git_ai::model::working_log::AgentId;
use git_ai::operations::git::repo_storage::InitialAttributions;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

fn stash_v2_dir(repo: &TestRepo) -> PathBuf {
    repo.path().join(".git").join("ai").join("stashes_v2")
}

fn single_stash_v2_initial(repo: &TestRepo) -> InitialAttributions {
    let stashes = stash_v2_dir(repo);
    let stash_dir = fs::read_dir(&stashes)
        .expect("stashes_v2 dir exists")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("a compact stash dir should exist");
    let initial = fs::read_to_string(stash_dir.join("INITIAL")).expect("stash INITIAL exists");
    serde_json::from_str(&initial).expect("stash INITIAL is valid")
}

fn current_checkpoint_files(repo: &TestRepo) -> BTreeSet<String> {
    repo.current_working_logs()
        .read_all_checkpoints()
        .expect("read current checkpoints")
        .into_iter()
        .flat_map(|checkpoint| checkpoint.entries.into_iter().map(|entry| entry.file))
        .collect()
}

fn joke_lines(file_idx: usize, count: usize) -> Vec<String> {
    (0..count)
        .map(|line_idx| format!("joke file {file_idx} line {line_idx}: boilerplate punchline"))
        .collect()
}

fn lines_to_content(lines: &[String]) -> String {
    let mut content = lines.join("\n");
    content.push('\n');
    content
}

fn test_agent(id: &str) -> AgentId {
    AgentId {
        tool: "test".to_string(),
        id: id.to_string(),
        model: "test-model".to_string(),
    }
}

fn test_prompt(id: &str) -> PromptRecord {
    PromptRecord {
        agent_id: test_agent(id),
        human_author: None,
        messages_url: None,
        total_additions: 0,
        total_deletions: 0,
        accepted_lines: 0,
        overriden_lines: 0,
        custom_attributes: None,
    }
}

mod conflict_resolution;
mod cross_branch_recovery;

mod pathspecs;

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

#[test]
fn test_stash_pop_with_ai_attribution() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI attribution
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["line 1".ai(), "line 2".ai(), "line 3".ai()]);

    // Run checkpoint to track AI attribution
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash", "push", "-m", "test stash"])
        .expect("stash should succeed");

    // Verify file is gone
    assert!(repo.read_file("example.txt").is_none());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Verify file is back
    assert!(repo.read_file("example.txt").is_some());

    // Commit the changes
    let commit = repo
        .stage_all_and_commit("apply stashed changes")
        .expect("commit should succeed");

    // Verify AI attribution is preserved
    example.assert_lines_and_blame(vec!["line 1".ai(), "line 2".ai(), "line 3".ai()]);

    // Check authorship log has AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_apply_with_ai_attribution() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI attribution
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["line 1".ai(), "line 2".ai()]);

    // Run checkpoint to track AI attribution
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Apply (not pop) the stash
    repo.git(&["stash", "apply"])
        .expect("stash apply should succeed");

    // Commit the changes
    let commit = repo
        .stage_all_and_commit("apply stashed changes")
        .expect("commit should succeed");

    // Verify AI attribution is preserved
    example.assert_lines_and_blame(vec!["line 1".ai(), "line 2".ai()]);

    // Check authorship log has AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_multiple_files() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create multiple files with AI attribution
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["file 1 line 1".ai(), "file 1 line 2".ai()]);

    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec!["file 2 line 1".ai(), "file 2 line 2".ai()]);

    let mut file3 = repo.filename("file3.txt");
    file3.set_contents(vec!["file 3 line 1".ai()]);

    // Run checkpoint to track AI attribution
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash all changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Verify files are gone
    assert!(repo.read_file("file1.txt").is_none());
    assert!(repo.read_file("file2.txt").is_none());
    assert!(repo.read_file("file3.txt").is_none());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit all files
    let commit = repo
        .stage_all_and_commit("apply multi-file stash")
        .expect("commit should succeed");

    // Verify all files have AI attribution
    file1.assert_lines_and_blame(vec!["file 1 line 1".ai(), "file 1 line 2".ai()]);
    file2.assert_lines_and_blame(vec!["file 2 line 1".ai(), "file 2 line 2".ai()]);
    file3.assert_lines_and_blame(vec!["file 3 line 1".ai()]);

    // Check authorship log has the files
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
    assert_eq!(
        commit.authorship_log.attestations.len(),
        3,
        "Expected 3 files in authorship log"
    );
}

#[test]
fn test_stash_with_existing_initial_attributions() {
    // Test that stash attributions merge correctly with existing INITIAL attributions
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file and commit it (this will have some attribution)
    repo.human_edit("example.txt", "existing line\n");
    let mut example = repo.filename("example.txt");
    let _first_commit = repo
        .stage_all_and_commit("add example")
        .expect("commit should succeed");

    // Modify the file with AI
    example.set_contents(vec!["existing line".human(), "new AI line".ai()]);

    // Run checkpoint
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Verify file reverted to original
    let content = repo.read_file("example.txt").expect("file should exist");
    assert_eq!(content.lines().count(), 1, "Should have reverted to 1 line");

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit
    let commit = repo
        .stage_all_and_commit("apply stash")
        .expect("commit should succeed");

    // Verify mixed attribution
    example.assert_lines_and_blame(vec!["existing line".human(), "new AI line".ai()]);

    // Should have both human and AI in authorship
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_mixed_human_and_ai() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create file with mixed attribution
    let mut example = repo.filename("example.txt");
    example.set_contents(vec![
        "line 1".human(),
        "line 2".ai(),
        "line 3".human(),
        "line 4".ai(),
    ]);

    // Run checkpoint
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash and pop
    repo.git(&["stash"]).expect("stash should succeed");
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit
    let commit = repo
        .stage_all_and_commit("mixed content")
        .expect("commit should succeed");

    // Verify blame shows mixed attribution
    example.assert_lines_and_blame(vec![
        "line 1".human(),
        "line 2".ai(),
        "line 3".human(),
        "line 4".ai(),
    ]);

    // Authorship log should have AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_apply_named_reference() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create first stash
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["first stash".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash"]).expect("first stash should succeed");

    // Create second stash
    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec!["second stash".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash"]).expect("second stash should succeed");

    // Apply the first stash (stash@{1})
    repo.git(&["stash", "apply", "stash@{1}"])
        .expect("stash apply stash@{1} should succeed");

    // Verify file1 is back
    assert!(repo.read_file("file1.txt").is_some());
    assert!(repo.read_file("file2.txt").is_none());

    // Commit and verify attribution
    let commit = repo
        .stage_all_and_commit("apply first stash")
        .expect("commit should succeed");

    file1.assert_lines_and_blame(vec!["first stash".ai()]);

    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_pop_with_existing_stack_entries() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    let mut first = repo.filename("first.txt");
    first.set_contents(vec!["first stash line".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash", "push", "-m", "first"])
        .expect("first stash should succeed");

    let mut second = repo.filename("second.txt");
    second.set_contents(vec!["second stash line".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash", "push", "-m", "second"])
        .expect("second stash should succeed");

    // Pop when stash stack still has another entry (non-empty -> non-empty on some Git versions).
    repo.git(&["stash", "pop"])
        .expect("first pop should succeed");
    let first_pop_commit = repo
        .stage_all_and_commit("apply top stash entry")
        .expect("commit after first pop should succeed");

    second.assert_lines_and_blame(vec!["second stash line".ai()]);
    assert!(
        !first_pop_commit.authorship_log.metadata.sessions.is_empty(),
        "expected sessions for first pop commit"
    );

    // Pop remaining stash entry and verify attribution still restores correctly.
    repo.git(&["stash", "pop"])
        .expect("second pop should succeed");
    let second_pop_commit = repo
        .stage_all_and_commit("apply remaining stash entry")
        .expect("commit after second pop should succeed");

    first.assert_lines_and_blame(vec!["first stash line".ai()]);
    assert!(
        !second_pop_commit
            .authorship_log
            .metadata
            .sessions
            .is_empty(),
        "expected sessions for second pop commit"
    );
}

#[test]
fn test_stash_pop_default_reference() {
    // Test that stash pop defaults to stash@{0}
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create AI content
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["AI content".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash without explicit reference
    repo.git(&["stash"]).expect("stash should succeed");

    // Pop without explicit reference (should use stash@{0})
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit and verify
    let commit = repo
        .stage_all_and_commit("apply default stash")
        .expect("commit should succeed");

    example.assert_lines_and_blame(vec!["AI content".ai()]);

    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_pop_empty_repo() {
    // Test that stash operations don't crash on edge cases
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Try to pop when there's no stash - should fail gracefully
    let result = repo.git(&["stash", "pop"]);
    assert!(result.is_err(), "Should fail when no stash exists");
}

#[test]
fn test_stash_mixed_staged_and_unstaged() {
    // Test stashing with a mix of staged and unstaged changes
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI content
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["staged line 1".ai(), "staged line 2".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stage these changes
    repo.git(&["add", "example.txt"])
        .expect("should stage example.txt");

    // Now add more unstaged changes
    example.set_contents(vec![
        "staged line 1".ai(),
        "staged line 2".ai(),
        "unstaged line 3".ai(),
        "unstaged line 4".ai(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash both staged and unstaged (git stash by default stashes both)
    repo.git(&["stash", "--include-untracked"])
        .expect("stash should succeed");

    // Verify file is back to original state (doesn't exist)
    assert!(repo.read_file("example.txt").is_none());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit all changes
    let commit = repo
        .stage_all_and_commit("apply mixed stash")
        .expect("commit should succeed");

    // All lines should have AI attribution preserved (both staged and unstaged)
    example.assert_lines_and_blame(vec![
        "staged line 1".ai(),
        "staged line 2".ai(),
        "unstaged line 3".ai(),
        "unstaged line 4".ai(),
    ]);

    // Should have AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

crate::reuse_tests_in_worktree!(
    test_stash_apply_reset_apply_again,
    test_stash_apply_shift_uses_final_commit_tree_after_later_edit,
    test_stash_pop_with_ai_attribution,
    test_stash_apply_with_ai_attribution,
    test_stash_multiple_files,
    test_stash_with_existing_initial_attributions,
    test_stash_mixed_human_and_ai,
    test_stash_apply_named_reference,
    test_stash_pop_with_existing_stack_entries,
    test_stash_pop_default_reference,
    test_stash_pop_empty_repo,
    test_stash_mixed_staged_and_unstaged,
);
