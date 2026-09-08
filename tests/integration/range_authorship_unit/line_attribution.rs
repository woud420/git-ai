use super::{CommitRange, EMPTY_TREE_HASH, TestRepo, find_repository_in_path, range_authorship};

#[test]
fn test_range_authorship_simple_range() {
    let repo = TestRepo::new();

    // Create initial commit with human work
    std::fs::write(repo.path().join("test.txt"), "Line 1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Add AI work
    std::fs::write(
        repo.path().join("test.txt"),
        "Line 1\nAI Line 2\nAI Line 3\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("AI adds lines").unwrap();
    let second_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship from first to second commit
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

    // Verify stats
    assert_eq!(stats.authorship_stats.total_commits, 1);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 1);
    assert_eq!(stats.range_stats.ai_additions, 2);
    assert_eq!(stats.range_stats.git_diff_added_lines, 2);
}

#[test]
fn test_range_authorship_from_empty_tree() {
    let repo = TestRepo::new();

    // Create initial commit with AI work
    std::fs::write(repo.path().join("test.txt"), "AI Line 1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("Initial AI commit").unwrap();

    // Add more AI work
    std::fs::write(
        repo.path().join("test.txt"),
        "AI Line 1\nAI Line 2\nAI Line 3\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("Second AI commit").unwrap();
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship from empty tree to HEAD
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        EMPTY_TREE_HASH.to_string(),
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

    // Verify stats - should include all commits from beginning
    assert_eq!(stats.authorship_stats.total_commits, 2);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 2);
    // When using empty tree, the range stats show the diff from empty to HEAD
    // The AI additions count is based on the filtered attributions for commits in range
    assert_eq!(stats.range_stats.ai_additions, 3);
    assert_eq!(stats.range_stats.git_diff_added_lines, 3);
}

#[test]
fn test_range_authorship_single_commit() {
    let repo = TestRepo::new();

    // Create initial commit
    std::fs::write(repo.path().join("test.txt"), "Line 1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create AI commit
    std::fs::write(repo.path().join("test.txt"), "Line 1\nAI Line 2\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("AI commit").unwrap();
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship for single commit (start == end)
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        head_sha.clone(),
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

    // For single commit, should use stats_for_commit_stats
    assert_eq!(stats.authorship_stats.total_commits, 1);
    assert_eq!(stats.range_stats.ai_additions, 1);
}

#[test]
fn test_range_authorship_mixed_commits() {
    let repo = TestRepo::new();

    // Create initial commit with human work
    std::fs::write(repo.path().join("test.txt"), "Human Line 1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Add AI work
    std::fs::write(repo.path().join("test.txt"), "Human Line 1\nAI Line 2\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("AI commit").unwrap();

    // Add human work
    std::fs::write(
        repo.path().join("test.txt"),
        "Human Line 1\nAI Line 2\nHuman Line 3\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Human commit").unwrap();

    // Add more AI work
    std::fs::write(
        repo.path().join("test.txt"),
        "Human Line 1\nAI Line 2\nHuman Line 3\nAI Line 4\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("Another AI commit").unwrap();
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship from first to head
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

    // Verify stats
    assert_eq!(stats.authorship_stats.total_commits, 3);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 3);
    // Range authorship merges attributions from start to end, filtering to commits in range
    // The exact AI/human split depends on the merge attribution logic
    assert_eq!(stats.range_stats.ai_additions, 2);
    // Known-human lines in the range count as human additions, not unknown.
    assert_eq!(stats.range_stats.human_additions, 1);
    assert_eq!(stats.range_stats.unknown_additions, 0);
    assert_eq!(stats.range_stats.git_diff_added_lines, 3);
}

#[test]
fn test_range_authorship_no_changes() {
    let repo = TestRepo::new();

    // Create a commit
    std::fs::write(repo.path().join("test.txt"), "Line 1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();
    let sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship with same start and end (already tested above but worth verifying)
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        sha.clone(),
        sha.clone(),
        Some("HEAD".to_string()),
    )
    .unwrap();

    let lockfile_patterns = vec![
        "Cargo.lock".to_string(),
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
    ];
    let stats = range_authorship(commit_range, false, &lockfile_patterns, None).unwrap();

    // Should have 1 commit but no diffs since start == end
    assert_eq!(stats.authorship_stats.total_commits, 1);
}

#[test]
fn test_range_authorship_empty_tree_with_multiple_files() {
    let repo = TestRepo::new();

    // Create multiple files with AI work in first commit
    std::fs::write(repo.path().join("file1.txt"), "AI content 1\n").unwrap();
    std::fs::write(repo.path().join("file2.txt"), "AI content 2\n").unwrap();
    repo.git(&["add", "file1.txt", "file2.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file1.txt"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file2.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial multi-file commit")
        .unwrap();
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Test range authorship from empty tree
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let commit_range = CommitRange::new_infer_refname(
        &gitai_repo,
        EMPTY_TREE_HASH.to_string(),
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

    // Verify all files are included
    assert_eq!(stats.authorship_stats.total_commits, 1);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 1);
    assert_eq!(stats.range_stats.ai_additions, 2);
    assert_eq!(stats.range_stats.git_diff_added_lines, 2);
}

#[test]
fn test_range_authorship_counts_known_human_additions() {
    // Range stats hardcoded human additions to zero, so known-human lines in
    // a range surfaced as unknown additions instead of human additions.
    use crate::repos::test_file::ExpectedLineExt;

    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "Human Line 1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial human commit").unwrap();
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines!["Human Line 1".human()]);
    let first_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    std::fs::write(
        repo.path().join("test.txt"),
        "Human Line 1\nAI Line 2\nAI Line 3\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("AI adds 2 lines").unwrap();
    file.assert_committed_lines(crate::lines![
        "Human Line 1".human(),
        "AI Line 2".ai(),
        "AI Line 3".ai(),
    ]);

    std::fs::write(
        repo.path().join("test.txt"),
        "Human Line 1\nAI Line 2\nAI Line 3\nHuman Line 4\nHuman Line 5\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Human adds 2 more lines")
        .unwrap();
    file.assert_committed_lines(crate::lines![
        "Human Line 1".human(),
        "AI Line 2".ai(),
        "AI Line 3".ai(),
        "Human Line 4".human(),
        "Human Line 5".human(),
    ]);
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

    let stats = range_authorship(commit_range, false, &[], None).unwrap();

    // The two known-human lines added inside the range count as human
    // additions, not unknown additions.
    assert!(
        stats.range_stats.human_additions > 0,
        "known-human lines in the range should be counted as human additions, got {:?}",
        stats.range_stats
    );
    assert!(
        stats.range_stats.unknown_additions < stats.range_stats.human_additions,
        "unknown_additions should not absorb the known-human lines, got {:?}",
        stats.range_stats
    );
    assert_eq!(stats.authorship_stats.total_commits, 2);
    assert_eq!(stats.authorship_stats.commits_with_authorship, 2);
}
