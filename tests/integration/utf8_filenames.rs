use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
/// Tests for UTF-8 filename handling with Chinese characters and emojis.
///
/// This tests verifies that files with non-ASCII characters in their filenames
/// are correctly tracked and attributed when git-ai processes commits.
///
/// Issue: Files with Chinese (or other non-ASCII) characters in filenames were
/// incorrectly classified as human-written because git outputs such filenames
/// with octal escape sequences (e.g., `"\344\270\255\346\226\207.txt"` for "中文.txt").
use crate::test_utils::extract_json_object;
use git_ai::operations::authorship::stats::CommitStats;

mod emoji_sequences;

mod normalization;
mod path_attribution;
mod right_to_left;

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
    test_chinese_filename_ai_attribution,
    test_japanese_hiragana_katakana_filename,
    test_japanese_kanji_filename,
    test_korean_hangul_filename,
    test_chinese_traditional_filename,
    test_mixed_cjk_filename,
    test_russian_cyrillic_filename,
    test_ukrainian_cyrillic_filename,
    test_greek_filename,
    test_greek_polytonic_filename,
    test_hindi_devanagari_filename,
    test_tamil_filename,
    test_bengali_filename,
    test_telugu_filename,
    test_gujarati_filename,
    test_devanagari_combining_chars,
    test_thai_filename,
    test_vietnamese_filename,
    test_khmer_filename,
    test_lao_filename,
    test_mathematical_symbols_filename,
    test_currency_symbols_filename,
    test_box_drawing_characters_filename,
    test_dingbats_and_symbols_filename,
);
