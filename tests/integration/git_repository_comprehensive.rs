//! Comprehensive tests for src/git/repository.rs
//!
//! This test suite covers the core git operations layer including:
//! - Repository initialization and discovery
//! - Git command execution and error handling
//! - HEAD operations and branch management
//! - Commit operations and traversal
//! - Config get/set operations
//! - Pathspec validation and filtering
//! - Rewrite log operations
//! - Error handling and edge cases
//! - Working directory operations
//! - Bare repository support

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::operations::git::repository::{find_repository, find_repository_in_path};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

mod configuration_and_remotes;
mod discovery;
mod history_and_file_queries;

// ============================================================================
// HEAD and Reference Tests
// ============================================================================

#[test]
fn test_head_on_main_branch() {
    let test_repo = TestRepo::new();

    // Create initial commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Initial commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let head = repo.head().unwrap();
    let name = head.name().unwrap();

    // Should be on main or master
    assert!(
        name.contains("main") || name.contains("master"),
        "HEAD should be on main/master branch, got: {}",
        name
    );
}

#[test]
fn test_head_on_feature_branch() {
    let test_repo = TestRepo::new();

    // Create initial commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Initial commit").unwrap();

    // Create and checkout feature branch
    test_repo.git(&["checkout", "-b", "feature"]).unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let head = repo.head().unwrap();
    let shorthand = head.shorthand().unwrap();

    assert_eq!(shorthand, "feature", "HEAD should be on feature branch");
}

#[test]
fn test_head_shorthand_on_unborn_branch_preserves_git_error() {
    let test_repo = TestRepo::new();
    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let error = repo
        .head()
        .unwrap()
        .shorthand()
        .expect_err("an unborn branch has no resolvable abbreviated ref");

    assert!(error.to_string().contains("rev-parse"), "{error}");
}

#[test]
fn test_head_target() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit = test_repo.stage_all_and_commit("Initial commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let head = repo.head().unwrap();
    let target = head.target().unwrap();

    assert_eq!(
        target, commit.commit_sha,
        "HEAD target should match commit SHA"
    );
}

// ============================================================================
// Commit Operations and Traversal Tests
// ============================================================================

#[test]
fn test_find_commit() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit_info = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha.clone());
    assert!(commit.is_ok(), "Should find commit by SHA");

    let commit = commit.unwrap();
    assert_eq!(
        commit.id(),
        commit_info.commit_sha,
        "Commit ID should match"
    );
}

#[test]
fn test_commit_summary() {
    let test_repo = TestRepo::new();

    // Create commit with message
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit_info = test_repo
        .stage_all_and_commit("Test summary message")
        .unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha).unwrap();
    let summary = commit.summary().unwrap();

    assert_eq!(
        summary, "Test summary message",
        "Summary should match commit message"
    );
}

#[test]
fn test_commit_body() {
    let test_repo = TestRepo::new();

    // Create commit with multi-line message
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.git(&["add", "-A"]).unwrap();

    let message = "Summary line\n\nBody line 1\nBody line 2";
    test_repo.git(&["commit", "-m", message]).unwrap();

    let commit_sha = test_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_sha).unwrap();
    let body = commit.body().unwrap();

    assert!(
        body.contains("Body line 1"),
        "Body should contain first body line"
    );
    assert!(
        body.contains("Body line 2"),
        "Body should contain second body line"
    );
}

#[test]
fn test_commit_parent() {
    let test_repo = TestRepo::new();

    // Create two commits
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content1".human()]);
    let first = test_repo.stage_all_and_commit("First commit").unwrap();

    file.set_contents(crate::lines!["content2".human()]);
    let second = test_repo.stage_all_and_commit("Second commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(second.commit_sha).unwrap();
    let parent = commit.parent(0).unwrap();

    assert_eq!(
        parent.id(),
        first.commit_sha,
        "Parent should be first commit"
    );
}

#[test]
fn test_commit_parents_iterator() {
    let test_repo = TestRepo::new();

    // Create commits
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content1".human()]);
    test_repo.stage_all_and_commit("First commit").unwrap();

    file.set_contents(crate::lines!["content2".human()]);
    test_repo.stage_all_and_commit("Second commit").unwrap();

    let commit_sha = test_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_sha).unwrap();
    let parents: Vec<_> = commit.parents().collect();

    assert_eq!(parents.len(), 1, "Should have one parent");
}

