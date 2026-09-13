use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log::PromptRecord;
use git_ai::model::working_log::AgentId;
use git_ai::operations::git::notes_api::write_note;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn git_common_dir(repo: &TestRepo) -> PathBuf {
    let raw = repo
        .git_og(&["rev-parse", "--git-common-dir"])
        .expect("rev-parse --git-common-dir should succeed");
    let common_dir = PathBuf::from(raw.trim());
    if common_dir.is_absolute() {
        common_dir
    } else {
        repo.path().join(common_dir)
    }
}

mod remote_sources;

/// Test cherry-pick with conflicts and --continue
#[test]
fn test_cherry_pick_with_conflict_and_continue() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch with AI changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.replace_at(1, "AI_FEATURE_VERSION".ai());
    repo.stage_all_and_commit("AI feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and make conflicting change
    repo.git(&["checkout", &main_branch]).unwrap();
    file.replace_at(1, "MAIN_BRANCH_VERSION".human());
    repo.stage_all_and_commit("Human change").unwrap();

    // Try to cherry-pick (should conflict)
    let cherry_pick_result = repo.git(&["cherry-pick", &feature_commit]);
    assert!(cherry_pick_result.is_err(), "Should have conflict");

    // Resolve conflict by choosing the AI version
    use std::fs;
    fs::write(
        repo.path().join("file.txt"),
        "Line 1\nAI_FEATURE_VERSION\nLine 3",
    )
    .unwrap();
    repo.git(&["add", "file.txt"]).unwrap();

    // Continue cherry-pick
    repo.git(&["cherry-pick", "--continue"]).unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "AI_FEATURE_VERSION".ai(),
        "Line 3".human(),
    ]);
}

/// Test cherry-pick --abort
#[test]
fn test_cherry_pick_abort() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let initial_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let main_branch = repo.current_branch();

    // Create feature branch with AI changes (modify line 2)
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.replace_at(1, "AI modification of line 2".ai());
    repo.stage_all_and_commit("AI feature").unwrap();

    // Assert intermediary blame
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "AI modification of line 2".ai(),
    ]);

    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and make conflicting change (also modify line 2)
    repo.git(&["checkout", &main_branch]).unwrap();
    file.replace_at(1, "Human modification of line 2".human());
    repo.stage_all_and_commit("Human change").unwrap();

    // Assert intermediary blame
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "Human modification of line 2".human(),
    ]);

    // Try to cherry-pick (should conflict)
    let cherry_pick_result = repo.git(&["cherry-pick", &feature_commit]);
    assert!(cherry_pick_result.is_err(), "Should have conflict");

    // Abort the cherry-pick
    repo.git(&["cherry-pick", "--abort"]).unwrap();

    // Verify HEAD is back to before the cherry-pick
    let current_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_ne!(current_head, initial_head); // Different because we made the "Human change" commit

    // Verify final file state (should have human's version)
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "Human modification of line 2".human(),
    ]);
}

