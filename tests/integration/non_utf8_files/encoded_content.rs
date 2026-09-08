use super::{
    CommitStats, TestRepo, extract_json_object, fs, gbk_hello_world, gbk_multiline, latin1_bytes,
    mixed_valid_invalid_utf8, shift_jis_bytes,
};

// =============================================================================
// Core: Commit flow with non-UTF-8 files
// =============================================================================

#[test]
fn test_commit_gbk_encoded_file_succeeds() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("gbk_file.txt");
    fs::write(&file_path, gbk_hello_world()).unwrap();

    let result = repo.stage_all_and_commit("Add GBK file");
    assert!(
        result.is_ok(),
        "Committing a GBK-encoded file should not fail, got: {:?}",
        result.err()
    );
}

#[test]
fn test_commit_latin1_encoded_file_succeeds() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("latin1_file.txt");
    fs::write(&file_path, latin1_bytes()).unwrap();

    let result = repo.stage_all_and_commit("Add Latin-1 file");
    assert!(
        result.is_ok(),
        "Committing a Latin-1 encoded file should not fail, got: {:?}",
        result.err()
    );
}

#[test]
fn test_commit_shift_jis_encoded_file_succeeds() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("shiftjis_file.txt");
    fs::write(&file_path, shift_jis_bytes()).unwrap();

    let result = repo.stage_all_and_commit("Add Shift-JIS file");
    assert!(
        result.is_ok(),
        "Committing a Shift-JIS encoded file should not fail, got: {:?}",
        result.err()
    );
}

#[test]
fn test_commit_mixed_valid_invalid_utf8_file_succeeds() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("mixed_encoding.txt");
    fs::write(&file_path, mixed_valid_invalid_utf8()).unwrap();

    let result = repo.stage_all_and_commit("Add mixed encoding file");
    assert!(
        result.is_ok(),
        "Committing a file with mixed valid/invalid UTF-8 should not fail, got: {:?}",
        result.err()
    );
}

// =============================================================================
// Edit flow: Modifying non-UTF-8 files across commits
// =============================================================================

#[test]
fn test_edit_non_utf8_file_second_commit() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("gbk_file.txt");
    fs::write(&file_path, gbk_hello_world()).unwrap();
    repo.stage_all_and_commit("Add GBK file").unwrap();

    fs::write(&file_path, gbk_multiline()).unwrap();
    let result = repo.stage_all_and_commit("Edit GBK file");
    assert!(
        result.is_ok(),
        "Editing a non-UTF-8 file should not fail, got: {:?}",
        result.err()
    );
}

#[test]
fn test_delete_non_utf8_file() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("gbk_file.txt");
    fs::write(&file_path, gbk_hello_world()).unwrap();
    repo.stage_all_and_commit("Add GBK file").unwrap();

    fs::remove_file(&file_path).unwrap();
    let result = repo.stage_all_and_commit("Delete GBK file");
    assert!(
        result.is_ok(),
        "Deleting a non-UTF-8 file should not fail, got: {:?}",
        result.err()
    );
}

// =============================================================================
// Multiple non-UTF-8 files in a single commit
// =============================================================================

#[test]
fn test_multiple_non_utf8_encodings_in_one_commit() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let gbk_path = repo.path().join("gbk_file.txt");
    fs::write(&gbk_path, gbk_multiline()).unwrap();

    let latin1_path = repo.path().join("latin1_file.txt");
    fs::write(&latin1_path, latin1_bytes()).unwrap();

    let sjis_path = repo.path().join("shiftjis_file.txt");
    fs::write(&sjis_path, shift_jis_bytes()).unwrap();

    let result = repo.stage_all_and_commit("Add files with various encodings");
    assert!(
        result.is_ok(),
        "Committing multiple non-UTF-8 files should not fail, got: {:?}",
        result.err()
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();
    assert!(
        stats.git_diff_added_lines >= 5,
        "Should count lines across all files, got: {}",
        stats.git_diff_added_lines
    );
}

