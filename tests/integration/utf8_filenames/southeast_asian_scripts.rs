use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

// =============================================================================
// Phase 4: Southeast Asian Scripts (Thai, Vietnamese, Khmer, Lao)
// =============================================================================

#[test]
fn test_thai_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Thai characters in the filename
    let mut thai_file = repo.filename("ภาษาไทย.txt");
    thai_file.set_contents(crate::lines!["สวัสดี".ai(), "ขอบคุณ".ai(), "ประเทศไทย".ai(),]);

    // Commit the Thai-named file
    let commit = repo.stage_all_and_commit("Add Thai file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "ภาษาไทย.txt",
        "File path should preserve Thai characters"
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
fn test_vietnamese_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Vietnamese characters (with tone marks) in the filename
    let mut vietnamese_file = repo.filename("tiếng_việt.txt");
    vietnamese_file.set_contents(crate::lines![
        "Xin chào".ai(),
        "Cảm ơn".ai(),
        "Việt Nam".ai(),
    ]);

    // Commit the Vietnamese-named file
    let commit = repo.stage_all_and_commit("Add Vietnamese file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "tiếng_việt.txt",
        "File path should preserve Vietnamese tone marks"
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
fn test_khmer_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Khmer (Cambodian) characters in the filename
    let mut khmer_file = repo.filename("ភាសាខ្មែរ.txt");
    khmer_file.set_contents(crate::lines!["សួស្តី".ai(), "អរគុណ".ai(),]);

    // Commit the Khmer-named file
    let commit = repo.stage_all_and_commit("Add Khmer file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "ភាសាខ្មែរ.txt",
        "File path should preserve Khmer characters"
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
fn test_lao_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Lao characters in the filename
    let mut lao_file = repo.filename("ພາສາລາວ.txt");
    lao_file.set_contents(crate::lines!["ສະບາຍດີ".ai(), "ຂອບໃຈ".ai(),]);

    // Commit the Lao-named file
    let commit = repo.stage_all_and_commit("Add Lao file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "ພາສາລາວ.txt",
        "File path should preserve Lao characters"
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
    test_thai_filename,
    test_vietnamese_filename,
    test_khmer_filename,
    test_lao_filename,
);