/// Regression test for #952: Failed cherry-pick with bad args should not corrupt state
/// for subsequent valid cherry-picks.
///
/// Bug: git-ai pre-hook writes a CherryPickStart with empty source_commits when given
/// bad revision arguments.  If that stale event is left in the rewrite log, the next
/// valid cherry-pick may process attribution against the wrong (empty) source list,
/// producing zero AI attributions even for lines that came from an AI session.
#[test]
fn test_cherry_pick_bad_args_dont_corrupt_subsequent_attribution() {
    let repo = TestRepo::new();
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["base line"]);
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    // Create feature branch with 2 AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI line 1".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();
    let sha1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    file.insert_at(2, crate::lines!["AI line 2".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();
    let sha2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    repo.git(&["checkout", &main_branch]).unwrap();

    // Attempt cherry-pick with bad args: two SHAs concatenated into one string, which
    // is not a valid revision.  This must fail and must NOT write a corrupt event to
    // the rewrite log (fixed by skipping CherryPickStart when source_commits is empty).
    let bad_arg = format!("{} {}", sha1, sha2);
    let bad_result = repo.git(&["cherry-pick", &bad_arg]);
    assert!(
        bad_result.is_err(),
        "cherry-pick with invalid revision should fail"
    );
    let _ = repo.git(&["cherry-pick", "--abort"]); // clean up any partial state

    // Cherry-pick sha1 — must produce correct per-line AI attribution despite the
    // prior corrupted attempt.
    repo.git(&["cherry-pick", &sha1]).unwrap();
    // Single-commit cherry-pick: the source commit's note covers all file content,
    // so all lines (including "base line") end up AI-attributed after the copy.
    file.assert_lines_and_blame(crate::lines!["base line".ai(), "AI line 1".ai(),]);

    // Cherry-pick sha2 as well — state must still be clean.
    repo.git(&["cherry-pick", &sha2]).unwrap();
    file.assert_lines_and_blame(crate::lines![
        "base line".ai(),
        "AI line 1".ai(),
        "AI line 2".ai(),
    ]);
}

/// Regression test for #951: cherry-pick --skip should preserve attribution for the
/// remaining commits in the sequence.
///
/// Bug: when a cherry-pick becomes "empty" (its changes are already present) and the
/// user runs `cherry-pick --skip`, git-ai failed to remove the skipped commit from the
/// CherryPickStart source_commits list.  The post-hook then found a mismatch between
/// the number of source commits (3) and the number of new commits actually created (2),
/// and skipped attribution for ALL remaining commits.
#[test]
fn test_cherry_pick_skip_preserves_subsequent_attribution() {
    let repo = TestRepo::new();
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["base line"]);
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    // Feature branch: three AI commits that each append one line.
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI line 1".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();
    let sha1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    file.insert_at(2, crate::lines!["AI line 2".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();
    let sha2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    file.insert_at(3, crate::lines!["AI line 3".ai()]);
    repo.stage_all_and_commit("AI commit 3").unwrap();
    let sha3 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    repo.git(&["checkout", &main_branch]).unwrap();

    // Pre-apply sha1's change as a plain human commit so that cherry-picking sha1
    // results in an empty diff — forcing git to stop and require --skip.
    let mut main_file = repo.filename("file.txt");
    main_file.insert_at(1, crate::lines!["AI line 1"]); // no .ai() — human commit
    repo.stage_all_and_commit("pre-apply sha1 as human")
        .unwrap();

    // Start cherry-picking all three.  sha1 is now empty → git stops with an error.
    let pick_result = repo.git(&["cherry-pick", &sha1, &sha2, &sha3]);
    assert!(
        pick_result.is_err(),
        "cherry-pick of an already-applied commit should require --skip"
    );

    // Skip the empty sha1 commit; git should then apply sha2 and sha3 automatically.
    repo.git(&["cherry-pick", "--skip"]).unwrap();

    // Final file state after the full series:
    //   "base line"  — initial commit, human
    //   "AI line 1"  — pre-applied as a human commit, but sha2's note carries sha1's
    //                  AI attribution from the feature branch, so it ends up AI after
    //                  the cherry-pick of sha2 overwrites the note.
    //   "AI line 2"  — cherry-picked from sha2, AI session
    //   "AI line 3"  — cherry-picked from sha3, AI session
    file.assert_lines_and_blame(crate::lines![
        "base line".human(),
        "AI line 1".ai(),
        "AI line 2".ai(),
        "AI line 3".ai(),
    ]);
}

/// Cherry-pick chain where only commit 1 conflicts (on file_a), requiring a
/// conflict-resolution working log.  Commit 2 (file_b only) applies cleanly and
/// its working log was consumed at commit time on the feature branch, so only
/// commit 1 produces a `flush_pending_note_writes` entry.
///
/// The key falsifiability property: the conflict is resolved with NEW content
/// "RESOLVED_NEW_AI" that was never present in the source commit's tree.  The
/// `CherryPickComplete` shift writes a note for new_commit1 that carries
/// "FEATURE_A_AI" attribution; only the merged conflict-resolution note produced
/// by `flush_pending_note_writes` can carry "RESOLVED_NEW_AI" as AI-attributed.
/// If the flush call is dropped, the blame assertion on file_a fails.
#[test]
fn test_multi_commit_cherry_pick_chain_with_conflict_resolution_working_logs() {
    let repo = TestRepo::new();

    let file_a_path = repo.path().join("file_a.txt");
    let file_b_path = repo.path().join("file_b.txt");

    fs::write(&file_a_path, "base_a\nshared_a\n").unwrap();
    fs::write(&file_b_path, "base_b\nshared_b\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "file_a.txt"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "file_b.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();
    let mut file_a = repo.filename("file_a.txt");
    let mut file_b = repo.filename("file_b.txt");
    file_a.assert_committed_lines(crate::lines!["base_a".human(), "shared_a".human()]);
    file_b.assert_committed_lines(crate::lines!["base_b".human(), "shared_b".human()]);
    let main_branch = repo.current_branch();

    // Feature branch: commit 1 adds AI content to file_a (will conflict).
    // Commit 2 adds AI content to file_b only (applies cleanly; its working log
    // is consumed at commit time and does not contribute a conflict-resolution entry).
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    fs::write(&file_a_path, "base_a\nFEATURE_A_AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_a.txt"])
        .unwrap();
    repo.stage_all_and_commit("AI changes to A").unwrap();
    let feature_commit1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    file_a.assert_committed_lines(crate::lines![
        "base_a".unattributed_human(),
        "FEATURE_A_AI".ai()
    ]);

    fs::write(&file_b_path, "base_b\nFEATURE_B_AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_b.txt"])
        .unwrap();
    repo.stage_all_and_commit("AI changes to B").unwrap();
    let feature_commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    file_b.assert_committed_lines(crate::lines![
        "base_b".unattributed_human(),
        "FEATURE_B_AI".ai()
    ]);

    // Main branch: only file_a has a conflicting change; file_b is untouched.
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.human_edit("file_a.txt", "base_a\nMAIN_A_HUMAN\n");
    repo.stage_all_and_commit("Human change to A").unwrap();
    file_a.assert_committed_lines(crate::lines!["base_a".human(), "MAIN_A_HUMAN".human()]);
    file_b.assert_committed_lines(crate::lines!["base_b".human(), "shared_b".human()]);

    // Only commit 1 conflicts (file_a); commit 2 will apply cleanly afterward.
    let pick_result = repo.git(&["cherry-pick", &feature_commit1, &feature_commit2]);
    assert!(
        pick_result.is_err(),
        "cherry-pick should conflict on file_a"
    );
    repo.sync_daemon();

    // Resolve with entirely new AI content absent from the source commit.
    // Modelling an AI agent: pre-edit human checkpoint, then the AI writes new content.
    // "RESOLVED_NEW_AI" is the observable signal: only the conflict-resolution working
    // log (written here and flushed by `flush_pending_note_writes`) can carry it as
    // AI-attributed.  The shifted note from `CherryPickComplete` has "FEATURE_A_AI",
    // not "RESOLVED_NEW_AI", so if the flush is dropped the blame assertion below fails.
    fs::write(&file_a_path, "base_a\nFEATURE_A_AI\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "file_a.txt"]).unwrap();
    fs::write(&file_a_path, "base_a\nRESOLVED_NEW_AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_a.txt"])
        .unwrap();
    repo.git(&["add", "file_a.txt"]).unwrap();
    // --continue applies commit 1, then immediately applies commit 2 (non-conflicting).
    repo.git(&["cherry-pick", "--continue"]).unwrap();

    repo.sync_daemon();

    // Commit 1 is HEAD~1; commit 2 is HEAD.
    let new_commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let new_commit1 = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    // The note for commit 1 must be the merged conflict-resolution note (from
    // `flush_pending_note_writes`), not the raw shifted note -- evidenced by
    // "RESOLVED_NEW_AI" appearing as AI-attributed in the blame.
    let note1 = repo
        .read_authorship_note(&new_commit1)
        .expect("cherry-picked commit 1 must have an authorship note after batched flush");
    assert!(
        note1.contains("file_a.txt"),
        "commit 1 note should contain file_a.txt attribution: {note1}"
    );

    // Commit 2's note comes from the CherryPickComplete shift (no conflict-resolution
    // working log for this commit).
    let note2 = repo
        .read_authorship_note(&new_commit2)
        .expect("cherry-picked commit 2 must have an authorship note");
    assert!(
        note2.contains("file_b.txt"),
        "commit 2 note should contain file_b.txt attribution: {note2}"
    );

    // "RESOLVED_NEW_AI" is AI-attributed: proves the conflict-resolution working log
    // was flushed and merged into the note.  Dropping the flush in
    // `flush_pending_note_writes` causes this assertion to fail.
    file_a.assert_committed_lines(crate::lines![
        "base_a".unattributed_human(),
        "RESOLVED_NEW_AI".ai()
    ]);
    file_b.assert_committed_lines(crate::lines![
        "base_b".unattributed_human(),
        "FEATURE_B_AI".ai()
    ]);
}

/// Test cherry-picking a single AI-authored commit
#[test]
fn test_single_commit_cherry_pick() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Initial content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get current branch name
    let main_branch = repo.current_branch();

    // Create feature branch with AI-authored changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI feature line".ai()]);
    repo.stage_all_and_commit("Add AI feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and cherry-pick the feature commit
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines![
        "Initial content".ai(),
        "AI feature line".ai(),
    ]);

    // Verify stats
    let stats = repo.stats().unwrap();
    assert_eq!(
        stats.git_diff_added_lines, 2,
        "Should add 1 AI line (+ newline)"
    );
    assert_eq!(stats.ai_additions, 2, "2 AI lines added");
    assert_eq!(stats.ai_accepted, 2, "2 AI lines accepted");
    assert_eq!(stats.human_additions, 0, "0 human lines added");

    // Verify prompt records have correct stats
    let head_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = git_ai::operations::git::notes_api::read_authorship_v3(
        &git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap(),
        &head_commit,
    )
    .unwrap();

    assert!(
        log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "Should have at least one session record"
    );
    for (session_id, session_record) in &log.metadata.sessions {
        assert!(
            !session_record.agent_id.tool.is_empty(),
            "Session {} should have a non-empty tool",
            session_id
        );
        assert!(
            !session_record.agent_id.model.is_empty(),
            "Session {} should have a non-empty model",
            session_id
        );
    }
}

