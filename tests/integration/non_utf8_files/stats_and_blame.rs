use super::{
    CommitStats, ExpectedLineExt, TestRepo, extract_json_object, fs, gbk_hello_world,
    gbk_multiline, latin1_bytes,
};

// =============================================================================
// Stats: Ensure stats work with non-UTF-8 files present
// =============================================================================

#[test]
fn test_stats_with_non_utf8_file_only() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("gbk_data.txt");
    fs::write(&file_path, gbk_multiline()).unwrap();
    repo.stage_all_and_commit("Add GBK file").unwrap();

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert!(
        stats.git_diff_added_lines >= 3,
        "Git should count added lines even for non-UTF-8 files, got: {}",
        stats.git_diff_added_lines
    );
}

#[test]
fn test_stats_with_non_utf8_and_utf8_files_mixed() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut normal_file = repo.filename("normal.txt");
    normal_file.set_contents(crate::lines!["line one".ai(), "line two".ai()]);

    let gbk_path = repo.path().join("gbk_data.txt");
    fs::write(&gbk_path, gbk_multiline()).unwrap();

    repo.stage_all_and_commit("Add mixed files").unwrap();

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 2,
        "AI additions from UTF-8 file should be counted correctly"
    );
    assert!(
        stats.git_diff_added_lines >= 5,
        "Git should count added lines from both files, got: {}",
        stats.git_diff_added_lines
    );
}

#[test]
fn test_stats_json_output_valid_with_non_utf8_file() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("latin1_data.txt");
    fs::write(&file_path, latin1_bytes()).unwrap();
    repo.stage_all_and_commit("Add Latin-1 file").unwrap();

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let result: Result<CommitStats, _> = serde_json::from_str(&json);
    assert!(
        result.is_ok(),
        "Stats JSON should be valid even with non-UTF-8 files"
    );
}

// =============================================================================
// Blame: Ensure blame works (or gracefully degrades) with non-UTF-8 files
// =============================================================================

#[test]
fn test_blame_non_utf8_file_does_not_crash() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("gbk_file.txt");
    fs::write(&file_path, gbk_multiline()).unwrap();
    repo.stage_all_and_commit("Add GBK file").unwrap();

    let result = repo.git_ai(&["blame", "gbk_file.txt"]);
    assert!(
        result.is_ok(),
        "Blame on a non-UTF-8 file should not crash, got: {:?}",
        result.err()
    );
}

#[test]
fn test_blame_utf8_file_unaffected_by_non_utf8_neighbor() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut normal_file = repo.filename("normal.rs");
    normal_file.set_contents(crate::lines![
        "fn main() {".ai(),
        "    println!(\"hello\");".ai(),
        "}".ai(),
    ]);

    let gbk_path = repo.path().join("gbk_data.txt");
    fs::write(&gbk_path, gbk_multiline()).unwrap();

    repo.stage_all_and_commit("Add both files").unwrap();

    normal_file.assert_lines_and_blame(crate::lines![
        "fn main() {".ai(),
        "    println!(\"hello\");".ai(),
        "}".ai(),
    ]);
}

// =============================================================================
// Stats on non-UTF-8 file edits
// =============================================================================

#[test]
fn test_stats_after_editing_non_utf8_file() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("gbk_file.txt");
    fs::write(&file_path, gbk_hello_world()).unwrap();
    repo.stage_all_and_commit("Add GBK file").unwrap();

    fs::write(&file_path, gbk_multiline()).unwrap();
    repo.stage_all_and_commit("Edit GBK file").unwrap();

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let result: Result<CommitStats, _> = serde_json::from_str(&json);
    assert!(
        result.is_ok(),
        "Stats should produce valid JSON after editing non-UTF-8 files"
    );
}

crate::reuse_tests_in_worktree!(
    test_stats_with_non_utf8_file_only,
    test_stats_with_non_utf8_and_utf8_files_mixed,
    test_stats_json_output_valid_with_non_utf8_file,
    test_blame_non_utf8_file_does_not_crash,
    test_blame_utf8_file_unaffected_by_non_utf8_neighbor,
    test_stats_after_editing_non_utf8_file,
);
