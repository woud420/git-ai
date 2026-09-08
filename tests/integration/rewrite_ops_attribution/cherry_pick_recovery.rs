use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_cherry_pick_conflict_ai_rewrite_resolution_is_attributed() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("conflict.csv");

    let base = "id,value\nbase,seed\n";
    fs::write(&file_path, base).unwrap();
    repo.stage_all_and_commit("base").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "source"]).unwrap();
    fs::write(&file_path, format!("{base}feature,source\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.stage_all_and_commit("source line").unwrap();
    let source_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let mut file = repo.filename("conflict.csv");
    file.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "feature,source".ai(),
    ]);

    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(&file_path, format!("{base}main,target\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.stage_all_and_commit("main line").unwrap();
    file.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "main,target".ai(),
    ]);

    assert!(
        repo.git(&["cherry-pick", &source_sha]).is_err(),
        "cherry-pick should conflict"
    );

    repo.git_ai(&["checkpoint", "human", "conflict.csv"])
        .unwrap();
    fs::write(&file_path, format!("{base}main,target\nresolver,rewrite\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.git(&["add", "conflict.csv"]).unwrap();
    repo.git_with_env(
        &["cherry-pick", "--continue"],
        &[("GIT_EDITOR", "true")],
        None,
    )
    .unwrap();

    file.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "main,target".ai(),
        "resolver,rewrite".ai(),
    ]);
}

// =============================================================================
// Category B (human attributed as AI): Cherry-pick conflict + abort
//
// Reproduction of fuzz_combined_0:
// After a cherry-pick that conflicts and is aborted, the commit that follows
// has a note claiming ALL lines as AI, even though some were KnownHuman.
// The note's session range (1-5) doesn't distinguish human from AI lines.
// =============================================================================

/// Exact reproduction of fuzz_combined_0 failure sequence.
///
/// The critical sequence is:
/// 1. Delete-recreate file (8 lines: H=Ai×4, I=Human×1, J=Ai×3)
/// 2. checkpoint-storm (many rapid edits, 22 lines total), commit
/// 3. hard-reset HEAD~1 (back to 8 lines)
/// 4. overwrite-and-rollback: Y=Ai OverwriteAll 2, Z=Human Append 2, commit
/// 5. cherry-pick-conflict: feature branch prepends a=Human×4, main prepends b=Ai×1
///    cherry-pick conflicts, aborts
/// 6. verify: the "main commit" from step 5 has b(line1) + Y,Y,Z,Z
///    note should say line 1 = AI, lines 2-3 = AI (Y), lines 4-5 = Human (Z)
#[test]
fn test_cherry_pick_abort_main_commit_note_accuracy() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Step 1: Initial commit (simulates delete-recreate result)
    fs::write(
        &file_path,
        "HHHH\nHHHH\nHHHH\nHHHH\nIIII\nJJJJ\nJJJJ\nJJJJ\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    // Checkpoint the human line separately
    fs::write(
        &file_path,
        "HHHH\nHHHH\nHHHH\nHHHH\nIIII\nJJJJ\nJJJJ\nJJJJ\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete-recreate commit").unwrap();

    // Step 2: checkpoint-storm with many edits, then commit
    fs::write(
        &file_path,
        "storm1\nstorm2\nstorm3\nHHHH\nHHHH\nHHHH\nHHHH\nIIII\nJJJJ\nJJJJ\nJJJJ\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("storm commit").unwrap();

    // Step 3: hard-reset to the delete-recreate commit
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // Step 4: overwrite-and-rollback: overwrite entire file with AI, then append human
    fs::write(&file_path, "YYYY\nYYYY\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&file_path, "YYYY\nYYYY\nZZZZ\nZZZZ\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("overwrite-and-rollback").unwrap();

    // Step 5: cherry-pick-conflict
    // Create feature branch from HEAD~1 (the delete-recreate state)
    repo.git(&["checkout", "-b", "cp-feature", "HEAD~1"])
        .unwrap();
    // Feature: prepend human lines
    fs::write(
        &file_path,
        "aaaa\naaaa\naaaa\naaaa\nHHHH\nHHHH\nHHHH\nHHHH\nIIII\nJJJJ\nJJJJ\nJJJJ\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature: prepend human").unwrap();
    let feature_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main (overwrite-and-rollback commit)
    repo.git(&["checkout", "-"]).unwrap();
    // Prepend AI line on main to create conflict
    fs::write(&file_path, "bbbb\nYYYY\nYYYY\nZZZZ\nZZZZ\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("main: prepend ai").unwrap();

    // Cherry-pick feature commit — should conflict (both prepend)
    let cp_result = repo.git(&["cherry-pick", &feature_sha]);
    if cp_result.is_err() {
        repo.git(&["cherry-pick", "--abort"]).ok();
    }

    // After abort: file should be in "main: prepend ai" state
    // = bbbb, YYYY, YYYY, ZZZZ, ZZZZ
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "bbbb".ai(),
        "YYYY".ai(),
        "YYYY".ai(),
        "ZZZZ".human(),
        "ZZZZ".human(),
    ]);
}

// =============================================================================
// Category G: Cherry-pick over-attribution
// =============================================================================

/// After cherry-pick, lines from the target branch must NOT be re-attributed
/// by the source commit's note. This models the fuzzer scenario: feature has
/// AI content, main has human content at a different position. Cherry-pick
/// applies cleanly but the note transfer must not claim main's lines as AI.
///
/// The setup ensures a clean cherry-pick: feature adds lines at the END of
/// the file, while main added lines at the BEGINNING. Git applies without conflict.
#[test]
fn test_cherry_pick_does_not_overattribute_target_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit: single shared line
    fs::write(&file_path, "shared\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Main: prepend human lines (non-conflicting position)
    fs::write(&file_path, "human-1\nhuman-2\nshared\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("main: prepend human").unwrap();

    // Feature branch from initial: append AI lines (non-conflicting position)
    repo.git(&["checkout", "-b", "feature", "HEAD~1"]).unwrap();
    fs::write(&file_path, "shared\nai-1\nai-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature: append AI").unwrap();
    let feature_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Back to main, cherry-pick feature
    repo.git(&["checkout", "-"]).unwrap();
    repo.git(&["cherry-pick", &feature_sha]).unwrap();

    // Result: human-1, human-2, shared, ai-1, ai-2
    // human lines must remain human, AI lines must be AI
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "human-1".human(),
        "human-2".human(),
        "shared".unattributed_human(),
        "ai-1".ai(),
        "ai-2".ai(),
    ]);
}

// =============================================================================
// Category E: Cherry-pick --no-commit loses attribution
//
// When cherry-pick is invoked with --no-commit, HEAD doesn't change so the
// daemon doesn't emit a CherryPickComplete event. The cherry-picked content
// gets staged but has no working log entries, so the subsequent commit loses
// attribution for those lines.
// =============================================================================

#[test]
fn test_cherry_pick_no_commit_preserves_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit with base content
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Feature branch: add AI lines
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(&file_path, "base\nai-line-1\nai-line-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature: AI lines").unwrap();
    let feature_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Back to main
    repo.git(&["checkout", "main"]).unwrap();

    // Cherry-pick with --no-commit (stages content without creating commit)
    repo.git(&["cherry-pick", "--no-commit", &feature_sha])
        .unwrap();

    // Now commit (attribution should be preserved from source commit's note)
    repo.commit("cherry-picked content").unwrap();

    // Verify AI lines retain attribution
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "base".human(),
        "ai-line-1".ai(),
        "ai-line-2".ai(),
    ]);
}