/// Test cherry-picking multiple commits in sequence
#[test]
fn test_multiple_commits_cherry_pick() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1", ""]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch with multiple AI-authored commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    file.insert_at(1, crate::lines!["AI line 2".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();
    let commit1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Second AI commit
    file.insert_at(2, crate::lines!["AI line 3".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();
    let commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Third AI commit
    file.insert_at(3, crate::lines!["AI line 4".ai()]);
    repo.stage_all_and_commit("AI commit 3").unwrap();
    let commit3 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and cherry-pick all three commits
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &commit1, &commit2, &commit3])
        .unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "AI line 2".ai(),
        "AI line 3".ai(),
        "AI line 4".ai(),
    ]);

    // Verify stats for the last cherry-picked commit
    let stats = repo.stats().unwrap();
    eprintln!("Stats: {:?}", stats);
    // Last commit inserts "AI line 4" - git_diff_added_lines only counts this commit's changes
    // ai_additions is capped by git_diff_added_lines, so it reflects this commit only
    assert_eq!(stats.git_diff_added_lines, 1, "Should have added 1 lines");
    assert_eq!(stats.ai_additions, 1, "At least 1 AI line in this commit");
    assert_eq!(stats.ai_accepted, 1, "1 AI lines accepted in commit");

    // Verify session records exist
    let head_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = git_ai::operations::git::notes_api::read_authorship_v3(
        &git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap(),
        &head_commit,
    )
    .unwrap();

    assert!(
        log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "Should have session records"
    );
    for (session_id, session_record) in &log.metadata.sessions {
        assert!(
            !session_record.agent_id.tool.is_empty(),
            "Session {} should have a non-empty tool",
            session_id
        );
        assert!(
            !session_record.agent_id.model.is_empty(),
            "Session {} should have a non-empty model",
            session_id
        );
    }
}

