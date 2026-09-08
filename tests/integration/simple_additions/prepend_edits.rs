use super::{ExpectedLineExt, TestRepo, fs};

/// Reproduces the fuzz_chaos_99 bug: multiple checkpoints on the same file where a later
/// prepend checkpoint should preserve prior AI/KnownHuman attribution for shifted lines.
#[test]
fn test_multi_checkpoint_prepend_preserves_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    // Step 1: Initial content with KnownHuman
    let content1 = "AAAA\nBBBB\nCCCC\nDDDD\n";
    fs::write(&file_path, content1).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Step 2: Append AI lines
    let content2 = "AAAA\nBBBB\nCCCC\nDDDD\nEEEE\nFFFF\n";
    fs::write(&file_path, content2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Step 3: Prepend AI lines (this should preserve lines 1-6 attribution shifted to 9-14)
    // Pre-edit "human" snapshot
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let content3 =
        "1111\n2222\n3333\n4444\n5555\n6666\n7777\n8888\nAAAA\nBBBB\nCCCC\nDDDD\nEEEE\nFFFF\n";
    fs::write(&file_path, content3).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Commit
    repo.stage_all_and_commit("multi checkpoint test").unwrap();

    // Assert: lines 1-8 are AI (prepended), lines 9-12 are KnownHuman (shifted from original),
    // lines 13-14 are AI (shifted from step 2's append)
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines![
        "1111".ai(),
        "2222".ai(),
        "3333".ai(),
        "4444".ai(),
        "5555".ai(),
        "6666".ai(),
        "7777".ai(),
        "8888".ai(),
        "AAAA".human(), // KnownHuman shifted
        "BBBB".human(), // KnownHuman shifted
        "CCCC".human(), // KnownHuman shifted
        "DDDD".human(), // KnownHuman shifted
        "EEEE".ai(),    // AI shifted
        "FFFF".ai(),    // AI shifted
    ]);
}

/// Reproduces exact fuzz_chaos_99 pattern: 4 rapid edits (KnownHuman append, AI append,
/// KnownHuman ReplaceRandom, AI Prepend) where the final prepend must preserve all 8 lines.
#[test]
fn test_burst_edits_prepend_preserves_all_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    // Start with some base content (simulates file before the burst)
    fs::write(&file_path, "X1\nX2\nX3\nX4\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // Edit 1: KnownHuman Append 4 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(&file_path, "X1\nX2\nX3\nX4\nH1\nH2\nH3\nH4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 2: AI Append 6 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(
        &file_path,
        "X1\nX2\nX3\nX4\nH1\nH2\nH3\nH4\nA1\nA2\nA3\nA4\nA5\nA6\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Edit 3: KnownHuman ReplaceRandom 8 lines (replace lines at positions 1-8)
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(
        &file_path,
        "R1\nR2\nR3\nR4\nR5\nR6\nR7\nR8\nA1\nA2\nA3\nA4\nA5\nA6\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 4: AI Prepend 8 lines - ALL 8 must be AI
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(
        &file_path,
        "P1\nP2\nP3\nP4\nP5\nP6\nP7\nP8\nR1\nR2\nR3\nR4\nR5\nR6\nR7\nR8\nA1\nA2\nA3\nA4\nA5\nA6\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Commit
    repo.stage_all_and_commit("burst commit").unwrap();

    // Assert: ALL 8 prepended lines are AI, R1-R8 are KnownHuman, A1-A6 are AI
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines![
        "P1".ai(),
        "P2".ai(),
        "P3".ai(),
        "P4".ai(),
        "P5".ai(),
        "P6".ai(),
        "P7".ai(),
        "P8".ai(),
        "R1".human(),
        "R2".human(),
        "R3".human(),
        "R4".human(),
        "R5".human(),
        "R6".human(),
        "R7".human(),
        "R8".human(),
        "A1".ai(),
        "A2".ai(),
        "A3".ai(),
        "A4".ai(),
        "A5".ai(),
        "A6".ai(),
    ]);
}

/// Same as above but with single multi-byte Unicode chars per line (like the fuzzer uses).
/// The fuzzer allocates one char per step; when it exhausts ASCII, it uses U+0100+.
#[test]
fn test_burst_edits_prepend_multibyte_chars() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    // Use multi-byte Unicode chars (2-3 bytes each in UTF-8)
    // These simulate what the fuzzer produces at steps 100+
    let base = "\u{0100}\n\u{0101}\n\u{0102}\n\u{0103}\n"; // Ā ā Ă ă
    fs::write(&file_path, base).unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // Edit 1: KnownHuman Append 4 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit1 = "\u{0100}\n\u{0101}\n\u{0102}\n\u{0103}\n\u{0110}\n\u{0111}\n\u{0112}\n\u{0113}\n";
    fs::write(&file_path, edit1).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 2: AI Append 6 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit2 = "\u{0100}\n\u{0101}\n\u{0102}\n\u{0103}\n\u{0110}\n\u{0111}\n\u{0112}\n\u{0113}\n\u{0120}\n\u{0121}\n\u{0122}\n\u{0123}\n\u{0124}\n\u{0125}\n";
    fs::write(&file_path, edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Edit 3: KnownHuman ReplaceRandom 8 lines (replace first 8)
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit3 = "\u{0130}\n\u{0131}\n\u{0132}\n\u{0133}\n\u{0134}\n\u{0135}\n\u{0136}\n\u{0137}\n\u{0120}\n\u{0121}\n\u{0122}\n\u{0123}\n\u{0124}\n\u{0125}\n";
    fs::write(&file_path, edit3).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 4: AI Prepend 8 lines - ALL 8 must be AI
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit4 = "\u{0140}\n\u{0141}\n\u{0142}\n\u{0143}\n\u{0144}\n\u{0145}\n\u{0146}\n\u{0147}\n\u{0130}\n\u{0131}\n\u{0132}\n\u{0133}\n\u{0134}\n\u{0135}\n\u{0136}\n\u{0137}\n\u{0120}\n\u{0121}\n\u{0122}\n\u{0123}\n\u{0124}\n\u{0125}\n";
    fs::write(&file_path, edit4).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Commit
    repo.stage_all_and_commit("burst commit").unwrap();

    // Assert: ALL 8 prepended lines are AI, next 8 are KnownHuman, last 6 are AI
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines![
        "\u{0140}".ai(),
        "\u{0141}".ai(),
        "\u{0142}".ai(),
        "\u{0143}".ai(),
        "\u{0144}".ai(),
        "\u{0145}".ai(),
        "\u{0146}".ai(),
        "\u{0147}".ai(),
        "\u{0130}".human(),
        "\u{0131}".human(),
        "\u{0132}".human(),
        "\u{0133}".human(),
        "\u{0134}".human(),
        "\u{0135}".human(),
        "\u{0136}".human(),
        "\u{0137}".human(),
        "\u{0120}".ai(),
        "\u{0121}".ai(),
        "\u{0122}".ai(),
        "\u{0123}".ai(),
        "\u{0124}".ai(),
        "\u{0125}".ai(),
    ]);
}