#[test]
fn test_commit_parent_count() {
    let test_repo = TestRepo::new();

    // Create initial commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let first = test_repo.stage_all_and_commit("First commit").unwrap();

    // Create second commit
    file.set_contents(crate::lines!["content2".human()]);
    test_repo.stage_all_and_commit("Second commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Initial commit has no parents
    let first_commit = repo.find_commit(first.commit_sha).unwrap();
    assert_eq!(
        first_commit.parent_count().unwrap(),
        0,
        "Initial commit should have no parents"
    );

    // Second commit has one parent
    let head_sha = test_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let second_commit = repo.find_commit(head_sha).unwrap();
    assert_eq!(
        second_commit.parent_count().unwrap(),
        1,
        "Second commit should have one parent"
    );
}

#[test]
fn test_commit_tree() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit_info = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha).unwrap();
    let tree = commit.tree();

    assert!(tree.is_ok(), "Should get tree from commit");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_find_commit_invalid_sha() {
    let test_repo = TestRepo::new();

    // Create a valid repo
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Test").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let result = repo.find_commit("0000000000000000000000000000000000000000".to_string());
    assert!(result.is_err(), "Should error on invalid commit SHA");
}

#[test]
fn test_initial_commit_has_no_parent() {
    let test_repo = TestRepo::new();

    // Create initial commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit = test_repo.stage_all_and_commit("Initial").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit_obj = repo.find_commit(commit.commit_sha).unwrap();

    // Should have no parents
    let parent_result = commit_obj.parent(0);
    assert!(
        parent_result.is_err(),
        "Initial commit should have no parent"
    );
}

#[test]
fn test_commit_with_unicode_message() {
    let test_repo = TestRepo::new();

    // Create commit with unicode message
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.git(&["add", "-A"]).unwrap();
    test_repo
        .git(&["commit", "-m", "Unicode message: 你好世界 🎉"])
        .unwrap();

    let commit_sha = test_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_sha).unwrap();
    let summary = commit.summary().unwrap();

    assert!(
        summary.contains("你好世界"),
        "Summary should contain unicode characters"
    );
}

#[test]
fn test_revparse_single() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Revparse HEAD
    let obj = repo.revparse_single("HEAD");
    assert!(obj.is_ok(), "Should revparse HEAD");
}

#[test]
fn test_revparse_single_with_relative_ref() {
    let test_repo = TestRepo::new();

    // Create two commits
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content1".human()]);
    test_repo.stage_all_and_commit("First commit").unwrap();

    file.set_contents(crate::lines!["content2".human()]);
    test_repo.stage_all_and_commit("Second commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Revparse HEAD~1
    let obj = repo.revparse_single("HEAD~1");
    assert!(obj.is_ok(), "Should revparse HEAD~1");
}

#[test]
fn test_object_peel_to_commit() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let obj = repo.revparse_single("HEAD").unwrap();
    let commit = obj.peel_to_commit();

    assert!(commit.is_ok(), "Should peel object to commit");
}

// ============================================================================
// Tree and Blob Tests
// ============================================================================

#[test]
fn test_tree_get_path() {
    let test_repo = TestRepo::new();

    // Create file and commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit_info = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha).unwrap();
    let tree = commit.tree().unwrap();
    let entry = tree.get_path(Path::new("test.txt"));

    assert!(entry.is_ok(), "Should find file in tree");
}

