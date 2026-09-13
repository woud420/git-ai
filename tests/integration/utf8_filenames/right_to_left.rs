use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

// =============================================================================
// Phase 2: RTL Scripts (Arabic, Hebrew, Persian, Urdu)
// =============================================================================

#[test]
fn test_arabic_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Arabic characters in the filename
    let mut arabic_file = repo.filename("مرحبا.txt");
    arabic_file.set_contents(crate::lines![
        "السلام عليكم".ai(),
        "مرحبا بالعالم".ai(),
        "شكراً".ai(),
    ]);

    // Commit the Arabic-named file
    let commit = repo.stage_all_and_commit("Add Arabic file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation for the Arabic-named file"
    );
    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "مرحبا.txt",
        "File path should preserve Arabic characters"
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
fn test_hebrew_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Hebrew characters in the filename
    let mut hebrew_file = repo.filename("שלום.txt");
    hebrew_file.set_contents(crate::lines!["שלום עולם".ai(), "תודה רבה".ai(),]);

    // Commit the Hebrew-named file
    let commit = repo.stage_all_and_commit("Add Hebrew file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "שלום.txt",
        "File path should preserve Hebrew characters"
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
fn test_persian_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Persian/Farsi characters in the filename
    let mut persian_file = repo.filename("فارسی.txt");
    persian_file.set_contents(crate::lines!["سلام".ai(), "خوش آمدید".ai(), "ممنون".ai(),]);

    // Commit the Persian-named file
    let commit = repo.stage_all_and_commit("Add Persian file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "فارسی.txt",
        "File path should preserve Persian characters"
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
fn test_urdu_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Urdu characters in the filename
    let mut urdu_file = repo.filename("اردو.txt");
    urdu_file.set_contents(crate::lines!["السلام علیکم".ai(), "شکریہ".ai(),]);

    // Commit the Urdu-named file
    let commit = repo.stage_all_and_commit("Add Urdu file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "اردو.txt",
        "File path should preserve Urdu characters"
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
fn test_rtl_with_ltr_mixed_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with mixed RTL (Arabic) and LTR (English) in the filename
    let mut mixed_file = repo.filename("test_مرحبا_file.txt");
    mixed_file.set_contents(crate::lines![
        "Mixed RTL and LTR content".ai(),
        "محتوى مختلط".ai(),
    ]);

    // Commit the mixed RTL/LTR-named file
    let commit = repo.stage_all_and_commit("Add mixed RTL/LTR file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "test_مرحبا_file.txt",
        "File path should preserve mixed RTL/LTR characters"
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
fn test_rtl_directory_path() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file in a directory with Arabic name
    let mut nested_file = repo.filename("src/العربية/ملف.rs");
    nested_file.set_contents(crate::lines![
        "fn main() {".ai(),
        "    println!(\"مرحبا\");".ai(),
        "}".ai(),
    ]);

    // Commit the file in RTL-named directory
    let commit = repo
        .stage_all_and_commit("Add file in Arabic directory")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "src/العربية/ملف.rs",
        "File path should preserve Arabic characters in both directory and file names"
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

crate::reuse_tests_in_worktree!(
    test_arabic_filename,
    test_hebrew_filename,
    test_persian_filename,
    test_urdu_filename,
    test_rtl_with_ltr_mixed_filename,
    test_rtl_directory_path,
);
