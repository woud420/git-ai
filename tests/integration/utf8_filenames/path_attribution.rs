use super::{CommitStats, ExpectedLineExt, TestRepo, extract_json_object};

#[test]
fn test_mixed_ascii_and_utf8_filenames() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates multiple files - one with ASCII name, one with Chinese, one with emoji
    let mut ascii_file = repo.filename("normal_file.txt");
    ascii_file.set_contents(crate::lines!["Normal line 1".ai(), "Normal line 2".ai(),]);

    let mut chinese_file = repo.filename("配置文件.txt");
    chinese_file.set_contents(crate::lines!["设置一".ai(), "设置二".ai(), "设置三".ai(),]);

    let mut emoji_file = repo.filename("🎉celebration.txt");
    emoji_file.set_contents(crate::lines!["Party time!".ai(),]);

    // Commit all files together
    let commit = repo.stage_all_and_commit("Add mixed files").unwrap();

    // Verify the authorship log contains all 3 files
    assert_eq!(
        commit.authorship_log.attestations.len(),
        3,
        "Should have 3 attestations for all files"
    );

    // Verify each file path is correctly stored
    let file_paths: Vec<&str> = commit
        .authorship_log
        .attestations
        .iter()
        .map(|a| a.file_path.as_str())
        .collect();
    assert!(
        file_paths.contains(&"normal_file.txt"),
        "Should contain ASCII filename"
    );
    assert!(
        file_paths.contains(&"配置文件.txt"),
        "Should contain Chinese filename"
    );
    assert!(
        file_paths.contains(&"🎉celebration.txt"),
        "Should contain emoji filename"
    );

    // Get stats and verify AI attribution is correct for all files
    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    // Total: 2 + 3 + 1 = 6 AI lines
    assert_eq!(
        stats.ai_additions, 6,
        "All 6 lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
    assert_eq!(
        stats.ai_accepted, 6,
        "All 6 AI lines should be counted as accepted"
    );
    assert_eq!(
        stats.git_diff_added_lines, 6,
        "Git should report 6 added lines"
    );
}

#[test]
fn test_utf8_content_in_file() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with UTF-8 content (but ASCII filename)
    let mut content_file = repo.filename("content.txt");
    content_file.set_contents(crate::lines![
        "Hello World".ai(),
        "你好世界".ai(),
        "🌍 地球".ai(),
        "مرحبا بالعالم".ai(),
        "Привет мир".ai(),
    ]);

    // Commit the file
    let commit = repo.stage_all_and_commit("Add UTF-8 content").unwrap();

    // Verify the authorship log
    assert_eq!(commit.authorship_log.attestations.len(), 1);

    // Get stats and verify AI attribution is correct
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
    assert_eq!(
        stats.ai_accepted, 5,
        "All 5 AI lines should be counted as accepted"
    );
}

#[test]
fn test_utf8_filename_blame() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with Chinese characters in the filename
    let mut chinese_file = repo.filename("测试文件.rs");
    chinese_file.set_contents(crate::lines![
        "fn main() {".ai(),
        "    println!(\"Hello\");".ai(),
        "}".ai(),
    ]);

    // Commit the Chinese-named file
    repo.stage_all_and_commit("Add test file").unwrap();

    // Verify blame works correctly with the UTF-8 filename
    chinese_file.assert_lines_and_blame(crate::lines![
        "fn main() {".ai(),
        "    println!(\"Hello\");".ai(),
        "}".ai(),
    ]);
}

#[test]
fn test_nested_directory_with_utf8_filename() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file in a nested directory with UTF-8 name
    let mut nested_file = repo.filename("src/模块/组件.ts");
    nested_file.set_contents(crate::lines![
        "export const 组件 = () => {};".ai(),
        "export default 组件;".ai(),
    ]);

    // Commit the file
    let commit = repo.stage_all_and_commit("Add nested UTF-8 file").unwrap();

    // Verify the authorship log contains the correct path
    assert_eq!(commit.authorship_log.attestations.len(), 1);
    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "src/模块/组件.ts",
        "File path should preserve UTF-8 in both directory and file names"
    );

    // Get stats and verify AI attribution
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
fn test_utf8_filename_with_human_and_ai_lines() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create a file with mixed human and AI contributions
    let mut mixed_file = repo.filename("数据.json");
    mixed_file.set_contents(crate::lines![
        "{".human(),
        "  \"name\": \"测试\",".ai(),
        "  \"value\": 123,".ai(),
        "  \"enabled\": true".human(),
        "}".human(),
    ]);

    // Commit the file
    repo.stage_all_and_commit("Add data file").unwrap();

    // Get stats and verify attribution
    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(stats.ai_additions, 2, "2 lines should be attributed to AI");
    assert_eq!(
        stats.ai_accepted, 2,
        "2 AI lines should be counted as accepted"
    );
    assert_eq!(
        stats.human_additions, 3,
        "3 h_<hash>-attested lines from KnownHuman checkpoint on fresh file"
    );
    assert_eq!(
        stats.unknown_additions, 0,
        "No unattested human lines - all human lines now have h_<hash>-attestation"
    );
    assert_eq!(
        stats.git_diff_added_lines, 5,
        "Git should report 5 total added lines"
    );
}

