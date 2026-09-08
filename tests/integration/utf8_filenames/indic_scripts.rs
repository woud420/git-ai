use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

// =============================================================================
// Phase 3: Indic Scripts (Hindi, Tamil, Bengali, Telugu, Gujarati)
// =============================================================================

#[test]
fn test_hindi_devanagari_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Hindi/Devanagari characters in the filename
    let mut hindi_file = repo.filename("हिंदी.txt");
    hindi_file.set_contents(crate::lines!["नमस्ते".ai(), "धन्यवाद".ai(), "भारत".ai(),]);

    // Commit the Hindi-named file
    let commit = repo.stage_all_and_commit("Add Hindi file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "हिंदी.txt",
        "File path should preserve Hindi/Devanagari characters"
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
fn test_tamil_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Tamil characters in the filename
    let mut tamil_file = repo.filename("தமிழ்.txt");
    tamil_file.set_contents(crate::lines!["வணக்கம்".ai(), "நன்றி".ai(),]);

    // Commit the Tamil-named file
    let commit = repo.stage_all_and_commit("Add Tamil file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "தமிழ்.txt",
        "File path should preserve Tamil characters"
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
fn test_bengali_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Bengali characters in the filename
    let mut bengali_file = repo.filename("বাংলা.txt");
    bengali_file.set_contents(crate::lines!["নমস্কার".ai(), "ধন্যবাদ".ai(),]);

    // Commit the Bengali-named file
    let commit = repo.stage_all_and_commit("Add Bengali file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "বাংলা.txt",
        "File path should preserve Bengali characters"
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
fn test_telugu_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Telugu characters in the filename
    let mut telugu_file = repo.filename("తెలుగు.txt");
    telugu_file.set_contents(crate::lines!["నమస్కారం".ai(), "ధన్యవాదాలు".ai(),]);

    // Commit the Telugu-named file
    let commit = repo.stage_all_and_commit("Add Telugu file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "తెలుగు.txt",
        "File path should preserve Telugu characters"
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
fn test_gujarati_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Gujarati characters in the filename
    let mut gujarati_file = repo.filename("ગુજરાતી.txt");
    gujarati_file.set_contents(crate::lines!["નમસ્તે".ai(), "આભાર".ai(),]);

    // Commit the Gujarati-named file
    let commit = repo.stage_all_and_commit("Add Gujarati file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "ગુજરાતી.txt",
        "File path should preserve Gujarati characters"
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
fn test_devanagari_combining_chars() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Devanagari combining vowel marks
    // The word "किताब" (kitaab = book) uses combining vowels
    let mut combining_file = repo.filename("किताब.txt");
    combining_file.set_contents(crate::lines!["पुस्तक".ai(), "अध्याय".ai(),]);

    // Commit the file with combining characters
    let commit = repo
        .stage_all_and_commit("Add file with combining chars")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "किताब.txt",
        "File path should preserve Devanagari combining characters"
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
    test_hindi_devanagari_filename,
    test_tamil_filename,
    test_bengali_filename,
    test_telugu_filename,
    test_gujarati_filename,
    test_devanagari_combining_chars,
);
