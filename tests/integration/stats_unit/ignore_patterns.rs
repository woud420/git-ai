use super::{TestRepo, find_repository_in_path};
use git_ai::operations::authorship::stats::stats_for_commit_stats;

#[test]
fn test_stats_ignores_single_lockfile() {
    let repo = TestRepo::new();

    // Initial commit
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    repo.git(&["add", "src/main.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/main.rs"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Commit that adds source code and a large lockfile
    std::fs::write(
        repo.path().join("src/main.rs"),
        "fn main() {}\nfn helper() {}\n",
    )
    .unwrap();
    repo.git(&["add", "src/main.rs"]).unwrap();
    std::fs::write(repo.path().join("Cargo.lock"), "# lockfile\n".repeat(1000)).unwrap();
    repo.git(&["add", "Cargo.lock"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/main.rs"])
        .unwrap();
    repo.stage_all_and_commit("Add helper and deps").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Test WITHOUT ignore - should count lockfile
    let stats_with_lockfile = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();
    assert_eq!(stats_with_lockfile.git_diff_added_lines, 1001); // 1 source + 1000 lockfile

    // Test WITH ignore - should exclude lockfile
    let ignore_patterns = vec!["Cargo.lock".to_string()];
    let stats_without_lockfile =
        stats_for_commit_stats(&gitai_repo, &head_sha, &ignore_patterns).unwrap();
    assert_eq!(stats_without_lockfile.git_diff_added_lines, 1); // Only 1 source line
    assert_eq!(stats_without_lockfile.ai_additions, 1);
}

#[test]
fn test_stats_ignores_multiple_lockfiles() {
    let repo = TestRepo::new();

    // Initial commit
    std::fs::write(repo.path().join("README.md"), "# Project\n").unwrap();
    repo.git(&["add", "README.md"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "README.md"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Commit that updates multiple lockfiles and one source file
    std::fs::write(repo.path().join("README.md"), "# Project\n## New\n").unwrap();
    repo.git(&["add", "README.md"]).unwrap();
    std::fs::write(repo.path().join("Cargo.lock"), "# cargo\n".repeat(500)).unwrap();
    repo.git(&["add", "Cargo.lock"]).unwrap();
    std::fs::write(repo.path().join("package-lock.json"), "{}\n".repeat(500)).unwrap();
    repo.git(&["add", "package-lock.json"]).unwrap();
    std::fs::write(repo.path().join("yarn.lock"), "# yarn\n".repeat(500)).unwrap();
    repo.git(&["add", "yarn.lock"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "README.md"])
        .unwrap();
    repo.stage_all_and_commit("Update deps").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Test WITHOUT ignore - counts all files (1501 lines)
    let stats_all = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();
    assert_eq!(stats_all.git_diff_added_lines, 1501);

    // Test WITH ignore - only counts README (1 line)
    let ignore_patterns = vec![
        "Cargo.lock".to_string(),
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
    ];
    let stats_filtered = stats_for_commit_stats(&gitai_repo, &head_sha, &ignore_patterns).unwrap();
    assert_eq!(stats_filtered.git_diff_added_lines, 1);
    // KnownHuman checkpoints record h_<hash> attributions, so the README line is human_additions.
    assert_eq!(stats_filtered.human_additions, 1);
    assert_eq!(stats_filtered.unknown_additions, 0);
}

#[test]
fn test_stats_with_lockfile_only_commit() {
    let repo = TestRepo::new();

    // Initial commit
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/lib.rs"), "pub fn foo() {}\n").unwrap();
    repo.git(&["add", "src/lib.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/lib.rs"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Commit that ONLY updates lockfiles (common during dependency updates)
    std::fs::write(repo.path().join("Cargo.lock"), "# updated\n".repeat(2000)).unwrap();
    repo.git(&["add", "Cargo.lock"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "Cargo.lock"])
        .unwrap();
    repo.stage_all_and_commit("Update dependencies").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Test WITHOUT ignore - shows 2000 lines
    let stats_with = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();
    assert_eq!(stats_with.git_diff_added_lines, 2000);

    // Test WITH ignore - shows 0 lines (lockfile-only commit)
    let ignore_patterns = vec!["Cargo.lock".to_string()];
    let stats_without = stats_for_commit_stats(&gitai_repo, &head_sha, &ignore_patterns).unwrap();
    assert_eq!(stats_without.git_diff_added_lines, 0);
    assert_eq!(stats_without.ai_additions, 0);
    assert_eq!(stats_without.human_additions, 0);
}

#[test]
fn test_stats_empty_ignore_patterns() {
    let repo = TestRepo::new();

    // Initial commit
    std::fs::write(repo.path().join("test.txt"), "Line1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Add lines
    std::fs::write(repo.path().join("test.txt"), "Line1\nLine2\nLine3\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("Add lines").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Test with empty patterns - should behave same as no filtering
    let stats = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();
    assert_eq!(stats.git_diff_added_lines, 2);
    assert_eq!(stats.ai_additions, 2);
}

#[test]
fn test_stats_with_glob_patterns() {
    let repo = TestRepo::new();

    // Initial commit
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/lib.rs"), "pub fn foo() {}\n").unwrap();
    repo.git(&["add", "src/lib.rs"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "src/lib.rs"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Commit with source code + lockfiles + generated files
    std::fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn foo() {}\npub fn bar() {}\n",
    )
    .unwrap();
    repo.git(&["add", "src/lib.rs"]).unwrap();
    std::fs::write(repo.path().join("Cargo.lock"), "# lock\n".repeat(1000)).unwrap();
    repo.git(&["add", "Cargo.lock"]).unwrap();
    std::fs::write(repo.path().join("package-lock.json"), "{}\n".repeat(500)).unwrap();
    repo.git(&["add", "package-lock.json"]).unwrap();
    std::fs::write(
        repo.path().join("api.generated.ts"),
        "// generated\n".repeat(300),
    )
    .unwrap();
    repo.git(&["add", "api.generated.ts"]).unwrap();
    std::fs::write(
        repo.path().join("schema.generated.js"),
        "// schema\n".repeat(200),
    )
    .unwrap();
    repo.git(&["add", "schema.generated.js"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/lib.rs"])
        .unwrap();
    repo.stage_all_and_commit("Add code").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Test WITHOUT ignore - all files included (2001 lines)
    let stats_all = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();
    assert_eq!(stats_all.git_diff_added_lines, 2001);

    // Test WITH glob patterns - only source code (1 line)
    let glob_patterns = vec![
        "*.lock".to_string(),        // Matches Cargo.lock
        "*lock.json".to_string(),    // Matches package-lock.json
        "*.generated.*".to_string(), // Matches *.generated.ts, *.generated.js
    ];
    let stats_filtered = stats_for_commit_stats(&gitai_repo, &head_sha, &glob_patterns).unwrap();
    assert_eq!(stats_filtered.git_diff_added_lines, 1);
    assert_eq!(stats_filtered.ai_additions, 1);
}
