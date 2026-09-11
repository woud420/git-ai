use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_squash_merge_conflict_keep_both_preserves_ai_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("conflict.csv");

    let base = "id,value\nbase,seed\n";
    fs::write(&file_path, base).unwrap();
    repo.stage_all_and_commit("base").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(&file_path, format!("{base}feature,squash\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.stage_all_and_commit("feature line").unwrap();
    let mut file = repo.filename("conflict.csv");
    file.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "feature,squash".ai(),
    ]);

    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(&file_path, format!("{base}main,squash\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.stage_all_and_commit("main line").unwrap();
    file.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "main,squash".ai(),
    ]);

    assert!(
        repo.git(&["merge", "--squash", "feature"]).is_err(),
        "squash merge should conflict"
    );

    repo.git_ai(&["checkpoint", "human", "conflict.csv"])
        .unwrap();
    fs::write(&file_path, format!("{base}main,squash\nfeature,squash\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.git(&["add", "conflict.csv"]).unwrap();
    repo.commit("squash feature").unwrap();

    file.assert_committed_lines(crate::lines![
        "id,value".unattributed_human(),
        "base,seed".unattributed_human(),
        "main,squash".ai(),
        "feature,squash".ai(),
    ]);
}

// =============================================================================
// Category B (AI attributed as human): Multi-squash produces incomplete note
//
// Reproduction of fuzz_destructive_0:
// After squashing 3 commits, the resulting note only covers some lines,
// leaving gaps where AI lines have no attestation (default to human).
// =============================================================================

/// Multi-squash: squash 3 commits with AI content, note must cover all AI lines.
///
/// Models the fuzz_destructive_0 failure:
/// 1. Make 3 commits on a feature branch with AI edits
/// 2. Squash merge them into main
/// 3. The squashed commit's note must attribute ALL AI lines
#[test]
fn test_multi_squash_incomplete_note() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let main_branch = repo.current_branch();

    // Feature branch: 3 commits with AI edits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    fs::write(&file_path, "base\nline-c\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature 1").unwrap();

    fs::write(&file_path, "base\nline-c\nline-d\nline-d\nline-d\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature 2").unwrap();

    // Third commit has a human DeleteAndInsert
    fs::write(&file_path, "base\nline-c\nhuman-e\nhuman-e\nline-d\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature 3").unwrap();

    // Switch to main and squash merge
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["merge", "--squash", "feature"]).unwrap();
    repo.commit("squash all").unwrap();

    // Verify: all lines must have correct attribution
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "base".ai(),
        "line-c".ai(),
        "human-e".human(),
        "human-e".human(),
        "line-d".ai(),
    ]);
}

// =============================================================================
// Category G: Incomplete note ranges after squash/rebase
//
// Reproduction of fuzz_destructive_0:
// After squash merge, the resulting note's line ranges have gaps — some AI
// lines fall outside any attestation range and default to human.
// =============================================================================

/// Squash merge with multiple AI commits: all AI lines must be covered.
#[test]
fn test_squash_merge_incomplete_ranges() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let main_branch = repo.current_branch();

    // Feature branch with multiple AI commits that build on each other
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    fs::write(&file_path, "base\nfeat-1\nfeat-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feat commit 1").unwrap();

    fs::write(&file_path, "base\nfeat-1\nfeat-2\nfeat-3\nfeat-4\nfeat-5\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feat commit 2").unwrap();

    // Insert human lines in the middle
    fs::write(
        &file_path,
        "base\nfeat-1\nhuman-mid\nfeat-2\nfeat-3\nfeat-4\nfeat-5\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feat commit 3 (human insert)").unwrap();

    // Squash merge into main
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["merge", "--squash", "feature"]).unwrap();
    repo.commit("squash merge").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "base".ai(),
        "feat-1".ai(),
        "human-mid".human(),
        "feat-2".ai(),
        "feat-3".ai(),
        "feat-4".ai(),
        "feat-5".ai(),
    ]);
}

// =============================================================================
// Category F: Multi-squash attribution preservation
// =============================================================================

/// After reset --soft HEAD~N + commit (manual squash), AI lines added in
/// intermediate commits must survive. This models the fuzzer's multi-squash
/// pattern: multiple commits with mixed operations including deletions that
/// cause the authorship note's line coverage to be incomplete — some lines
/// only appear in intermediate commits' notes, not the final one.
#[test]
fn test_multi_squash_preserves_intermediate_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Base commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Commit 1: DeleteAndInsert — delete line 1, insert 2 human lines at top
    fs::write(&file_path, "HH1\nHH2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("squash-1: delete-insert human").unwrap();

    // Commit 2: append AI line
    fs::write(&file_path, "HH1\nHH2\nAI-appended\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("squash-2: append AI").unwrap();

    // Commit 3: replace line 1 with different human content
    fs::write(&file_path, "HH-replaced\nHH2\nAI-appended\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("squash-3: replace human").unwrap();

    // Commit 4: prepend 2 AI lines
    fs::write(
        &file_path,
        "AI-pre1\nAI-pre2\nHH-replaced\nHH2\nAI-appended\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("squash-4: prepend AI").unwrap();

    // Squash all 4 into one
    repo.git(&["reset", "--soft", &base]).unwrap();
    repo.commit("squashed").unwrap();

    // Verify attribution
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "AI-pre1".ai(),
        "AI-pre2".ai(),
        "HH-replaced".human(),
        "HH2".human(),
        "AI-appended".ai(),
    ]);
}

/// After squash, a file that was only created in an intermediate commit must
/// still appear in the authorship note with correct attribution.
#[test]
fn test_multi_squash_preserves_secondary_file() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("secondary.txt");

    // Initial commit
    fs::write(&main_path, "main\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Commit 1: edit main
    fs::write(&main_path, "main\nmain-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("edit main").unwrap();

    // Commit 2: create secondary file with mixed attribution
    fs::write(&sec_path, "sec-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    fs::write(&sec_path, "sec-ai\nsec-human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "secondary.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("add secondary").unwrap();

    // Commit 3: edit main again
    fs::write(&main_path, "main\nmain-2\nmain-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("edit main again").unwrap();

    // Squash all into one
    repo.git(&["reset", "--soft", &base]).unwrap();
    repo.commit("squashed").unwrap();

    // Both files must be in the note with correct attribution
    let mut main_file = repo.filename("main.txt");
    main_file.assert_committed_lines(crate::lines!["main".ai(), "main-2".ai(), "main-3".ai(),]);

    let mut sec_file = repo.filename("secondary.txt");
    sec_file.assert_committed_lines(crate::lines!["sec-ai".ai(), "sec-human".human(),]);
}
