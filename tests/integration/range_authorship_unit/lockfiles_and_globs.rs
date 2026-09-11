use super::{CommitRange, TestRepo, find_repository_in_path, range_authorship};

#[test]
fn test_range_authorship_ignores_single_lockfile() {
    let repo = TestRepo::new();

    // Create initial commit with a source file
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    repo.git(&["add", "src/main.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/main.rs"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Add AI work to source file and also change a lockfile
    std::fs::write(
        repo.path().join("src/main.rs"),
        "fn main() {}\n// AI added code\nfn helper() {}\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("Cargo.lock"),
        "# Large lockfile with 1000 lines\n".repeat(1000),
    )
    .unwrap();
    repo.git(&["add", "src/main.rs", "Cargo.lock"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/main.rs"])
        .unwrap();
    repo.stage_all_and_commit("Add helper and update deps")
        .unwrap();
    let second_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        first_sha.clone(),
        second_sha.clone(),
        Some("HEAD".to_string()),
    )
    .unwrap();

    let lockfile_patterns = vec![
        "Cargo.lock".to_string(),
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
    ];
    let stats = range_authorship(commit_range, false, &lockfile_patterns, None).unwrap();

    // Verify lockfile is excluded: only 2 lines added (from main.rs), not 1000+ from lockfile
    assert_eq!(stats.authorship_stats.total_commits, 1);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 1);
    assert_eq!(stats.range_stats.ai_additions, 2); // Only the 2 AI lines in main.rs
    assert_eq!(stats.range_stats.git_diff_added_lines, 2); // Lockfile excluded (1000 lines ignored)
    // The key assertion: git_diff should be 2, not 1002 if lockfile was included
    assert!(stats.range_stats.git_diff_added_lines < 100); // Significantly less than if lockfile was counted
}

#[test]
fn test_range_authorship_mixed_lockfile_and_source() {
    let repo = TestRepo::new();

    // Create initial commit
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/lib.rs"), "pub fn old() {}\n").unwrap();
    repo.git(&["add", "src/lib.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/lib.rs"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Human adds to source file
    std::fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn old() {}\npub fn new() {}\n",
    )
    .unwrap();
    repo.git(&["add", "src/lib.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/lib.rs"])
        .unwrap();
    repo.stage_all_and_commit("Human adds function").unwrap();

    // AI adds to source file, and package-lock.json is updated (with 1000 lines)
    std::fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn old() {}\npub fn new() {}\n// AI comment\npub fn ai_func() {}\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("package-lock.json"),
        "{\n  \"lockfileVersion\": 2,\n}\n".repeat(1000),
    )
    .unwrap();
    repo.git(&["add", "src/lib.rs", "package-lock.json"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/lib.rs"])
        .unwrap();
    repo.stage_all_and_commit("AI adds function and updates deps")
        .unwrap();
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        first_sha.clone(),
        head_sha.clone(),
        Some("HEAD".to_string()),
    )
    .unwrap();

    let lockfile_patterns = vec![
        "Cargo.lock".to_string(),
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
    ];
    let stats = range_authorship(commit_range, false, &lockfile_patterns, None).unwrap();

    // Key assertion: git_diff should only count lib.rs changes (3 lines), not package-lock.json (3000 lines)
    assert_eq!(stats.authorship_stats.total_commits, 2);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 2);
    assert_eq!(stats.range_stats.git_diff_added_lines, 3); // Only lib.rs, package-lock.json excluded
    // Verify the total is much less than 3003 (if lockfile was included)
    assert!(stats.range_stats.git_diff_added_lines < 100);
    // Verify that some AI work is detected and human lines are correctly
    // counted as human additions rather than unknown.
    assert!(stats.range_stats.ai_additions > 0);
    assert!(stats.range_stats.human_additions > 0);
}

#[test]
fn test_range_authorship_multiple_lockfile_types() {
    let repo = TestRepo::new();

    // Create initial commit
    std::fs::write(repo.path().join("README.md"), "# Project\n").unwrap();
    repo.git(&["add", "README.md"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "README.md"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Add multiple lockfiles and one real source change
    std::fs::write(repo.path().join("Cargo.lock"), "# Cargo lock\n".repeat(500)).unwrap();
    std::fs::write(repo.path().join("yarn.lock"), "# yarn lock\n".repeat(500)).unwrap();
    std::fs::write(
        repo.path().join("poetry.lock"),
        "# poetry lock\n".repeat(500),
    )
    .unwrap();
    std::fs::write(repo.path().join("go.sum"), "# go sum\n".repeat(500)).unwrap();
    std::fs::write(repo.path().join("README.md"), "# Project\n## New Section\n").unwrap();
    repo.git(&[
        "add",
        "Cargo.lock",
        "yarn.lock",
        "poetry.lock",
        "go.sum",
        "README.md",
    ])
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "README.md"])
        .unwrap();
    repo.stage_all_and_commit("Update dependencies").unwrap();
    let second_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        first_sha.clone(),
        second_sha.clone(),
        Some("HEAD".to_string()),
    )
    .unwrap();

    let lockfile_patterns = vec![
        "Cargo.lock".to_string(),
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
        "poetry.lock".to_string(),
        "go.sum".to_string(),
    ];
    let stats = range_authorship(commit_range, false, &lockfile_patterns, None).unwrap();

    // Verify: only the 1 README line is counted, all lockfiles excluded (2000 lines ignored)
    assert_eq!(stats.authorship_stats.total_commits, 1);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 1);
    assert_eq!(stats.range_stats.ai_additions, 1); // Only README.md line
    assert_eq!(stats.range_stats.git_diff_added_lines, 1); // All lockfiles excluded
}

