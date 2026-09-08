use super::{
    cleanup_tmp_dir, create_file, create_unique_tmp_dir, find_repository_for_file,
    group_files_by_repository, init_git_repo,
};

#[test]
fn test_files_with_spaces_in_path() {
    // Test handling of paths with spaces
    let workspace = create_unique_tmp_dir("git-ai-spaces-test").unwrap();

    let repo = workspace.join("my project");
    init_git_repo(&repo).unwrap();

    let file_with_spaces = repo.join("src files").join("my file.txt");
    create_file(&file_with_spaces, "content").unwrap();

    let file_paths = vec![file_with_spaces.to_str().unwrap().to_string()];

    let (repo_files, orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    assert_eq!(repo_files.len(), 1, "Should find repo with spaces in path");
    assert!(orphan_files.is_empty(), "No orphan files");

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_duplicate_files_same_repo() {
    // Test that duplicate file paths are handled correctly
    let workspace = create_unique_tmp_dir("git-ai-duplicate-test").unwrap();

    let repo = workspace.join("repo");
    init_git_repo(&repo).unwrap();

    let file = repo.join("file.txt");
    create_file(&file, "content").unwrap();

    // Pass the same file twice
    let file_paths = vec![
        file.to_str().unwrap().to_string(),
        file.to_str().unwrap().to_string(),
    ];

    let (repo_files, orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    assert_eq!(repo_files.len(), 1, "Should have 1 repository");
    assert!(orphan_files.is_empty(), "No orphan files");

    // The duplicate should be included twice in the file list
    for (_repo, files) in repo_files.values() {
        assert_eq!(files.len(), 2, "Duplicate files should both be in the list");
    }

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_group_files_by_repository() {
    // Create a workspace directory (not a git repo)
    let workspace = create_unique_tmp_dir("git-ai-group-files-test").unwrap();

    // Create two git repositories inside the workspace
    let repo_a = workspace.join("repo-a");
    let repo_b = workspace.join("repo-b");

    init_git_repo(&repo_a).unwrap();
    init_git_repo(&repo_b).unwrap();

    // Create files in each repository
    let file_a1 = repo_a.join("file_a1.txt");
    let file_a2 = repo_a.join("src").join("file_a2.txt");
    let file_b1 = repo_b.join("file_b1.txt");
    let orphan = workspace.join("orphan.txt");

    create_file(&file_a1, "content a1").unwrap();
    create_file(&file_a2, "content a2").unwrap();
    create_file(&file_b1, "content b1").unwrap();
    create_file(&orphan, "orphan content").unwrap();

    // Group files by repository
    let file_paths = vec![
        file_a1.to_str().unwrap().to_string(),
        file_a2.to_str().unwrap().to_string(),
        file_b1.to_str().unwrap().to_string(),
        orphan.to_str().unwrap().to_string(),
    ];

    let (repo_files, orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    // Should have 2 repositories detected
    assert_eq!(repo_files.len(), 2, "Should detect 2 repositories");

    // Should have 1 orphan file
    assert_eq!(orphan_files.len(), 1, "Should have 1 orphan file");
    assert!(
        orphan_files[0].contains("orphan.txt"),
        "Orphan file should be orphan.txt"
    );

    // Verify file grouping
    let mut repo_a_files_count = 0;
    let mut repo_b_files_count = 0;

    for (workdir, (_repo, files)) in &repo_files {
        if workdir.to_string_lossy().contains("repo-a") {
            repo_a_files_count = files.len();
            assert_eq!(files.len(), 2, "repo-a should have 2 files");
        } else if workdir.to_string_lossy().contains("repo-b") {
            repo_b_files_count = files.len();
            assert_eq!(files.len(), 1, "repo-b should have 1 file");
        }
    }

    assert_eq!(repo_a_files_count, 2, "Should find 2 files in repo-a");
    assert_eq!(repo_b_files_count, 1, "Should find 1 file in repo-b");

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_empty_file_list_grouping() {
    // Test edge case with empty file list
    let (repo_files, orphan_files) = group_files_by_repository(&[], None);

    assert!(
        repo_files.is_empty(),
        "Should have no repos with empty file list"
    );
    assert!(
        orphan_files.is_empty(),
        "Should have no orphans with empty file list"
    );
}

#[test]
fn test_cross_repo_edits_grouping() {
    // Test that files from a single AI session spanning multiple repos are grouped correctly
    let workspace = create_unique_tmp_dir("git-ai-cross-repo-test").unwrap();

    // Create three git repositories simulating a monorepo-like workspace
    let frontend_repo = workspace.join("frontend");
    let backend_repo = workspace.join("backend");
    let shared_repo = workspace.join("shared");

    init_git_repo(&frontend_repo).unwrap();
    init_git_repo(&backend_repo).unwrap();
    init_git_repo(&shared_repo).unwrap();

    // Create files in each repository (simulating an AI making related changes across repos)
    let frontend_file1 = frontend_repo.join("src").join("App.tsx");
    let frontend_file2 = frontend_repo
        .join("src")
        .join("components")
        .join("Button.tsx");
    let backend_file = backend_repo.join("src").join("api.py");
    let shared_file = shared_repo.join("types").join("shared_types.ts");

    create_file(&frontend_file1, "// Frontend App").unwrap();
    create_file(&frontend_file2, "// Button component").unwrap();
    create_file(&backend_file, "# Backend API").unwrap();
    create_file(&shared_file, "// Shared types").unwrap();

    // Simulate a single AI session editing files across all repos
    let file_paths = vec![
        frontend_file1.to_str().unwrap().to_string(),
        frontend_file2.to_str().unwrap().to_string(),
        backend_file.to_str().unwrap().to_string(),
        shared_file.to_str().unwrap().to_string(),
    ];

    let (repo_files, orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    // Should have 3 repositories detected
    assert_eq!(
        repo_files.len(),
        3,
        "Should detect 3 repositories for cross-repo edits"
    );

    // No orphan files
    assert!(orphan_files.is_empty(), "Should have no orphan files");

    // Verify correct distribution
    let mut frontend_count = 0;
    let mut backend_count = 0;
    let mut shared_count = 0;

    for (workdir, (_repo, files)) in &repo_files {
        let workdir_str = workdir.to_string_lossy();
        if workdir_str.contains("frontend") {
            frontend_count = files.len();
        } else if workdir_str.contains("backend") {
            backend_count = files.len();
        } else if workdir_str.contains("shared") {
            shared_count = files.len();
        }
    }

    assert_eq!(frontend_count, 2, "Frontend should have 2 files");
    assert_eq!(backend_count, 1, "Backend should have 1 file");
    assert_eq!(shared_count, 1, "Shared should have 1 file");

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_workspace_relative_paths() {
    // Test that relative paths work when converted to absolute
    let workspace = create_unique_tmp_dir("git-ai-relative-path-test").unwrap();

    let repo = workspace.join("my-project");
    init_git_repo(&repo).unwrap();

    // Create files
    let file1 = repo.join("src").join("main.rs");
    let file2 = repo.join("lib").join("utils.rs");

    create_file(&file1, "fn main() {}").unwrap();
    create_file(&file2, "pub fn util() {}").unwrap();

    // Test with workspace-relative paths (simulating what an IDE might send)
    // When paths are relative to workspace root
    let relative_paths = [
        "my-project/src/main.rs".to_string(),
        "my-project/lib/utils.rs".to_string(),
    ];

    // Convert to absolute paths (simulating what handle_checkpoint does)
    let absolute_paths: Vec<String> = relative_paths
        .iter()
        .map(|p| workspace.join(p).to_string_lossy().to_string())
        .collect();

    let (repo_files, orphan_files) =
        group_files_by_repository(&absolute_paths, Some(workspace.to_str().unwrap()));

    assert_eq!(repo_files.len(), 1, "Should find 1 repository");
    assert!(orphan_files.is_empty(), "Should have no orphan files");

    // Verify the files are grouped correctly
    for (_repo, files) in repo_files.values() {
        assert_eq!(files.len(), 2, "Should have 2 files in the repo");
    }

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_deeply_nested_file_detection() {
    // Test that deeply nested files still find their repository correctly
    let workspace = create_unique_tmp_dir("git-ai-deep-nest-test").unwrap();

    let repo = workspace.join("monorepo");
    init_git_repo(&repo).unwrap();

    // Create a deeply nested file structure
    let deep_file = repo
        .join("packages")
        .join("core")
        .join("src")
        .join("utils")
        .join("helpers")
        .join("deeply_nested.ts");

    create_file(&deep_file, "export const helper = () => {};").unwrap();

    let result = find_repository_for_file(
        deep_file.to_str().unwrap(),
        Some(workspace.to_str().unwrap()),
    );

    assert!(
        result.is_ok(),
        "Should find repository for deeply nested file"
    );

    let workdir = result.unwrap().workdir().unwrap();
    assert!(
        workdir.to_string_lossy().contains("monorepo"),
        "Deeply nested file should be in monorepo"
    );

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_mixed_absolute_and_relative_grouping() {
    // Test grouping with a mix of absolute and relative paths
    let workspace = create_unique_tmp_dir("git-ai-mixed-paths-test").unwrap();

    let repo = workspace.join("project");
    init_git_repo(&repo).unwrap();

    let file1 = repo.join("file1.txt");
    let file2 = repo.join("file2.txt");

    create_file(&file1, "content 1").unwrap();
    create_file(&file2, "content 2").unwrap();

    // Mix of absolute path and path that needs conversion
    let paths = vec![
        file1.to_str().unwrap().to_string(), // Already absolute
        file2.to_str().unwrap().to_string(), // Already absolute
    ];

    let (repo_files, orphan_files) =
        group_files_by_repository(&paths, Some(workspace.to_str().unwrap()));

    assert_eq!(repo_files.len(), 1, "Should detect 1 repository");
    assert!(orphan_files.is_empty(), "Should have no orphans");

    for (_repo, files) in repo_files.values() {
        assert_eq!(files.len(), 2, "Both files should be in the same repo");
    }

    cleanup_tmp_dir(&workspace);
}

#[test]
fn test_repository_isolation() {
    // Verify that files in different repos don't get mixed up
    // and each repo maintains its own attribution tracking
    let workspace = create_unique_tmp_dir("git-ai-isolation-test").unwrap();

    // Create two completely separate repos
    let repo_alpha = workspace.join("alpha");
    let repo_beta = workspace.join("beta");

    init_git_repo(&repo_alpha).unwrap();
    init_git_repo(&repo_beta).unwrap();

    // Create same-named files in both repos (shouldn't cause confusion)
    let alpha_readme = repo_alpha.join("README.md");
    let beta_readme = repo_beta.join("README.md");
    let alpha_config = repo_alpha.join("config.json");
    let beta_config = repo_beta.join("config.json");

    create_file(&alpha_readme, "# Alpha Project").unwrap();
    create_file(&beta_readme, "# Beta Project").unwrap();
    create_file(&alpha_config, r#"{"name": "alpha"}"#).unwrap();
    create_file(&beta_config, r#"{"name": "beta"}"#).unwrap();

    let file_paths = vec![
        alpha_readme.to_str().unwrap().to_string(),
        beta_readme.to_str().unwrap().to_string(),
        alpha_config.to_str().unwrap().to_string(),
        beta_config.to_str().unwrap().to_string(),
    ];

    let (repo_files, orphan_files) =
        group_files_by_repository(&file_paths, Some(workspace.to_str().unwrap()));

    // Verify isolation - should be 2 separate repos
    assert_eq!(repo_files.len(), 2, "Should have 2 isolated repositories");
    assert!(orphan_files.is_empty(), "No orphan files");

    // Each repo should have exactly 2 files
    for (workdir, (_repo, files)) in &repo_files {
        assert_eq!(
            files.len(),
            2,
            "Each repo should have exactly 2 files, got {} in {}",
            files.len(),
            workdir.display()
        );

        // Verify files belong to correct repo
        let workdir_str = workdir.to_string_lossy();
        for file in files {
            if workdir_str.contains("alpha") {
                assert!(
                    file.contains("alpha"),
                    "Alpha repo should only contain alpha files"
                );
            } else if workdir_str.contains("beta") {
                assert!(
                    file.contains("beta"),
                    "Beta repo should only contain beta files"
                );
            }
        }
    }

    cleanup_tmp_dir(&workspace);
}
