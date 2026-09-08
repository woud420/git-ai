use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

// =============================================================================
// Phase 7: Special Unicode Characters (zero-width, math, currency)
// =============================================================================

#[test]
fn test_mathematical_symbols_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with mathematical symbols
    let mut math_file = repo.filename("∑_integral_√.txt");
    math_file.set_contents(crate::lines![
        "Summation: ∑".ai(),
        "Square root: √".ai(),
        "Integral: ∫".ai(),
    ]);

    // Commit the math symbols file
    let commit = repo.stage_all_and_commit("Add math symbols file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "∑_integral_√.txt",
        "File path should preserve mathematical symbols"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 3,
        "All 3 lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_currency_symbols_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with currency symbols
    let mut currency_file = repo.filename("€£¥₹₿_prices.txt");
    currency_file.set_contents(crate::lines![
        "Euro: €100".ai(),
        "Pound: £50".ai(),
        "Yen: ¥1000".ai(),
        "Rupee: ₹500".ai(),
        "Bitcoin: ₿0.01".ai(),
    ]);

    // Commit the currency symbols file
    let commit = repo
        .stage_all_and_commit("Add currency symbols file")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "€£¥₹₿_prices.txt",
        "File path should preserve currency symbols"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 5,
        "All 5 lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_box_drawing_characters_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with box drawing characters
    let mut box_file = repo.filename("┌─┐│└┘_box.txt");
    box_file.set_contents(crate::lines![
        "┌───────┐".ai(),
        "│ Box   │".ai(),
        "└───────┘".ai(),
    ]);

    // Commit the box drawing file
    let commit = repo.stage_all_and_commit("Add box drawing file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "┌─┐│└┘_box.txt",
        "File path should preserve box drawing characters"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 3,
        "All 3 lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_dingbats_and_symbols_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with dingbats and symbols
    let mut symbols_file = repo.filename("✓✗★☆♠♣♥♦.txt");
    symbols_file.set_contents(crate::lines![
        "Check: ✓".ai(),
        "Cross: ✗".ai(),
        "Stars: ★☆".ai(),
        "Cards: ♠♣♥♦".ai(),
    ]);

    // Commit the dingbats file
    let commit = repo.stage_all_and_commit("Add dingbats file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "✓✗★☆♠♣♥♦.txt",
        "File path should preserve dingbats and symbols"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 4,
        "All 4 lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

crate::reuse_tests_in_worktree!(
    test_mathematical_symbols_filename,
    test_currency_symbols_filename,
    test_box_drawing_characters_filename,
    test_dingbats_and_symbols_filename,
);