#[test]
fn test_range_authorship_lockfile_only_commit() {
    let repo = TestRepo::new();

    // Create initial commit
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    repo.git(&["add", "src/main.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/main.rs"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Commit that only changes lockfiles (common scenario)
    std::fs::write(
        repo.path().join("package-lock.json"),
        "{\n  \"version\": \"1.0.0\"\n}\n".repeat(1000),
    )
    .unwrap();
    std::fs::write(repo.path().join("yarn.lock"), "# yarn\n".repeat(500)).unwrap();
    repo.git(&["add", "package-lock.json", "yarn.lock"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "package-lock.json"])
        .unwrap();
    repo.stage_all_and_commit("Update lockfiles only").unwrap();
    let second_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        first_sha.clone(),
        second_sha.clone(),
        Some("HEAD".to_string()),
    )
    .unwrap();

    let lockfile_patterns = vec![
        "Cargo.lock".to_string(),
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
    ];
    let stats = range_authorship(commit_range, false, &lockfile_patterns, None).unwrap();

    // Verify: no lines counted since only lockfiles changed
    assert_eq!(stats.authorship_stats.total_commits, 1);
    assert_eq!(stats.range_stats.git_diff_added_lines, 0); // All lockfiles excluded
    assert_eq!(stats.range_stats.ai_additions, 0);
    assert_eq!(stats.range_stats.human_additions, 0);
}

#[test]
fn test_range_authorship_with_glob_patterns() {
    let repo = TestRepo::new();

    // Initial commit
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    repo.git(&["add", "src/main.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/main.rs"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Add various files including lockfiles and generated files
    std::fs::write(
        repo.path().join("src/main.rs"),
        "fn main() {}\nfn helper() {}\n",
    )
    .unwrap();
    std::fs::write(repo.path().join("Cargo.lock"), "# lock\n".repeat(1000)).unwrap();
    std::fs::write(repo.path().join("package-lock.json"), "{}\n".repeat(500)).unwrap();
    std::fs::write(
        repo.path().join("api.generated.js"),
        "// generated\n".repeat(200),
    )
    .unwrap();
    repo.git(&[
        "add",
        "src/main.rs",
        "Cargo.lock",
        "package-lock.json",
        "api.generated.js",
    ])
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/main.rs"])
        .unwrap();
    repo.stage_all_and_commit("Add code and deps").unwrap();
    let second_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        first_sha.clone(),
        second_sha.clone(),
        Some("HEAD".to_string()),
    )
    .unwrap();

    // Use glob patterns to ignore lockfiles and generated files
    let glob_patterns = vec![
        "*.lock".to_string(),
        "*lock.json".to_string(), // Matches package-lock.json
        "*.generated.*".to_string(),
    ];
    let stats = range_authorship(commit_range, false, &glob_patterns, None).unwrap();

    // Should only count the 1 line in main.rs, ignoring 1700 lines in lockfiles and generated files
    assert_eq!(stats.range_stats.git_diff_added_lines, 1);
    assert_eq!(stats.range_stats.ai_additions, 1);
}
