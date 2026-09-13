use super::{ExpectedLineExt, HashSet, TestRepo, find_repository};

// ============================================================================
// Commit Range Tests
// ============================================================================

#[test]
fn test_commit_range_length() {
    let test_repo = TestRepo::new();

    // Create commits
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["line1".human()]);
    let first = test_repo.stage_all_and_commit("First").unwrap();

    file.set_contents(crate::lines!["line1".human(), "line2".human()]);
    test_repo.stage_all_and_commit("Second").unwrap();

    file.set_contents(crate::lines![
        "line1".human(),
        "line2".human(),
        "line3".human()
    ]);
    let third = test_repo.stage_all_and_commit("Third").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Create commit range
    let range = git_ai::operations::git::repository::CommitRange::new_infer_refname(
        &repo,
        first.commit_sha.clone(),
        third.commit_sha.clone(),
        Some("HEAD".to_string()),
    )
    .unwrap();

    let length = range.all_commits().len();
    assert_eq!(
        length, 2,
        "Range should contain 2 commits (second and third)"
    );
}

// ============================================================================
// Merge Base Tests
// ============================================================================

#[test]
fn test_merge_base_linear_history() {
    let test_repo = TestRepo::new();

    // Create linear history
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["line1".human()]);
    let first = test_repo.stage_all_and_commit("First").unwrap();

    file.set_contents(crate::lines!["line1".human(), "line2".human()]);
    let second = test_repo.stage_all_and_commit("Second").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let merge_base = repo.merge_base(first.commit_sha.clone(), second.commit_sha);
    assert!(merge_base.is_ok(), "Should find merge base");

    let base = merge_base.unwrap();
    assert_eq!(base, first.commit_sha, "Merge base should be first commit");
}

#[test]
fn test_merge_base_with_branches() {
    let test_repo = TestRepo::new();

    // Create initial commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["line1".human()]);
    let base = test_repo.stage_all_and_commit("Base").unwrap();

    // Capture the original branch name before creating feature branch
    let original_branch = test_repo.current_branch();

    // Create branch
    test_repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(crate::lines!["line1".human(), "feature".human()]);
    let feature = test_repo.stage_all_and_commit("Feature").unwrap();

    // Go back to original branch and make different commit
    test_repo.git(&["checkout", &original_branch]).unwrap();
    file.set_contents(crate::lines!["line1".human(), "main".human()]);
    let main = test_repo.stage_all_and_commit("Main").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let merge_base = repo.merge_base(feature.commit_sha, main.commit_sha);
    assert!(merge_base.is_ok(), "Should find merge base");

    let merge_base_sha = merge_base.unwrap();
    assert_eq!(
        merge_base_sha, base.commit_sha,
        "Merge base should be base commit"
    );
}

// ============================================================================
// File Content Tests
// ============================================================================

#[test]
fn test_get_file_content() {
    let test_repo = TestRepo::new();

    // Create file and commit
    let mut file = test_repo.filename("test.txt");
    let content = "test file content";
    file.set_contents(crate::lines![content.human()]);
    let commit = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let file_content = repo.get_file_content("test.txt", &commit.commit_sha);
    assert!(file_content.is_ok(), "Should get file content");

    let content_bytes = file_content.unwrap();
    let content_str = String::from_utf8(content_bytes).unwrap();
    assert!(content_str.contains(content), "Content should match");
}

#[test]
fn test_get_file_content_nonexistent() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let result = repo.get_file_content("nonexistent.txt", &commit.commit_sha);
    assert!(result.is_err(), "Should error on nonexistent file");
}

#[test]
fn test_list_commit_files() {
    let test_repo = TestRepo::new();

    // Create multiple files and commit
    let mut file1 = test_repo.filename("file1.txt");
    let mut file2 = test_repo.filename("file2.txt");
    file1.set_contents(crate::lines!["content1".human()]);
    file2.set_contents(crate::lines!["content2".human()]);
    let commit = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let files = repo.list_commit_files(&commit.commit_sha, None);
    assert!(files.is_ok(), "Should list commit files");

    let files = files.unwrap();
    assert!(files.contains("file1.txt"), "Should contain file1.txt");
    assert!(files.contains("file2.txt"), "Should contain file2.txt");
}

#[test]
fn test_list_commit_files_with_pathspec() {
    let test_repo = TestRepo::new();

    // Create multiple files and commit
    let mut file1 = test_repo.filename("file1.txt");
    let mut file2 = test_repo.filename("file2.txt");
    file1.set_contents(crate::lines!["content1".human()]);
    file2.set_contents(crate::lines!["content2".human()]);
    let commit = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Filter to only file1.txt
    let mut pathspec = HashSet::new();
    pathspec.insert("file1.txt".to_string());

    let files = repo.list_commit_files(&commit.commit_sha, Some(&pathspec));
    assert!(files.is_ok(), "Should list filtered commit files");

    let files = files.unwrap();
    assert!(files.contains("file1.txt"), "Should contain file1.txt");
    assert!(!files.contains("file2.txt"), "Should not contain file2.txt");
}

#[test]
fn test_diff_changed_files() {
    let test_repo = TestRepo::new();

    // Create initial commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["line1".human()]);
    let first = test_repo.stage_all_and_commit("First").unwrap();

    // Modify file
    file.set_contents(crate::lines!["line1".human(), "line2".human()]);
    let second = test_repo.stage_all_and_commit("Second").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let changed = repo.diff_changed_files(&first.commit_sha, &second.commit_sha);
    assert!(changed.is_ok(), "Should get changed files");

    let files = changed.unwrap();
    assert!(
        files.contains(&"test.txt".to_string()),
        "Should contain changed file"
    );
}

#[test]
fn test_multiple_files_in_single_commit() {
    let test_repo = TestRepo::new();

    // Create multiple files
    let mut file1 = test_repo.filename("file1.txt");
    let mut file2 = test_repo.filename("file2.txt");
    let mut file3 = test_repo.filename("file3.txt");

    file1.set_contents(crate::lines!["content1".human()]);
    file2.set_contents(crate::lines!["content2".human()]);
    file3.set_contents(crate::lines!["content3".human()]);

    let commit = test_repo.stage_all_and_commit("Multiple files").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let files = repo.list_commit_files(&commit.commit_sha, None).unwrap();

    assert_eq!(files.len(), 3, "Should have 3 files in commit");
    assert!(files.contains("file1.txt"), "Should contain file1.txt");
    assert!(files.contains("file2.txt"), "Should contain file2.txt");
    assert!(files.contains("file3.txt"), "Should contain file3.txt");
}

crate::reuse_tests_in_worktree!(
    test_commit_range_length,
    test_merge_base_linear_history,
    test_merge_base_with_branches,
    test_get_file_content,
    test_get_file_content_nonexistent,
    test_list_commit_files,
    test_list_commit_files_with_pathspec,
    test_diff_changed_files,
    test_multiple_files_in_single_commit,
);
