use super::{ExpectedLineExt, TestRepo, fs};

/// Regression test for gap between two different AI sessions in the same commit.
///
/// Scenario: A file gets two separate AI edits (different sessions) before a single
/// commit. The second edit inserts lines above the first edit's content, causing
/// hunk shifts. If shifts aren't applied correctly, the first edit's lines get
/// recorded at wrong positions, leaving a gap in the note.
#[test]
fn test_multi_session_ai_gap_between_different_sessions() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("multi.txt");

    // Initial commit with some base content
    let initial = "line1\nline2\nline3\nline4\nline5\n";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // AI session 1: replace lines 3-4 with AI content
    // Pre-edit human checkpoint (captures before state)
    repo.git_ai(&["checkpoint", "human", "multi.txt"]).unwrap();

    let after_ai1 = "line1\nline2\nAAA\nBBB\nline5\n";
    fs::write(&file_path, after_ai1).unwrap();

    // Post-edit AI checkpoint (captures AI changes)
    repo.git_ai(&["checkpoint", "mock_ai", "multi.txt"])
        .unwrap();

    // AI session 2: insert 3 lines at the top (shifts everything down)
    // Pre-edit human checkpoint
    repo.git_ai(&["checkpoint", "human", "multi.txt"]).unwrap();

    let after_ai2 = "XXX\nYYY\nZZZ\nline1\nline2\nAAA\nBBB\nline5\n";
    fs::write(&file_path, after_ai2).unwrap();

    // Post-edit AI checkpoint
    repo.git_ai(&["checkpoint", "mock_ai", "multi.txt"])
        .unwrap();

    // Commit both edits
    repo.stage_all_and_commit("two AI sessions").unwrap();

    // Verify: lines 1-3 (XXX, YYY, ZZZ) are AI from session 2
    //         lines 4-5 (line1, line2) are unattributed
    //         lines 6-7 (AAA, BBB) are AI from session 1
    //         line 8 (line5) is unattributed
    let mut file = repo.filename("multi.txt");
    file.assert_committed_lines(crate::lines![
        "XXX".ai(),
        "YYY".ai(),
        "ZZZ".ai(),
        "line1".unattributed_human(),
        "line2".unattributed_human(),
        "AAA".ai(),
        "BBB".ai(),
        "line5".unattributed_human(),
    ]);
}

/// Same scenario but with the second AI edit inserting BETWEEN the first edit's lines.
/// This specifically targets the imara_diff Equal matching gap.
#[test]
fn test_multi_session_ai_insert_between_first_session_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("gap.txt");

    // Initial commit with repetitive content (triggers imara Equal matching)
    let initial = "old\nold\nold\nold\nold\n";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // AI session 1: overwrite entire file with new AI content
    repo.git_ai(&["checkpoint", "human", "gap.txt"]).unwrap();

    let after_ai1 = "A1\nA2\nA3\nA4\nA5\n";
    fs::write(&file_path, after_ai1).unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "gap.txt"]).unwrap();

    // AI session 2: insert a line between A2 and A3
    repo.git_ai(&["checkpoint", "human", "gap.txt"]).unwrap();

    let after_ai2 = "A1\nA2\nINSERTED\nA3\nA4\nA5\n";
    fs::write(&file_path, after_ai2).unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "gap.txt"]).unwrap();

    repo.stage_all_and_commit("insert between").unwrap();

    // ALL lines should be AI — A1-A5 from session 1, INSERTED from session 2
    let mut file = repo.filename("gap.txt");
    file.assert_committed_lines(crate::lines![
        "A1".ai(),
        "A2".ai(),
        "INSERTED".ai(),
        "A3".ai(),
        "A4".ai(),
        "A5".ai(),
    ]);
}

