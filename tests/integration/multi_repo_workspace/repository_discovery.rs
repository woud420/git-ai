use super::{
    Command, cleanup_tmp_dir, create_file, create_unique_tmp_dir, find_repository_for_file,
    find_repository_in_path, fs, group_files_by_repository, init_git_repo,
};

#[test]
fn test_find_repository_for_file_basic() {
    // Create a workspace directory (not a git repo)
    let workspace = create_unique_tmp_dir("git-ai-multi-repo-test").unwrap();

    // Create a git repository inside the workspace
    let repo_a = workspace.join("repo-a");
    init_git_repo(&repo_a).unwrap();

    // Create a file inside the repository
    let file_path = repo_a.join("src").join("main.rs");
    create_file(&file_path, "fn main() {}").unwrap();

    // Test that we can find the repository from the file
    let result = find_repository_for_file(
        file_path.to_str().unwrap(),
        Some(workspace.to_str().unwrap()),
    );

    assert!(result.is_ok(), "Should find repository from file path");

    let repo = result.unwrap();
    let workdir = repo.workdir().unwrap();

    // The workdir should be the repo-a directory
    assert!(
        workdir.ends_with("repo-a") || workdir.to_string_lossy().contains("repo-a"),
        "Repository workdir should be repo-a, got: {}",
        workdir.display()
    );

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_nonexistent_file_path() {
    // Test behavior when file paths don't exist on disk
    let workspace = create_unique_tmp_dir("git-ai-nonexistent-test").unwrap();

    let repo = workspace.join("repo");
    init_git_repo(&repo).unwrap();

    // Create one real file
    let real_file = repo.join("real_file.txt");
    create_file(&real_file, "content").unwrap();

    // Reference a file that doesn't exist
    let nonexistent_file = repo.join("nonexistent_file.txt");

    let file_paths = vec![
        real_file.to_str().unwrap().to_string(),
        nonexistent_file.to_str().unwrap().to_string(),
    ];

    let (repo_files, _orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    // The real file should be found in the repo
    // The nonexistent file behavior depends on implementation -
    // it should still find the repo since the parent directory exists
    assert!(
        !repo_files.is_empty(),
        "Should find repository for existing file"
    );

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_all_files_orphaned() {
    // Test when all provided files are orphans (no git repos)
    let workspace = create_unique_tmp_dir("git-ai-all-orphans-test").unwrap();

    // Create files without any git repository
    let file1 = workspace.join("dir1").join("file1.txt");
    let file2 = workspace.join("dir2").join("file2.txt");

    create_file(&file1, "content 1").unwrap();
    create_file(&file2, "content 2").unwrap();

    let file_paths = vec![
        file1.to_str().unwrap().to_string(),
        file2.to_str().unwrap().to_string(),
    ];

    let (repo_files, orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    // All files should be orphans
    assert!(
        repo_files.is_empty(),
        "Should have no repos when all files are orphans"
    );
    assert_eq!(orphan_files.len(), 2, "All files should be orphans");

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_single_repo_in_multi_repo_workspace() {
    // Test when only one repo has edits even though workspace has multiple repos
    let workspace = create_unique_tmp_dir("git-ai-single-edit-test").unwrap();

    // Create multiple repos but only edit files in one
    let repo_a = workspace.join("repo-a");
    let repo_b = workspace.join("repo-b");
    let repo_c = workspace.join("repo-c");

    init_git_repo(&repo_a).unwrap();
    init_git_repo(&repo_b).unwrap();
    init_git_repo(&repo_c).unwrap();

    // Only create/edit files in repo-b
    let file_b1 = repo_b.join("src").join("main.rs");
    let file_b2 = repo_b.join("src").join("lib.rs");

    create_file(&file_b1, "fn main() {}").unwrap();
    create_file(&file_b2, "pub fn lib() {}").unwrap();

    let file_paths = vec![
        file_b1.to_str().unwrap().to_string(),
        file_b2.to_str().unwrap().to_string(),
    ];

    let (repo_files, orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    // Should detect only 1 repository (repo-b)
    assert_eq!(
        repo_files.len(),
        1,
        "Should detect only 1 repository with edits"
    );

    assert!(orphan_files.is_empty(), "No orphan files");

    // Verify it's repo-b
    for (workdir, (_repo, files)) in &repo_files {
        assert!(
            workdir.to_string_lossy().contains("repo-b"),
            "Should be repo-b, got: {}",
            workdir.display()
        );
        assert_eq!(files.len(), 2, "repo-b should have 2 files");
    }

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_symlinked_repository() {
    // Test that symlinked repositories are handled correctly
    let workspace = create_unique_tmp_dir("git-ai-symlink-test").unwrap();

    // Create actual repo
    let actual_repo = workspace.join("actual-repo");
    init_git_repo(&actual_repo).unwrap();

    let file_in_repo = actual_repo.join("file.txt");
    create_file(&file_in_repo, "content").unwrap();

    // Create symlink to the repo (symlinks only work reliably on unix)
    #[cfg(unix)]
    let symlink_path = workspace.join("linked-repo");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        if symlink(&actual_repo, &symlink_path).is_ok() {
            let file_via_symlink = symlink_path.join("file.txt");

            let result = find_repository_for_file(
                file_via_symlink.to_str().unwrap(),
                Some(workspace.to_str().unwrap()),
            );

            // Should find the repository through the symlink
            assert!(result.is_ok(), "Should find repository through symlink");
        }
    }

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_bare_repository_handling() {
    // Test that bare repositories are handled correctly (they have no working directory)
    let workspace = create_unique_tmp_dir("git-ai-bare-repo-test").unwrap();

    // Create a bare repository
    let bare_repo = workspace.join("bare.git");
    fs::create_dir_all(&bare_repo).unwrap();

    let output = Command::new("git")
        .current_dir(&bare_repo)
        .args(["init", "--bare"])
        .output();

    if output.is_ok() && output.unwrap().status.success() {
        // Create a normal repo alongside it
        let normal_repo = workspace.join("normal-repo");
        init_git_repo(&normal_repo).unwrap();

        let file = normal_repo.join("file.txt");
        create_file(&file, "content").unwrap();

        // File in normal repo should work fine
        let result =
            find_repository_for_file(file.to_str().unwrap(), Some(workspace.to_str().unwrap()));

        assert!(result.is_ok(), "Should find normal repository");
    }

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_find_repository_for_file_with_multiple_repos() {
    // Create a workspace directory (not a git repo)
    let workspace = create_unique_tmp_dir("git-ai-multi-repo-test").unwrap();

    // Create two git repositories inside the workspace
    let repo_a = workspace.join("repo-a");
    let repo_b = workspace.join("repo-b");

    init_git_repo(&repo_a).unwrap();
    init_git_repo(&repo_b).unwrap();

    // Create files in each repository
    let file_a = repo_a.join("file_a.txt");
    let file_b = repo_b.join("file_b.txt");

    create_file(&file_a, "content a").unwrap();
    create_file(&file_b, "content b").unwrap();

    // Test file in repo-a
    let result_a =
        find_repository_for_file(file_a.to_str().unwrap(), Some(workspace.to_str().unwrap()));

    assert!(
        result_a.is_ok(),
        "Should find repository for file in repo-a"
    );
    let repo_a_found = result_a.unwrap();
    let workdir_a = repo_a_found.workdir().unwrap();
    assert!(
        workdir_a.ends_with("repo-a") || workdir_a.to_string_lossy().contains("repo-a"),
        "File a should be in repo-a"
    );

    // Test file in repo-b
    let result_b =
        find_repository_for_file(file_b.to_str().unwrap(), Some(workspace.to_str().unwrap()));

    assert!(
        result_b.is_ok(),
        "Should find repository for file in repo-b"
    );
    let repo_b_found = result_b.unwrap();
    let workdir_b = repo_b_found.workdir().unwrap();
    assert!(
        workdir_b.ends_with("repo-b") || workdir_b.to_string_lossy().contains("repo-b"),
        "File b should be in repo-b"
    );

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_find_repository_for_file_no_repo_found() {
    // Create a directory without a git repository
    let workspace = create_unique_tmp_dir("git-ai-no-repo-test").unwrap();

    // Create a file in the workspace (no git repo)
    let file_path = workspace.join("orphan_file.txt");
    create_file(&file_path, "content").unwrap();

    // Test that no repository is found
    let result = find_repository_for_file(
        file_path.to_str().unwrap(),
        Some(workspace.to_str().unwrap()),
    );

    assert!(
        result.is_err(),
        "Should not find repository for orphan file"
    );

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_find_repository_for_file_respects_workspace_boundary() {
    // Create a parent git repo and a workspace inside it
    let parent_repo = create_unique_tmp_dir("git-ai-parent-repo-test").unwrap();
    init_git_repo(&parent_repo).unwrap();

    // Create a workspace subdirectory (not a git repo) inside the parent
    let workspace = parent_repo.join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Create a file in the workspace
    let file_path = workspace.join("file.txt");
    create_file(&file_path, "content").unwrap();

    // When workspace boundary is set, should NOT find the parent repo
    let result_with_boundary = find_repository_for_file(
        file_path.to_str().unwrap(),
        Some(workspace.to_str().unwrap()),
    );

    // This should fail because we're limiting the search to the workspace boundary
    assert!(
        result_with_boundary.is_err(),
        "Should not find parent repository when workspace boundary is set"
    );

    // When no workspace boundary is set, should find the parent repo
    let result_without_boundary = find_repository_for_file(file_path.to_str().unwrap(), None);

    assert!(
        result_without_boundary.is_ok(),
        "Should find parent repository when no workspace boundary is set"
    );

    cleanup_tmp_dir(&parent_repo);
}

#[test]
fn test_find_repository_for_file_nested_repos() {
    // Create a workspace with nested git repositories
    let workspace = create_unique_tmp_dir("git-ai-nested-repos-test").unwrap();

    // Create outer git repository
    let outer_repo = workspace.join("outer");
    init_git_repo(&outer_repo).unwrap();

    // Create inner git repository (nested)
    let inner_repo = outer_repo.join("inner");
    init_git_repo(&inner_repo).unwrap();

    // Create files in both repositories
    let outer_file = outer_repo.join("outer_file.txt");
    let inner_file = inner_repo.join("inner_file.txt");

    create_file(&outer_file, "outer content").unwrap();
    create_file(&inner_file, "inner content").unwrap();

    // File in outer repo should find outer repo
    let result_outer = find_repository_for_file(
        outer_file.to_str().unwrap(),
        Some(workspace.to_str().unwrap()),
    );

    assert!(result_outer.is_ok(), "Should find outer repository");
    let outer_workdir = result_outer.unwrap().workdir().unwrap();
    assert!(
        outer_workdir.ends_with("outer") && !outer_workdir.to_string_lossy().contains("inner"),
        "Outer file should be in outer repo, got: {}",
        outer_workdir.display()
    );

    // File in inner repo should find inner repo (the nearest .git)
    let result_inner = find_repository_for_file(
        inner_file.to_str().unwrap(),
        Some(workspace.to_str().unwrap()),
    );

    assert!(result_inner.is_ok(), "Should find inner repository");
    let inner_workdir = result_inner.unwrap().workdir().unwrap();
    assert!(
        inner_workdir.to_string_lossy().contains("inner"),
        "Inner file should be in inner repo, got: {}",
        inner_workdir.display()
    );

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_find_repository_in_path_still_works() {
    // Ensure the original function still works for normal single-repo scenarios
    let repo = create_unique_tmp_dir("git-ai-single-repo-test").unwrap();
    init_git_repo(&repo).unwrap();

    // Create an initial commit so the repo is valid
    let file = repo.join("README.md");
    create_file(&file, "# Test").unwrap();

    Command::new("git")
        .current_dir(&repo)
        .args(["add", "."])
        .output()
        .ok();

    Command::new("git")
        .current_dir(&repo)
        .args(["commit", "-m", "Initial commit"])
        .output()
        .ok();

    // The original function should work
    let result = find_repository_in_path(repo.to_str().unwrap());
    assert!(
        result.is_ok(),
        "find_repository_in_path should work for normal repos"
    );

    cleanup_tmp_dir(&repo);
}

#[test]
fn test_find_repository_for_directory() {
    // Test that find_repository_for_file works with directories too
    let workspace = create_unique_tmp_dir("git-ai-dir-test").unwrap();

    let repo = workspace.join("repo");
    init_git_repo(&repo).unwrap();

    let subdir = repo.join("src").join("components");
    fs::create_dir_all(&subdir).unwrap();

    // Test finding repo from a directory path
    let result =
        find_repository_for_file(subdir.to_str().unwrap(), Some(workspace.to_str().unwrap()));

    assert!(result.is_ok(), "Should find repository from directory path");
    let workdir = result.unwrap().workdir().unwrap();
    assert!(
        workdir.to_string_lossy().contains("repo"),
        "Directory should be in repo"
    );

    cleanup_tmp_dir(&workspace);
}