#[test]
fn test_tree_get_path_nested() {
    let test_repo = TestRepo::new();

    // Create nested file
    fs::create_dir(test_repo.path().join("subdir")).unwrap();
    let mut file = test_repo.filename("subdir/nested.txt");
    file.set_contents(crate::lines!["nested content".human()]);
    let commit_info = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha).unwrap();
    let tree = commit.tree().unwrap();
    let entry = tree.get_path(Path::new("subdir/nested.txt"));

    assert!(entry.is_ok(), "Should find nested file in tree");
}

#[test]
fn test_tree_get_path_nonexistent() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit_info = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha).unwrap();
    let tree = commit.tree().unwrap();
    let entry = tree.get_path(Path::new("nonexistent.txt"));

    assert!(entry.is_err(), "Should not find nonexistent file in tree");
}

#[test]
fn test_find_blob() {
    let test_repo = TestRepo::new();

    // Create file and commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit_info = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha).unwrap();
    let tree = commit.tree().unwrap();
    let entry = tree.get_path(Path::new("test.txt")).unwrap();
    let blob = repo.find_blob(entry.id());

    assert!(blob.is_ok(), "Should find blob");
}

#[test]
fn test_blob_content() {
    let test_repo = TestRepo::new();

    // Create file and commit
    let mut file = test_repo.filename("test.txt");
    let content = "test content line";
    file.set_contents(crate::lines![content.human()]);
    let commit_info = test_repo.stage_all_and_commit("Test commit").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit = repo.find_commit(commit_info.commit_sha).unwrap();
    let tree = commit.tree().unwrap();
    let entry = tree.get_path(Path::new("test.txt")).unwrap();
    let blob = repo.find_blob(entry.id()).unwrap();
    let blob_content = blob.content().unwrap();

    let blob_str = String::from_utf8(blob_content).unwrap();
    assert!(
        blob_str.contains(content),
        "Blob content should match file content"
    );
}

#[test]
fn test_find_blob_with_commit_sha() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit = test_repo.stage_all_and_commit("Test").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Try to find blob using commit SHA (should fail)
    let result = repo.find_blob(commit.commit_sha);
    assert!(
        result.is_err(),
        "Should error when finding blob with commit SHA"
    );
}

#[test]
fn test_find_tree_with_commit_sha() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit = test_repo.stage_all_and_commit("Test").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Try to find tree using commit SHA (should fail)
    let result = repo.find_tree(commit.commit_sha);
    assert!(
        result.is_err(),
        "Should error when finding tree with commit SHA"
    );
}

#[test]
fn test_revparse_invalid_ref() {
    let test_repo = TestRepo::new();

    // Create valid repo
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Test").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let result = repo.revparse_single("invalid-ref-name-12345");
    assert!(result.is_err(), "Should error on invalid ref");
}

#[test]
fn test_tree_clone() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    let commit = test_repo.stage_all_and_commit("Test").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let commit_obj = repo.find_commit(commit.commit_sha).unwrap();
    let tree = commit_obj.tree().unwrap();
    let tree_clone = tree.clone();

    assert_eq!(
        tree.id(),
        tree_clone.id(),
        "Cloned tree should have same ID"
    );
}

crate::reuse_tests_in_worktree!(
    test_head_on_main_branch,
    test_head_on_feature_branch,
    test_head_target,
    test_find_commit,
    test_commit_summary,
    test_commit_body,
    test_commit_parent,
    test_commit_parents_iterator,
    test_commit_parent_count,
    test_commit_tree,
    test_find_commit_invalid_sha,
    test_initial_commit_has_no_parent,
    test_commit_with_unicode_message,
    test_revparse_single,
    test_revparse_single_with_relative_ref,
    test_object_peel_to_commit,
    test_tree_get_path,
    test_tree_get_path_nested,
    test_tree_get_path_nonexistent,
    test_find_blob,
    test_blob_content,
    test_find_blob_with_commit_sha,
    test_find_tree_with_commit_sha,
    test_revparse_invalid_ref,
    test_tree_clone,
);