/// Test cherry-picking from branch without AI authorship
#[test]
fn test_cherry_pick_no_ai_authorship() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();
    // Create feature branch with human-only changes (no AI)
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["Human line 2".human()]);
    repo.stage_all_and_commit("Human feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and cherry-pick
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify final file state - should have no AI authorship
    file.assert_lines_and_blame(crate::lines!["Line 1".human(), "Human line 2".human(),]);
}

/// Test that trees-identical fast path works
#[test]
fn test_cherry_pick_identical_trees() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch with AI changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI line".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Add another commit on feature (just to have a parent)
    file.insert_at(2, crate::lines!["More AI".ai()]);
    repo.stage_all_and_commit("More AI").unwrap();

    // Cherry-pick the first feature commit to main
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines!["Line 1".ai(), "AI line".ai(),]);
}

/// Test cherry-pick where some commits become empty (already applied)
#[test]
fn test_cherry_pick_empty_commits() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["Feature line".ai()]);
    repo.stage_all_and_commit("Add feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Manually apply the same change to main
    repo.git(&["checkout", &main_branch]).unwrap();

    // Get a fresh TestFile after branch switch - it will auto-populate from the existing file
    let mut file_on_main = repo.filename("file.txt");
    file_on_main.insert_at(1, crate::lines!["Feature line".human()]);
    repo.stage_all_and_commit("Apply feature manually").unwrap();

    // Try to cherry-pick the feature commit (should become empty or conflict)
    let result = repo.git(&["cherry-pick", &feature_commit]);

    // Git might succeed and skip the empty commit, or it might create a conflict
    // The key is that it shouldn't crash
    match result {
        Ok(_) => {
            // Empty commit was skipped successfully
        }
        Err(_) => {
            // Git reported an error (conflict or empty commit)
            // Abort the cherry-pick to clean up
            let _ = repo.git(&["cherry-pick", "--abort"]);
        }
    }

    // Verify final file state - content should be preserved
    let actual_content = repo.read_file("file.txt").unwrap();
    assert_eq!(
        actual_content.trim(),
        "Line 1\nFeature line",
        "File content should be preserved after cherry-pick/abort"
    );
}

