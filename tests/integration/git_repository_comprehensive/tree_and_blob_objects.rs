use super::{ExpectedLineExt, Path, TestRepo, find_repository, fs};

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