// =============================================================================
// Non-UTF-8 file in subdirectory
// =============================================================================

#[test]
fn test_non_utf8_file_in_subdirectory() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let subdir = repo.path().join("src").join("legacy");
    fs::create_dir_all(&subdir).unwrap();
    let file_path = subdir.join("data.txt");
    fs::write(&file_path, gbk_multiline()).unwrap();

    let result = repo.stage_all_and_commit("Add non-UTF-8 file in subdirectory");
    assert!(
        result.is_ok(),
        "Committing non-UTF-8 file in subdirectory should not fail, got: {:?}",
        result.err()
    );
}

// =============================================================================
// Edge case: File that starts as UTF-8 and becomes non-UTF-8
// =============================================================================

#[test]
fn test_file_changes_from_utf8_to_non_utf8() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("data.txt");
    fs::write(&file_path, "Hello UTF-8 world\n").unwrap();
    repo.stage_all_and_commit("Add UTF-8 file").unwrap();

    fs::write(&file_path, gbk_multiline()).unwrap();
    let result = repo.stage_all_and_commit("Replace with GBK content");
    assert!(
        result.is_ok(),
        "Changing a file from UTF-8 to non-UTF-8 should not fail, got: {:?}",
        result.err()
    );
}

#[test]
fn test_file_changes_from_non_utf8_to_utf8() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("data.txt");
    fs::write(&file_path, gbk_multiline()).unwrap();
    repo.stage_all_and_commit("Add GBK file").unwrap();

    fs::write(&file_path, "Now this is UTF-8\n").unwrap();
    let result = repo.stage_all_and_commit("Replace with UTF-8 content");
    assert!(
        result.is_ok(),
        "Changing a file from non-UTF-8 to UTF-8 should not fail, got: {:?}",
        result.err()
    );
}

// =============================================================================
// Large non-UTF-8 file
// =============================================================================

#[test]
fn test_large_non_utf8_file() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("large_gbk.txt");
    let mut content = Vec::new();
    for i in 0..500 {
        // Each line: GBK bytes + line number + newline
        content.extend_from_slice(&[0xC4, 0xE3, 0xBA, 0xC3]);
        content.extend_from_slice(format!(" line {}", i).as_bytes());
        content.push(b'\n');
    }
    fs::write(&file_path, content).unwrap();

    let result = repo.stage_all_and_commit("Add large GBK file");
    assert!(
        result.is_ok(),
        "Committing a large non-UTF-8 file should not fail, got: {:?}",
        result.err()
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();
    assert!(
        stats.git_diff_added_lines >= 500,
        "Should count all 500 lines, got: {}",
        stats.git_diff_added_lines
    );
}

// =============================================================================
// Non-UTF-8 content with null bytes (edge case between binary and text)
// =============================================================================

#[test]
fn test_file_with_null_bytes_in_content() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("nulls.dat");
    let content: Vec<u8> = vec![
        b'h', b'e', b'l', b'l', b'o', 0x00, b'w', b'o', b'r', b'l', b'd', b'\n',
    ];
    fs::write(&file_path, content).unwrap();

    let result = repo.stage_all_and_commit("Add file with null bytes");
    assert!(
        result.is_ok(),
        "Committing a file with null bytes should not fail, got: {:?}",
        result.err()
    );
}

crate::reuse_tests_in_worktree!(
    test_commit_gbk_encoded_file_succeeds,
    test_commit_latin1_encoded_file_succeeds,
    test_commit_shift_jis_encoded_file_succeeds,
    test_commit_mixed_valid_invalid_utf8_file_succeeds,
    test_edit_non_utf8_file_second_commit,
    test_delete_non_utf8_file,
    test_multiple_non_utf8_encodings_in_one_commit,
    test_non_utf8_file_in_subdirectory,
    test_file_changes_from_utf8_to_non_utf8,
    test_file_changes_from_non_utf8_to_utf8,
    test_large_non_utf8_file,
    test_file_with_null_bytes_in_content,
);