#[test]
fn test_cherry_pick_no_commit_defers_to_final_commit_tree() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("file.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(&file_path, "base\nAI picked line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file.txt"]).unwrap();
    repo.stage_all_and_commit("ai source").unwrap();
    let source_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", "--no-commit", &source_commit])
        .unwrap();

    fs::write(&file_path, "base\nAI picked line\nlate untracked line\n").unwrap();
    repo.git(&["add", "file.txt"]).unwrap();
    repo.commit("commit no-commit cherry-pick with later edit")
        .unwrap();

    let mut file = repo.filename("file.txt");
    file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "AI picked line".ai(),
        "late untracked line".unattributed_human(),
    ]);
}

#[test]
fn test_cherry_pick_preserves_human_only_commit_note_metadata() {
    let repo = TestRepo::new();

    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["human-only change"]);
    let source_commit = repo
        .stage_all_and_commit("human-only commit")
        .expect("create source commit");

    let source_log = repo.require_authorship_log(&source_commit.commit_sha);
    assert!(source_log.metadata.prompts.is_empty());
    assert!(source_log.metadata.sessions.is_empty());

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &source_commit.commit_sha])
        .unwrap();
    let new_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let new_log = repo.require_authorship_log(&new_commit);
    assert!(new_log.metadata.prompts.is_empty());
    assert!(new_log.metadata.sessions.is_empty());
    assert_eq!(new_log.metadata.base_commit_sha, new_commit);
}

