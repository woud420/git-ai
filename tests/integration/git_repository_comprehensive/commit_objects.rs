use super::{ExpectedLineExt, TestRepo, find_repository};

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
);
