use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

#[test]
fn test_emoji_filename_ai_attribution() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with emoji in the filename
    let mut emoji_file = repo.filename("🚀rocket_launch.txt");
    emoji_file.set_contents(crate::lines![
        "Launch sequence initiated".ai(),
        "Engines igniting".ai(),
        "Liftoff!".ai(),
        "Mission success".ai(),
    ]);

    // Commit the emoji-named file
    let commit = repo.stage_all_and_commit("Add emoji file").unwrap();

    // Verify the authorship log contains the emoji filename
    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation for the emoji-named file"
    );
    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "🚀rocket_launch.txt",
        "File path should be the actual UTF-8 filename with emoji"
    );

    // Get stats and verify AI attribution is correct
    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    // The key check: ai_additions should NOT be 0
    assert_eq!(
        stats.ai_additions, 4,
        "All 4 lines should be attributed to AI, not human"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
    assert_eq!(
        stats.ai_accepted, 4,
        "All 4 AI lines should be counted as accepted"
    );
    assert_eq!(
        stats.git_diff_added_lines, 4,
        "Git should report 4 added lines"
    );
}

#[test]
fn test_filename_starting_with_emoji() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file that starts with emoji
    let mut emoji_start = repo.filename("🚀_project.txt");
    emoji_start.set_contents(crate::lines!["File starting with emoji".ai(),]);

    // Commit the file
    let commit = repo.stage_all_and_commit("Add emoji-start file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "🚀_project.txt",
        "File path starting with emoji should be preserved"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(stats.ai_additions, 1, "The line should be attributed to AI");
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_filename_ending_with_emoji() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file that ends with emoji
    let mut emoji_end = repo.filename("project_🚀.txt");
    emoji_end.set_contents(crate::lines!["File ending with emoji".ai(),]);

    // Commit the file
    let commit = repo.stage_all_and_commit("Add emoji-end file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "project_🚀.txt",
        "File path ending with emoji should be preserved"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(stats.ai_additions, 1, "The line should be attributed to AI");
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

// =============================================================================
// Phase 6: Extended Emoji (ZWJ, skin tones, flags, keycaps)
// =============================================================================

#[test]
fn test_emoji_with_skin_tone_modifiers() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with emoji skin tone modifier
    // 👋🏽 = 👋 (U+1F44B) + 🏽 (U+1F3FD skin tone modifier)
    let mut emoji_file = repo.filename("👋🏽wave.txt");
    emoji_file.set_contents(crate::lines![
        "Hello with wave!".ai(),
        "Skin tone modifier test".ai(),
    ]);

    // Commit the emoji file with skin tone modifier
    let commit = repo
        .stage_all_and_commit("Add emoji with skin tone")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "👋🏽wave.txt",
        "File path should preserve emoji with skin tone modifier"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 2,
        "Both lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_emoji_zwj_sequences() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with ZWJ (Zero-Width Joiner) emoji sequence
    // 👨‍👩‍👧‍👦 = family emoji (man + ZWJ + woman + ZWJ + girl + ZWJ + boy)
    let mut zwj_file = repo.filename("👨‍👩‍👧‍👦_family.txt");
    zwj_file.set_contents(crate::lines![
        "Family emoji ZWJ sequence test".ai(),
        "Complex unicode handling".ai(),
    ]);

    // Commit the ZWJ emoji file
    let commit = repo.stage_all_and_commit("Add ZWJ emoji file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "👨‍👩‍👧‍👦_family.txt",
        "File path should preserve ZWJ emoji sequences"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 2,
        "Both lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_emoji_flag_sequences() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with flag emoji (regional indicator sequence)
    // 🇺🇸 = U+1F1FA (regional indicator U) + U+1F1F8 (regional indicator S)
    let mut flag_file = repo.filename("🇺🇸_usa.txt");
    flag_file.set_contents(crate::lines![
        "USA flag emoji test".ai(),
        "Regional indicator sequence".ai(),
    ]);

    // Commit the flag emoji file
    let commit = repo.stage_all_and_commit("Add flag emoji file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "🇺🇸_usa.txt",
        "File path should preserve flag emoji (regional indicator sequences)"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 2,
        "Both lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_multiple_complex_emoji_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with multiple complex emoji
    let mut multi_emoji_file = repo.filename("🚀🎉🌟💻🔥_launch.txt");
    multi_emoji_file.set_contents(crate::lines![
        "Multiple emoji test".ai(),
        "Rocket, party, star, laptop, fire".ai(),
        "All 4-byte UTF-8".ai(),
    ]);

    // Commit the multi-emoji file
    let commit = repo.stage_all_and_commit("Add multi-emoji file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "🚀🎉🌟💻🔥_launch.txt",
        "File path should preserve multiple emoji"
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
fn test_emoji_in_directory_names() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file in directories with emoji names
    let mut nested_emoji_file = repo.filename("src/🔧tools/📝notes.txt");
    nested_emoji_file.set_contents(crate::lines![
        "Emoji in directory names".ai(),
        "Tool and note emoji".ai(),
    ]);

    // Commit the file in emoji-named directories
    let commit = repo
        .stage_all_and_commit("Add file in emoji directories")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "src/🔧tools/📝notes.txt",
        "File path should preserve emoji in directory names"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 2,
        "Both lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

crate::reuse_tests_in_worktree!(
    test_emoji_filename_ai_attribution,
    test_filename_starting_with_emoji,
    test_filename_ending_with_emoji,
    test_emoji_with_skin_tone_modifiers,
    test_emoji_zwj_sequences,
    test_emoji_flag_sequences,
    test_multiple_complex_emoji_filename,
    test_emoji_in_directory_names,
);