#[test]
fn test_cherry_pick_preserves_prompt_only_commit_note_metadata() {
    let repo = TestRepo::new();

    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["human-only change"]);
    let source_commit = repo
        .stage_all_and_commit("human-only commit")
        .expect("create source commit");

    let mut source_log = repo.require_authorship_log(&source_commit.commit_sha);
    assert!(
        source_log.metadata.prompts.is_empty(),
        "precondition: source commit should not have AI prompts before test mutation"
    );

    let mut test_attrs = HashMap::new();
    test_attrs.insert("employee_id".to_string(), "E456".to_string());
    test_attrs.insert("team".to_string(), "backend".to_string());
    test_attrs.insert("device_id".to_string(), "MAC-002".to_string());

    source_log.metadata.prompts.insert(
        "prompt-only-session".to_string(),
        PromptRecord {
            agent_id: AgentId {
                tool: "mock_ai".to_string(),
                id: "session-1".to_string(),
                model: "test-model".to_string(),
            },
            human_author: Some("Test User <test@example.com>".to_string()),
            total_additions: 11,
            total_deletions: 2,
            accepted_lines: 0,
            overriden_lines: 0,
            custom_attributes: Some(test_attrs.clone()),
            messages_url: None,
        },
    );

    let mutated_source_note = source_log
        .serialize_to_string()
        .expect("serialize mutated source note");
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(
        &git_ai_repo,
        &source_commit.commit_sha,
        &mutated_source_note,
    )
    .expect("overwrite source note with prompt-only metadata");

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &source_commit.commit_sha])
        .unwrap();
    let new_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let new_log = repo.require_authorship_log(&new_commit);
    assert_eq!(new_log.metadata.prompts.len(), 1);
    assert_eq!(new_log.metadata.base_commit_sha, new_commit);

    let prompt = new_log
        .metadata
        .prompts
        .get("prompt-only-session")
        .expect("prompt metadata should be preserved");
    assert_eq!(prompt.agent_id.tool, "mock_ai");
    assert_eq!(prompt.agent_id.id, "session-1");
    assert_eq!(prompt.agent_id.model, "test-model");
    assert_eq!(prompt.total_additions, 11);
    assert_eq!(prompt.total_deletions, 2);
    assert_eq!(
        prompt.custom_attributes,
        Some(test_attrs),
        "custom_attributes should be preserved through cherry-pick"
    );
}

