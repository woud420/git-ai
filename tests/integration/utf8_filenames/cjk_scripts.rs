use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

#[test]
fn test_chinese_filename_ai_attribution() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Chinese characters in the filename
    let mut chinese_file = repo.filename("中文文件.txt");
    chinese_file.set_contents(crate::lines!["第一行".ai(), "第二行".ai(), "第三行".ai(),]);

    // Commit the Chinese-named file
    let commit = repo.stage_all_and_commit("Add Chinese file").unwrap();

    // Verify the authorship log contains the Chinese filename
    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation for the Chinese-named file"
    );
    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "中文文件.txt",
        "File path should be the actual UTF-8 filename"
    );

    // Get stats and verify AI attribution is correct
    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    // The key check: ai_additions should NOT be 0
    assert_eq!(
        stats.ai_additions, 3,
        "All 3 lines should be attributed to AI, not human"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
    assert_eq!(
        stats.ai_accepted, 3,
        "All 3 AI lines should be counted as accepted"
    );
    assert_eq!(
        stats.git_diff_added_lines, 3,
        "Git should report 3 added lines"
    );
}

// =============================================================================
// Phase 1: CJK Extended Coverage (Japanese, Korean, Traditional Chinese)
// =============================================================================

#[test]
fn test_japanese_hiragana_katakana_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Japanese Hiragana and Katakana in the filename
    let mut japanese_file = repo.filename("ひらがな_カタカナ.txt");
    japanese_file.set_contents(crate::lines![
        "こんにちは".ai(),
        "コンニチハ".ai(),
        "Hello in Japanese".ai(),
    ]);

    // Commit the Japanese-named file
    let commit = repo
        .stage_all_and_commit("Add Japanese hiragana/katakana file")
        .unwrap();

    // Verify the authorship log contains the Japanese filename
    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have 1 attestation for the Japanese-named file"
    );
    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "ひらがな_カタカナ.txt",
        "File path should be the actual UTF-8 filename with Hiragana and Katakana"
    );

    // Get stats and verify AI attribution is correct
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
fn test_japanese_kanji_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Japanese Kanji in the filename
    let mut kanji_file = repo.filename("漢字ファイル.rs");
    kanji_file.set_contents(crate::lines![
        "fn main() {".ai(),
        "    println!(\"日本語\");".ai(),
        "}".ai(),
    ]);

    // Commit the Kanji-named file
    let commit = repo
        .stage_all_and_commit("Add Japanese kanji file")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "漢字ファイル.rs",
        "File path should preserve Japanese Kanji characters"
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
fn test_korean_hangul_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Korean Hangul in the filename
    let mut korean_file = repo.filename("한글파일.txt");
    korean_file.set_contents(crate::lines!["안녕하세요".ai(), "감사합니다".ai(),]);

    // Commit the Korean-named file
    let commit = repo.stage_all_and_commit("Add Korean hangul file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "한글파일.txt",
        "File path should preserve Korean Hangul characters"
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
fn test_chinese_traditional_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Traditional Chinese in the filename
    let mut traditional_file = repo.filename("繁體中文.txt");
    traditional_file.set_contents(crate::lines!["傳統字體".ai(), "正體中文".ai(), "臺灣".ai(),]);

    // Commit the Traditional Chinese-named file
    let commit = repo
        .stage_all_and_commit("Add Traditional Chinese file")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "繁體中文.txt",
        "File path should preserve Traditional Chinese characters"
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
fn test_mixed_cjk_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with mixed CJK (Chinese, Japanese, Korean) in the filename
    let mut mixed_cjk_file = repo.filename("日本語_中文_한글.txt");
    mixed_cjk_file.set_contents(crate::lines![
        "Japanese: 日本".ai(),
        "Chinese: 中国".ai(),
        "Korean: 한국".ai(),
        "Mixed CJK content".ai(),
    ]);

    // Commit the mixed CJK-named file
    let commit = repo.stage_all_and_commit("Add mixed CJK file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "日本語_中文_한글.txt",
        "File path should preserve mixed CJK characters"
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
    test_chinese_filename_ai_attribution,
    test_japanese_hiragana_katakana_filename,
    test_japanese_kanji_filename,
    test_korean_hangul_filename,
    test_chinese_traditional_filename,
    test_mixed_cjk_filename,
);
