use super::{ExpectedLineExt, TestRepo, fs};

// =============================================================================
// Category D: Overbroad AI session range (human lines inside AI range)
//
// Reproduction of fuzz_combined_0:
// When AI and KnownHuman checkpoints both fire before a single commit,
// the resulting note's AI session range covers ALL lines (1-N) instead of
// only the lines from the AI checkpoint. The KnownHuman checkpoint's lines
// are swallowed into the AI range.
// =============================================================================

/// AI checkpoint then KnownHuman checkpoint, single commit.
/// The note must NOT lump human lines into the AI session range.
///
/// Models fuzz_combined_0: note says `s_xxx 1-5` but lines 2-5 are KnownHuman.
#[test]
fn test_overbroad_ai_range_swallows_human_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // AI writes some lines
    fs::write(&file_path, "ai-line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human appends more lines AFTER the AI checkpoint
    fs::write(&file_path, "ai-line\nhuman-1\nhuman-2\nhuman-3\nhuman-4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("mixed").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-line".ai(),
        "human-1".human(),
        "human-2".human(),
        "human-3".human(),
        "human-4".human(),
    ]);
}

/// Inverse order: KnownHuman first, then AI prepends. Both must be tracked.
#[test]
fn test_overbroad_human_first_then_ai_prepend() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Human writes 4 lines
    fs::write(&file_path, "human-a\nhuman-b\nhuman-c\nhuman-d\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    // AI prepends 1 line
    fs::write(&file_path, "ai-top\nhuman-a\nhuman-b\nhuman-c\nhuman-d\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("prepend ai").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-top".ai(),
        "human-a".human(),
        "human-b".human(),
        "human-c".human(),
        "human-d".human(),
    ]);
}

/// AI OverwriteAll then Human Append — models the overwrite-and-rollback pattern.
/// The AI checkpoint covers ALL content initially, then human appends. The note
/// must NOT claim human-appended lines as AI.
///
/// Critical: uses OverwriteAll (deletes all existing content) which is a more
/// aggressive pattern than simple append/prepend.
#[test]
fn test_overbroad_overwrite_all_then_human_append() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit with some content
    fs::write(&file_path, "old-1\nold-2\nold-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // AI overwrites ALL content (OverwriteAll pattern)
    fs::write(&file_path, "ai-new-1\nai-new-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human appends after AI overwrite
    fs::write(&file_path, "ai-new-1\nai-new-2\nhuman-1\nhuman-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("overwrite then append").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-new-1".ai(),
        "ai-new-2".ai(),
        "human-1".human(),
        "human-2".human(),
    ]);
}

/// Exact fuzz_combined_0 pattern: many rapid checkpoints (storm), commit, hard
/// reset back, then AI overwrite + human append. The checkpoint storm creates
/// many working log entries that the hard reset must invalidate.
#[test]
fn test_overbroad_checkpoint_storm_then_reset_then_overwrite_human() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "aaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Checkpoint storm: many rapid edits, then commit
    fs::write(&file_path, "storm-1\nstorm-2\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&file_path, "storm-1\nstorm-2\nstorm-3\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(
        &file_path,
        "storm-1\nstorm-2\nstorm-3\nstorm-4\naaa\nbbb\nccc\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(
        &file_path,
        "storm-1\nstorm-2\nstorm-3\nstorm-4\nstorm-5\naaa\nbbb\nccc\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("storm commit").unwrap();

    // Hard reset back to initial, then checkpoint immediately.
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // AI OverwriteAll
    fs::write(&file_path, "Y-1\nY-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human Append
    fs::write(&file_path, "Y-1\nY-2\nZ-1\nZ-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("post-reset overwrite").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "Y-1".ai(),
        "Y-2".ai(),
        "Z-1".human(),
        "Z-2".human(),
    ]);
}

/// Like above but adds a cherry-pick conflict + abort after the overwrite,
/// matching the exact tail of fuzz_combined_0.
#[test]
fn test_overbroad_storm_reset_overwrite_then_cherry_pick_abort() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "aaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Storm + commit
    fs::write(&file_path, "s1\ns2\ns3\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("storm").unwrap();

    // Hard reset, then checkpoint immediately.
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // OverwriteAll (AI) + Append (Human) + commit
    fs::write(&file_path, "Y-1\nY-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&file_path, "Y-1\nY-2\nZ-1\nZ-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("overwrite-and-rollback").unwrap();

    // Feature branch from initial, prepend human lines
    repo.git(&["checkout", "-b", "cp-feature", "HEAD~1"])
        .unwrap();
    fs::write(&file_path, "human-a\nhuman-b\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature: prepend human").unwrap();
    let feature_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Back to main, prepend AI
    repo.git(&["checkout", "-"]).unwrap();
    fs::write(&file_path, "b-ai\nY-1\nY-2\nZ-1\nZ-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("main: prepend ai").unwrap();

    // Cherry-pick → conflict → abort
    let cp_result = repo.git(&["cherry-pick", &feature_sha]);
    if cp_result.is_err() {
        repo.git(&["cherry-pick", "--abort"]).ok();
    }

    // After abort: main state = b-ai, Y-1, Y-2, Z-1, Z-2
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "b-ai".ai(),
        "Y-1".ai(),
        "Y-2".ai(),
        "Z-1".human(),
        "Z-2".human(),
    ]);
}