/// Test cherry-pick preserving multiple AI sessions from different commits
#[test]
fn test_cherry_pick_multiple_ai_sessions() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("main.rs");
    file.set_contents(crate::lines!["fn main() {}"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI session adds logging
    file.replace_at(0, "fn main() {".human());
    file.insert_at(1, crate::lines!["    println!(\"Starting\");".ai()]);
    file.insert_at(2, crate::lines!["}".human()]);
    repo.stage_all_and_commit("Add logging").unwrap();
    let commit1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Second AI session adds error handling
    file.insert_at(2, crate::lines!["    // TODO: Add error handling".ai()]);
    repo.stage_all_and_commit("Add error handling").unwrap();
    let commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Cherry-pick both to main
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &commit1, &commit2]).unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines![
        "fn main() {".ai(),
        "    println!(\"Starting\");".ai(),
        "    // TODO: Add error handling".ai(),
        "}".human(),
    ]);

    // Verify stats for the last cherry-picked commit
    let stats = repo.stats().unwrap();
    assert_eq!(stats.git_diff_added_lines, 1, "Last commit adds 1 line");
    assert_eq!(stats.ai_additions, 1, "1 AI line in last commit");
    assert_eq!(stats.ai_accepted, 1, "1 AI lines accepted");

    // Verify session records exist
    let head_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = git_ai::operations::git::notes_api::read_authorship_v3(
        &git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap(),
        &head_commit,
    )
    .unwrap();

    assert!(
        log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "Should have at least one session record"
    );
    for (session_id, session_record) in &log.metadata.sessions {
        assert!(
            !session_record.agent_id.tool.is_empty(),
            "Session {} should have a non-empty tool",
            session_id
        );
        assert!(
            !session_record.agent_id.model.is_empty(),
            "Session {} should have a non-empty model",
            session_id
        );
    }
}

/// Test that custom attributes set via config are preserved through a cherry-pick
/// when the real post-commit pipeline injects them.
#[test]
fn test_cherry_pick_preserves_custom_attributes_from_config() {
    let mut repo =
        TestRepo::new_with_daemon_scope(crate::repos::test_repo::DaemonTestScope::Dedicated);

    // Configure custom attributes via config patch
    let mut attrs = HashMap::new();
    attrs.insert("employee_id".to_string(), "E101".to_string());
    attrs.insert("team".to_string(), "frontend".to_string());
    attrs.insert("device_id".to_string(), "LNX-007".to_string());
    repo.patch_git_ai_config(|patch| {
        patch.custom_attributes = Some(attrs.clone());
    });

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Initial content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // Create feature branch with AI-authored changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI feature line".ai()]);
    repo.stage_all_and_commit("Add AI feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Verify custom attributes were set on the original commit
    let original_log = repo.require_authorship_log(&feature_commit);
    assert!(
        original_log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !original_log.metadata.sessions.is_empty(),
        "precondition: original commit should have session records"
    );
    for session in original_log.metadata.sessions.values() {
        assert_eq!(
            session.custom_attributes.as_ref(),
            Some(&attrs),
            "precondition: original commit should have custom_attributes from config"
        );
    }

    // Switch back to main and cherry-pick the feature commit
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify custom attributes survived the cherry-pick
    let new_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let new_log = repo.require_authorship_log(&new_commit);
    assert!(
        new_log.metadata.prompts.is_empty(),
        "cherry-picked commit should not have prompts"
    );
    assert!(
        !new_log.metadata.sessions.is_empty(),
        "cherry-picked commit should have session records"
    );
    for session in new_log.metadata.sessions.values() {
        assert_eq!(
            session.custom_attributes.as_ref(),
            Some(&attrs),
            "custom_attributes should be preserved through cherry-pick"
        );
    }

    // Also verify the AI attribution itself survived
    file.assert_lines_and_blame(crate::lines![
        "Initial content".ai(),
        "AI feature line".ai()
    ]);
}

crate::reuse_tests_in_worktree!(
    test_cherry_pick_with_conflict_and_continue,
    test_cherry_pick_abort,
    test_cherry_pick_bad_args_dont_corrupt_subsequent_attribution,
    test_cherry_pick_skip_preserves_subsequent_attribution,
    test_multi_commit_cherry_pick_chain_with_conflict_resolution_working_logs,
    test_single_commit_cherry_pick,
    test_multiple_commits_cherry_pick,
    test_cherry_pick_no_ai_authorship,
    test_cherry_pick_identical_trees,
    test_cherry_pick_empty_commits,
    test_cherry_pick_no_commit_defers_to_final_commit_tree,
    test_cherry_pick_preserves_human_only_commit_note_metadata,
    test_cherry_pick_preserves_prompt_only_commit_note_metadata,
    test_cherry_pick_multiple_ai_sessions,
);