// =============================================================================
// Phase 9: Edge Cases and Stress Tests
// =============================================================================

#[test]
fn test_filename_with_all_unicode_categories() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with characters from many Unicode categories
    // Mix of CJK, Arabic, Cyrillic, Greek, emoji
    let mut mixed_file = repo.filename("Test_中文_🚀_العربية_Русский.txt");
    mixed_file.set_contents(crate::lines![
        "Multi-script filename test".ai(),
        "All Unicode categories should work".ai(),
        "Chinese, Arabic, Cyrillic, emoji combined".ai(),
    ]);

    // Commit the multi-category file
    let commit = repo
        .stage_all_and_commit("Add multi-category file")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "Test_中文_🚀_العربية_Русский.txt",
        "File path should preserve all Unicode categories"
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
fn test_deeply_nested_utf8_directories() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file in deeply nested directories with different scripts
    let mut nested_file = repo.filename("src/日本/中国/한국/भारत/العربية/file.txt");
    nested_file.set_contents(crate::lines![
        "Deeply nested UTF-8 directories".ai(),
        "Japanese > Chinese > Korean > Hindi > Arabic > file".ai(),
    ]);

    // Commit the deeply nested file
    let commit = repo.stage_all_and_commit("Add deeply nested file").unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "src/日本/中国/한국/भारत/العربية/file.txt",
        "File path should preserve all nested UTF-8 directories"
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
fn test_many_utf8_files_in_single_commit() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates multiple files with different UTF-8 names in a single commit
    let mut chinese = repo.filename("中文.txt");
    chinese.set_contents(crate::lines!["Chinese content".ai()]);

    let mut japanese = repo.filename("日本語.txt");
    japanese.set_contents(crate::lines!["Japanese content".ai()]);

    let mut korean = repo.filename("한글.txt");
    korean.set_contents(crate::lines!["Korean content".ai()]);

    let mut arabic = repo.filename("العربية.txt");
    arabic.set_contents(crate::lines!["Arabic content".ai()]);

    let mut russian = repo.filename("Русский.txt");
    russian.set_contents(crate::lines!["Russian content".ai()]);

    let mut emoji = repo.filename("🚀🎉.txt");
    emoji.set_contents(crate::lines!["Emoji content".ai()]);

    // Commit all files together
    let commit = repo.stage_all_and_commit("Add many UTF-8 files").unwrap();

    assert_eq!(
        commit.authorship_log.attestations.len(),
        6,
        "Should have 6 attestations for all UTF-8 files"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 6,
        "All 6 lines should be attributed to AI"
    );
    assert_eq!(
        stats.human_additions, 0,
        "No lines should be attributed to human"
    );
}

#[test]
fn test_filename_only_non_ascii() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a file with only non-ASCII characters (no extension)
    let mut only_nonascii = repo.filename("中文日本語한글");
    only_nonascii.set_contents(crate::lines!["File with only non-ASCII name".ai(),]);

    // Commit the file
    let commit = repo
        .stage_all_and_commit("Add non-ASCII only file")
        .unwrap();

    assert_eq!(
        commit.authorship_log.attestations[0].file_path, "中文日本語한글",
        "File path with only non-ASCII should be preserved"
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

crate::reuse_tests_in_worktree!(
    test_mixed_ascii_and_utf8_filenames,
    test_utf8_content_in_file,
    test_utf8_filename_blame,
    test_nested_directory_with_utf8_filename,
    test_utf8_filename_with_human_and_ai_lines,
    test_filename_with_all_unicode_categories,
    test_deeply_nested_utf8_directories,
    test_many_utf8_files_in_single_commit,
    test_filename_only_non_ascii,
);
