use super::{ExpectedLineExt, Path, TestRepo, find_repository, find_repository_in_path, fs};

// ============================================================================
// Repository Discovery and Initialization Tests
// ============================================================================

#[test]
fn test_find_repository_in_valid_repo() {
    let repo = TestRepo::new();

    // Create a commit to ensure it's a valid repo
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Should successfully find repository
    let found_repo =
        find_repository(&["-C".to_string(), repo.path().to_str().unwrap().to_string()]);

    assert!(found_repo.is_ok(), "Should find valid repository");
}

#[test]
fn test_find_repository_in_subdirectory() {
    let repo = TestRepo::new();

    // Create subdirectory
    let subdir = repo.path().join("subdir");
    fs::create_dir(&subdir).unwrap();

    // Should find repository from subdirectory
    let found_repo = find_repository(&["-C".to_string(), subdir.to_str().unwrap().to_string()]);

    assert!(
        found_repo.is_ok(),
        "Should find repository from subdirectory"
    );
}

#[test]
fn test_find_repository_in_nested_subdirectory() {
    let repo = TestRepo::new();

    // Create nested subdirectories
    let nested = repo.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).unwrap();

    // Should find repository from deeply nested subdirectory
    let found_repo = find_repository(&["-C".to_string(), nested.to_str().unwrap().to_string()]);

    assert!(
        found_repo.is_ok(),
        "Should find repository from nested subdirectory"
    );
}

#[test]
fn test_find_repository_for_bare_repo() {
    let bare_repo = TestRepo::new_bare();

    let found_repo = find_repository(&[
        "-C".to_string(),
        bare_repo.path().to_str().unwrap().to_string(),
    ]);

    assert!(found_repo.is_ok(), "Should find bare repository");

    let repo = found_repo.unwrap();
    assert!(
        repo.is_bare_repository().unwrap(),
        "Should detect bare repository"
    );
}

#[test]
fn test_repository_path_methods() {
    let test_repo = TestRepo::new();
    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // path() should always point at a valid git directory.
    let git_path = repo.path();
    assert!(git_path.is_dir(), "path() should return a git directory");
    if git_path == repo.common_dir() {
        assert!(
            git_path.ends_with(".git"),
            "non-worktree path() should return .git directory"
        );
    } else {
        assert!(
            git_path.to_string_lossy().contains("/worktrees/")
                || git_path
                    .components()
                    .any(|c| c.as_os_str() == std::ffi::OsStr::new("worktrees")),
            "worktree path() should resolve to a linked worktree git dir"
        );
    }

    // Test workdir() returns repository root (use canonical paths for macOS /var vs /private/var)
    let workdir = repo.workdir().unwrap();
    let canonical_workdir = workdir.canonicalize().unwrap();
    let canonical_test_path = test_repo.path().canonicalize().unwrap();
    assert_eq!(
        canonical_workdir, canonical_test_path,
        "workdir() should return repository root"
    );
}

