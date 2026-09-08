use super::{ExpectedLineExt, TestRepo, fs};

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

crate::reuse_tests_in_worktree!(
    test_cherry_pick_with_conflict_and_continue,
    test_cherry_pick_abort,
    test_cherry_pick_bad_args_dont_corrupt_subsequent_attribution,
    test_cherry_pick_skip_preserves_subsequent_attribution,
    test_multi_commit_cherry_pick_chain_with_conflict_resolution_working_logs,
);
