use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

// =============================================================================
// Phase 8: Unicode Normalization (NFC vs NFD)
// =============================================================================

#[test]
fn test_precomposed_nfc_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with precomposed (NFC) characters
    // "café" with precomposed é (U+00E9)
    let mut nfc_file = repo.filename("café.txt");
    nfc_file.set_contents(crate::lines![
        "Precomposed NFC form".ai(),
        "café with é = U+00E9".ai(),
    ]);

    // Commit the NFC file
    let commit = repo.stage_all_and_commit("Add NFC file").unwrap();

    // The file path may be stored as NFC or NFD depending on filesystem
    // We just verify that the attribution works regardless
    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation"
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
fn test_decomposed_nfd_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with decomposed (NFD) characters
    // "café" with decomposed e + combining acute accent (U+0065 + U+0301)
    let mut nfd_file = repo.filename("cafe\u{0301}.txt");
    nfd_file.set_contents(crate::lines![
        "Decomposed NFD form".ai(),
        "cafe with e + combining accent".ai(),
    ]);

    // Commit the NFD file
    let commit = repo.stage_all_and_commit("Add NFD file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation"
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
fn test_combining_diacritical_marks() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with combining diacritical marks
    // "naïve" with ï as i + combining diaeresis (U+0069 + U+0308)
    let mut combining_file = repo.filename("nai\u{0308}ve.txt");
    combining_file.set_contents(crate::lines![
        "Combining diacritical marks".ai(),
        "naïve with combining diaeresis".ai(),
    ]);

    // Commit the file with combining marks
    let commit = repo
        .stage_all_and_commit("Add combining marks file")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation"
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
fn test_swedish_angstrom() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Swedish Å (A with ring above)
    // This is a common normalization test case
    let mut swedish_file = repo.filename("Ångström.txt");
    swedish_file.set_contents(crate::lines!["Swedish Ångström".ai(), "Length unit".ai(),]);

    // Commit the Swedish file
    let commit = repo.stage_all_and_commit("Add Swedish file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation"
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
    test_precomposed_nfc_filename,
    test_decomposed_nfd_filename,
    test_combining_diacritical_marks,
    test_swedish_angstrom,
);