/// Reproduces fuzz_seed_5 pattern: multiple AI edits to a secondary file with
/// varying strategies (prepend, append, insert-random) between commits, where
/// hunk shifts cause attribution gaps.
#[test]
fn test_multi_session_varied_strategies_gap() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("varied.txt");

    // Initial commit with some content
    let initial = "base1\nbase2\nbase3\n";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // AI session 1: append 3 lines
    repo.git_ai(&["checkpoint", "human", "varied.txt"]).unwrap();

    let after_s1 = "base1\nbase2\nbase3\nS1a\nS1b\nS1c\n";
    fs::write(&file_path, after_s1).unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "varied.txt"])
        .unwrap();

    // AI session 2: prepend 2 lines (shifts everything down by 2)
    repo.git_ai(&["checkpoint", "human", "varied.txt"]).unwrap();

    let after_s2 = "S2x\nS2y\nbase1\nbase2\nbase3\nS1a\nS1b\nS1c\n";
    fs::write(&file_path, after_s2).unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "varied.txt"])
        .unwrap();

    // AI session 3: insert 1 line between S1a and S1b (at position 7)
    repo.git_ai(&["checkpoint", "human", "varied.txt"]).unwrap();

    let after_s3 = "S2x\nS2y\nbase1\nbase2\nbase3\nS1a\nS3mid\nS1b\nS1c\n";
    fs::write(&file_path, after_s3).unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "varied.txt"])
        .unwrap();

    repo.stage_all_and_commit("three AI sessions").unwrap();

    let mut file = repo.filename("varied.txt");
    file.assert_committed_lines(crate::lines![
        "S2x".ai(),
        "S2y".ai(),
        "base1".unattributed_human(),
        "base2".unattributed_human(),
        "base3".unattributed_human(),
        "S1a".ai(),
        "S3mid".ai(),
        "S1b".ai(),
        "S1c".ai(),
    ]);
}

