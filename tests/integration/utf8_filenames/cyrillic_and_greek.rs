use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

// =============================================================================
// Phase 5: Cyrillic and Greek Scripts
// =============================================================================

#[test]
fn test_russian_cyrillic_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Russian Cyrillic characters in the filename
    let mut russian_file = repo.filename("Русский.txt");
    russian_file.set_contents(crate::lines![
        "Привет мир".ai(),
        "Спасибо".ai(),
        "Россия".ai(),
    ]);

    // Commit the Russian-named file
    let commit = repo.stage_all_and_commit("Add Russian file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "Русский.txt",
        "File path should preserve Russian Cyrillic characters"
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
fn test_ukrainian_cyrillic_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Ukrainian Cyrillic characters in the filename
    // Ukrainian has unique letters like ї, і, є, ґ
    let mut ukrainian_file = repo.filename("Українська.txt");
    ukrainian_file.set_contents(crate::lines!["Привіт".ai(), "Дякую".ai(), "Україна".ai(),]);

    // Commit the Ukrainian-named file
    let commit = repo.stage_all_and_commit("Add Ukrainian file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "Українська.txt",
        "File path should preserve Ukrainian Cyrillic characters"
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
fn test_greek_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Greek characters in the filename
    let mut greek_file = repo.filename("Ελληνικά.txt");
    greek_file.set_contents(crate::lines![
        "Γειά σου".ai(),
        "Ευχαριστώ".ai(),
        "Ελλάδα".ai(),
    ]);

    // Commit the Greek-named file
    let commit = repo.stage_all_and_commit("Add Greek file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "Ελληνικά.txt",
        "File path should preserve Greek characters"
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
fn test_greek_polytonic_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Greek polytonic (with diacritics) characters in the filename
    let mut polytonic_file = repo.filename("Ἑλληνική.txt");
    polytonic_file.set_contents(crate::lines!["Ἀθῆναι".ai(), "φιλοσοφία".ai(),]);

    // Commit the Greek polytonic-named file
    let commit = repo
        .stage_all_and_commit("Add Greek polytonic file")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "Ἑλληνική.txt",
        "File path should preserve Greek polytonic characters with diacritics"
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
    test_russian_cyrillic_filename,
    test_ukrainian_cyrillic_filename,
    test_greek_filename,
    test_greek_polytonic_filename,
);