#[test]
fn test_path_is_in_workdir() {
    let test_repo = TestRepo::new();
    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Path inside workdir - create the file so it can be canonicalized
    let inside = test_repo.path().join("file.txt");
    fs::write(&inside, "test content").unwrap();
    assert!(
        repo.path_is_in_workdir(&inside),
        "File in workdir should return true"
    );

    // Path outside workdir
    let outside = Path::new("/tmp/outside.txt");
    assert!(
        !repo.path_is_in_workdir(outside),
        "File outside workdir should return false"
    );

    // Path inside a nested subrepo (has its own .git/ directory) should return false
    let nested_repo_dir = test_repo.path().join("nested-repo");
    fs::create_dir_all(nested_repo_dir.join("src")).unwrap();
    // Initialize a real git repo in the nested directory
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&nested_repo_dir)
        .output()
        .expect("failed to git init nested repo");
    let nested_file = nested_repo_dir.join("src").join("nested.txt");
    fs::write(&nested_file, "nested content").unwrap();
    assert!(
        !repo.path_is_in_workdir(&nested_file),
        "File inside a nested subrepo (with its own .git/ dir) should return false"
    );

    // Path directly in the nested repo root should also return false
    let nested_root_file = nested_repo_dir.join("root.txt");
    fs::write(&nested_root_file, "root content").unwrap();
    assert!(
        !repo.path_is_in_workdir(&nested_root_file),
        "File at root of nested subrepo should return false"
    );

    // Path in a subdirectory (no nested .git/) should still return true
    let subdir = test_repo.path().join("regular-subdir");
    fs::create_dir_all(&subdir).unwrap();
    let subdir_file = subdir.join("file.txt");
    fs::write(&subdir_file, "subdir content").unwrap();
    assert!(
        repo.path_is_in_workdir(&subdir_file),
        "File in a regular subdirectory (no .git/) should return true"
    );

    // Path inside a submodule (.git file, not directory) should return true
    // Submodules are transparent to the parent repo
    let submodule_dir = test_repo.path().join("my-submodule");
    fs::create_dir_all(submodule_dir.join("src")).unwrap();
    // Simulate a submodule by creating a .git *file* (not directory)
    fs::write(
        submodule_dir.join(".git"),
        "gitdir: ../.git/modules/my-submodule\n",
    )
    .unwrap();
    let submodule_file = submodule_dir.join("src").join("lib.rs");
    fs::write(&submodule_file, "submodule content").unwrap();
    assert!(
        repo.path_is_in_workdir(&submodule_file),
        "File inside a submodule (.git file, not directory) should return true"
    );

    // Non-existent file path inside a nested subrepo should return false
    // (exercises the normalized fallback path since canonicalize() will fail)
    let nonexistent = nested_repo_dir.join("does-not-exist").join("phantom.txt");
    assert!(
        !repo.path_is_in_workdir(&nonexistent),
        "Non-existent file inside a nested subrepo should return false (fallback path)"
    );

    // Non-existent file path in the repo (no nested .git) should return true
    let nonexistent_in_repo = test_repo.path().join("not-yet-created.txt");
    assert!(
        repo.path_is_in_workdir(&nonexistent_in_repo),
        "Non-existent file in the repo (no nested .git/) should return true (fallback path)"
    );

    // An empty directory-form .git (no HEAD) is not a valid repository and
    // must be transparent: files beneath it still belong to the parent repo.
    let empty_marker_dir = test_repo.path().join("empty-marker");
    fs::create_dir_all(empty_marker_dir.join(".git")).unwrap();
    let empty_marker_file = empty_marker_dir.join("file.txt");
    fs::write(&empty_marker_file, "vendored content").unwrap();
    assert!(
        repo.path_is_in_workdir(&empty_marker_file),
        "File beneath an empty .git directory should return true"
    );

    // Same for a non-existent file beneath it (canonical-parent fallback
    // branch; the parent must exist so the check stays symlink-safe on macOS,
    // where /tmp resolves to /private/tmp)
    let nonexistent_below_empty_marker = empty_marker_dir.join("phantom.txt");
    assert!(
        repo.path_is_in_workdir(&nonexistent_below_empty_marker),
        "Non-existent file beneath an empty .git directory should return true (fallback path)"
    );
}

// ============================================================================
// Bare Repository Tests
// ============================================================================

#[test]
fn test_is_bare_repository() {
    let bare_repo = TestRepo::new_bare();

    let repo = find_repository(&[
        "-C".to_string(),
        bare_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let is_bare = repo.is_bare_repository();
    assert!(is_bare.is_ok(), "Should check if bare");
    assert!(is_bare.unwrap(), "Should be bare repository");
}

#[test]
fn test_is_not_bare_repository() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let is_bare = repo.is_bare_repository();
    assert!(is_bare.is_ok(), "Should check if bare");
    assert!(!is_bare.unwrap(), "Should not be bare repository");
}

// ============================================================================
// Working Directory Operations Tests
// ============================================================================

#[test]
fn test_find_repository_in_path() {
    let test_repo = TestRepo::new();

    // Create a commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Test").unwrap();

    let result = find_repository_in_path(test_repo.path().to_str().unwrap());
    assert!(result.is_ok(), "Should find repository in path");
}

#[test]
fn test_global_args_for_exec() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let args = repo.global_args_for_exec();

    // Should include --no-pager
    assert!(
        args.contains(&"--no-pager".to_string()),
        "Global args should include --no-pager"
    );
}

// ============================================================================
// Edge Cases and Special Scenarios
// ============================================================================

#[test]
fn test_empty_repository() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // HEAD should exist even in empty repo
    let head = repo.head();
    assert!(head.is_ok(), "Should get HEAD in empty repository");
}

crate::reuse_tests_in_worktree!(
    test_find_repository_in_valid_repo,
    test_find_repository_in_subdirectory,
    test_find_repository_in_nested_subdirectory,
    test_find_repository_for_bare_repo,
    test_repository_path_methods,
    test_path_is_in_workdir,
    test_is_bare_repository,
    test_is_not_bare_repository,
    test_find_repository_in_path,
    test_global_args_for_exec,
    test_empty_repository,
);