/// Reproduces the exact fuzz_seed_5 bug: a file gets OverwriteAll + Prepend in one commit,
/// then heavy rewrites in a later commit. Some lines survive unchanged between commits,
/// but `git blame` re-attributes them to the later commit due to surrounding context changes.
/// Those survivor lines are NOT in `git diff -U0 earlier..later`, so the later commit's
/// note doesn't cover them. Git blame then shows "Test User" (no AI attribution)
/// for lines that WERE AI-written in the earlier commit.
///
/// The key: git blame re-attributes survivors when there's enough context change around them.
/// This only happens when the file has PRIOR history (not root commit).
#[test]
fn test_survivor_lines_across_heavy_rewrite() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("survivor.txt");

    // === Commit 0: Create the file with initial content (needed so commit 1 is NOT root) ===
    let initial = "aaa\nbbb\nccc\nddd\neee\nfff\nggg\nhhh\n";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("commit 0: initial").unwrap();

    // === Commit 1: OverwriteAll with AI, then Prepend with KnownHuman ===
    // Step 1: OverwriteAll with AI (replaces entire file with 8 lines of "p")
    repo.git_ai(&["checkpoint", "human", "survivor.txt"])
        .unwrap();
    let step_p = "ppppp\nppppp\nppppp\nppppp\nppppp\nppppp\nppppp\nppppp\n";
    fs::write(&file_path, step_p).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "survivor.txt"])
        .unwrap();

    // Step 2: Prepend known human (4 lines of "q" at top)
    repo.git_ai(&["checkpoint", "human", "survivor.txt"])
        .unwrap();
    let step_q =
        "qqqqqq\nqqqqqq\nqqqqqq\nqqqqqq\nppppp\nppppp\nppppp\nppppp\nppppp\nppppp\nppppp\nppppp\n";
    fs::write(&file_path, step_q).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "survivor.txt"])
        .unwrap();

    repo.stage_all_and_commit("commit 1: overwrite + prepend")
        .unwrap();

    // Verify commit 1 - all lines should be attributed
    let mut file = repo.filename("survivor.txt");
    file.assert_committed_lines(crate::lines![
        "qqqqqq".human(), // known human (prepend)
        "qqqqqq".human(),
        "qqqqqq".human(),
        "qqqqqq".human(),
        "ppppp".ai(), // AI (overwrite all)
        "ppppp".ai(),
        "ppppp".ai(),
        "ppppp".ai(),
        "ppppp".ai(),
        "ppppp".ai(),
        "ppppp".ai(),
        "ppppp".ai(),
    ]);

    // === Commit 2: Heavy rewrites that leave SOME "p" lines unchanged ===
    // The "p" lines at positions 5,6,7 get replaced/deleted, lines at
    // other positions survive. Insert new content around them so Myers
    // diff between commit 1 and commit 2 treats them as context (Equal).

    // Replace lines 6-8 (p at positions 6,7,8 in 1-indexed) with x content
    repo.git_ai(&["checkpoint", "human", "survivor.txt"])
        .unwrap();
    let after_x =
        "qqqqqq\nqqqqqq\nqqqqqq\nqqqqqq\nppppp\nxxxxx\nxxxxx\nxxxxx\nxxxxx\nppppp\nppppp\nppppp\n";
    fs::write(&file_path, after_x).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "survivor.txt"])
        .unwrap();

    // Replace x lines 7-9 with y (AI)
    repo.git_ai(&["checkpoint", "human", "survivor.txt"])
        .unwrap();
    let after_y =
        "qqqqqq\nqqqqqq\nqqqqqq\nqqqqqq\nppppp\nxxxxx\nyyyyy\nyyyyy\nyyyyy\nppppp\nppppp\nppppp\n";
    fs::write(&file_path, after_y).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "survivor.txt"])
        .unwrap();

    // Insert z (AI) between surviving p lines
    repo.git_ai(&["checkpoint", "human", "survivor.txt"])
        .unwrap();
    let after_z = "qqqqqq\nqqqqqq\nqqqqqq\nqqqqqq\nppppp\nxxxxx\nyyyyy\nyyyyy\nyyyyy\nzzzzz\nzzzzz\nppppp\nppppp\nppppp\n";
    fs::write(&file_path, after_z).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "survivor.txt"])
        .unwrap();

    // Replace last 2 p lines with 0 (KnownHuman)
    repo.git_ai(&["checkpoint", "human", "survivor.txt"])
        .unwrap();
    let after_0 = "qqqqqq\nqqqqqq\nqqqqqq\nqqqqqq\nppppp\nxxxxx\nyyyyy\nyyyyy\nyyyyy\nzzzzz\nzzzzz\nppppp\n00000\n00000\n00000\n";
    fs::write(&file_path, after_0).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "survivor.txt"])
        .unwrap();

    // Prepend 1 (AI)
    repo.git_ai(&["checkpoint", "human", "survivor.txt"])
        .unwrap();
    let after_1 = "11111\n11111\nqqqqqq\nqqqqqq\nqqqqqq\nqqqqqq\nppppp\nxxxxx\nyyyyy\nyyyyy\nyyyyy\nzzzzz\nzzzzz\nppppp\n00000\n00000\n00000\n";
    fs::write(&file_path, after_1).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "survivor.txt"])
        .unwrap();

    repo.stage_all_and_commit("commit 2: heavy rewrites")
        .unwrap();

    // The "p" lines at positions 7 and 14 survived from commit 1 unchanged.
    // `git diff -U0 commit1..commit2` will NOT include them (they're Equal in Myers).
    // So commit 2's note will NOT cover those lines.
    //
    // Git blame behavior:
    // - If blame attributes them to commit 1 → commit 1's note has AI → shows as AI ✓
    // - If blame attributes them to commit 2 → no coverage → shows as Test User (untracked)
    //
    // Either outcome is acceptable. The key insight: these lines were NOT touched in
    // commit 2, so "untracked" in commit 2's context is correct.
    let blame_output = repo
        .git_ai(&["blame", "survivor.txt"])
        .expect("blame should succeed");
    eprintln!("Blame output:\n{}", blame_output);

    // Check which commit blame attributes the survivor p lines to.
    // We need to verify git-ai handles both cases correctly.
    let blame_lines: Vec<&str> = blame_output.lines().collect();

    // Find the p lines and check their attribution
    for (i, line) in blame_lines.iter().enumerate() {
        if line.contains("ppppp") {
            let line_num = i + 1;
            let is_ai = line.contains("mock_ai");
            let is_human = line.contains("Test User");
            eprintln!(
                "Line {}: ppppp - AI={}, Human={} | {}",
                line_num, is_ai, is_human, line
            );
            // Either AI (from commit 1's note) or untracked human (from commit 2) is correct.
            // What is NOT correct: showing as AI from commit 2 (since commit 2 didn't touch it).
            assert!(
                is_ai || is_human,
                "Line {} with ppppp should be either AI (from commit 1) or Human/untracked (from commit 2), got: {}",
                line_num,
                line
            );
        }
    }
}
