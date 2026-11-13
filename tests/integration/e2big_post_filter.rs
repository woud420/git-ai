use crate::repos::test_repo::TestRepo;

use git_ai::operations::git::repository::MAX_PATHSPEC_ARGS;
use git_ai::operations::git::repository::find_repository_in_path;
use std::collections::HashSet;

/// Pad a set of real paths with non-existent paths to exceed MAX_PATHSPEC_ARGS.
/// Fake paths never match anything, so results should be identical to the CLI-arg path.
fn padded_pathspecs(real_paths: &[&str]) -> HashSet<String> {
    let mut set: HashSet<String> = real_paths.iter().map(|s| s.to_string()).collect();
    let needed = MAX_PATHSPEC_ARGS + 1 - set.len();
    for i in 0..needed {
        set.insert(format!("nonexistent/padding_{:04}.txt", i));
    }
    assert!(set.len() > MAX_PATHSPEC_ARGS);
    set
}

/// Create N numbered files in a test repo, stage them, and return filenames.
fn create_files(
    repo: &TestRepo,
    count: usize,
    content_fn: impl Fn(usize) -> String,
) -> Vec<String> {
    let mut filenames = Vec::new();
    for i in 0..count {
        let name = format!("file_{}.txt", i);
        let content = content_fn(i);
        std::fs::write(repo.path().join(&name), &content).unwrap();
        filenames.push(name);
    }
    filenames
}

// ============================================================
// Test Group A: status()
// ============================================================

// ============================================================
// Test Group B: list_commit_files()
// ============================================================

#[test]
fn test_list_commit_files_post_filter_equivalence() {
    let repo = TestRepo::new();

    // Create and commit 5 files
    let filenames = create_files(&repo, 5, |i| format!("content {}", i));
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "initial"]).unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = gitai_repo.head().unwrap().target().unwrap();

    // Small pathspec (CLI-arg path)
    let small: HashSet<String> = filenames.iter().cloned().collect();
    let result_small = gitai_repo
        .list_commit_files(&head_sha, Some(&small))
        .unwrap();

    // Padded pathspec (post-filter path)
    let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
    let large = padded_pathspecs(&refs);
    let result_large = gitai_repo
        .list_commit_files(&head_sha, Some(&large))
        .unwrap();

    assert_eq!(
        result_small, result_large,
        "list_commit_files results should be identical"
    );
}

#[test]
fn test_list_commit_files_post_filter_exclusion() {
    let repo = TestRepo::new();

    // Create and commit 5 files
    let filenames = create_files(&repo, 5, |i| format!("content {}", i));
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "initial"]).unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = gitai_repo.head().unwrap().target().unwrap();

    // Padded pathspec containing only 2 of the 5 files
    let subset = padded_pathspecs(&[&filenames[0], &filenames[1]]);
    let result = gitai_repo
        .list_commit_files(&head_sha, Some(&subset))
        .unwrap();

    let expected: HashSet<String> = [filenames[0].clone(), filenames[1].clone()]
        .into_iter()
        .collect();
    assert_eq!(result, expected, "should contain only file_0 and file_1");
}

#[test]
fn test_list_commit_files_post_filter_no_matches() {
    let repo = TestRepo::new();

    // Create and commit 5 files
    create_files(&repo, 5, |i| format!("content {}", i));
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "initial"]).unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = gitai_repo.head().unwrap().target().unwrap();

    // Padded pathspec with ALL fake paths (none matching real files)
    let mut all_fake: HashSet<String> = HashSet::new();
    for i in 0..=MAX_PATHSPEC_ARGS {
        all_fake.insert(format!("nonexistent/fake_{:04}.txt", i));
    }
    assert!(all_fake.len() > MAX_PATHSPEC_ARGS);

    let result = gitai_repo
        .list_commit_files(&head_sha, Some(&all_fake))
        .unwrap();
    assert!(
        result.is_empty(),
        "should return empty when no pathspecs match"
    );
}

// ============================================================
// Test Group C: diff_added_lines()
// ============================================================

#[test]
fn test_diff_added_lines_post_filter_equivalence() {
    let repo = TestRepo::new();

    // commit1: 5 files with "line1"
    let filenames = create_files(&repo, 5, |_| "line1\n".to_string());
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "commit1"]).unwrap();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let sha1 = gitai_repo.head().unwrap().target().unwrap();

    // commit2: modify 3 files (append "line2")
    for name in &filenames[0..3] {
        std::fs::write(repo.path().join(name), "line1\nline2\n").unwrap();
    }
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "commit2"]).unwrap();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let sha2 = gitai_repo.head().unwrap().target().unwrap();

    // Small pathspec (all 5 files)
    let small: HashSet<String> = filenames.iter().cloned().collect();
    let result_small = gitai_repo
        .diff_added_lines(&sha1, &sha2, Some(&small))
        .unwrap();

    // Padded pathspec
    let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
    let large = padded_pathspecs(&refs);
    let result_large = gitai_repo
        .diff_added_lines(&sha1, &sha2, Some(&large))
        .unwrap();

    assert_eq!(
        result_small, result_large,
        "diff_added_lines results should be identical"
    );
}

#[test]
fn test_diff_added_lines_post_filter_exclusion() {
    let repo = TestRepo::new();

    // commit1: 5 files with "line1"
    let filenames = create_files(&repo, 5, |_| "line1\n".to_string());
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "commit1"]).unwrap();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let sha1 = gitai_repo.head().unwrap().target().unwrap();

    // commit2: modify 3 files
    for name in &filenames[0..3] {
        std::fs::write(repo.path().join(name), "line1\nline2\n").unwrap();
    }
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "commit2"]).unwrap();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let sha2 = gitai_repo.head().unwrap().target().unwrap();

    // Padded pathspec containing only 1 of the 3 modified files
    let subset = padded_pathspecs(&[&filenames[0]]);
    let result = gitai_repo
        .diff_added_lines(&sha1, &sha2, Some(&subset))
        .unwrap();

    assert_eq!(result.len(), 1, "should have exactly 1 file");
    assert!(result.contains_key(&filenames[0]), "should contain file_0");
    assert!(
        !result.contains_key(&filenames[1]),
        "should NOT contain file_1"
    );
    assert!(
        !result.contains_key(&filenames[2]),
        "should NOT contain file_2"
    );
}

#[test]
fn test_diff_added_lines_post_filter_correct_line_numbers() {
    let repo = TestRepo::new();

    // commit1: a.txt = "L1\nL2\nL3\n"
    std::fs::write(repo.path().join("a.txt"), "L1\nL2\nL3\n").unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "commit1"]).unwrap();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let sha1 = gitai_repo.head().unwrap().target().unwrap();

    // commit2: a.txt = "L1\nL2\nL3\nL4\nL5\n"
    std::fs::write(repo.path().join("a.txt"), "L1\nL2\nL3\nL4\nL5\n").unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "commit2"]).unwrap();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let sha2 = gitai_repo.head().unwrap().target().unwrap();

    // Padded pathspec
    let pathspecs = padded_pathspecs(&["a.txt"]);
    let result = gitai_repo
        .diff_added_lines(&sha1, &sha2, Some(&pathspecs))
        .unwrap();

    assert!(result.contains_key("a.txt"), "should contain a.txt");
    assert_eq!(
        result["a.txt"],
        vec![4, 5],
        "should report lines 4 and 5 as added"
    );
}

// ============================================================
// Test Group D: diff_workdir_added_lines_with_insertions()
// ============================================================

#[test]
fn test_diff_workdir_insertions_post_filter_equivalence() {
    let repo = TestRepo::new();

    // Commit 3 files
    let filenames = create_files(&repo, 3, |i| format!("line1_{}\n", i));
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "initial"]).unwrap();

    // Modify 2 in working dir (don't commit)
    std::fs::write(repo.path().join(&filenames[0]), "line1_0\nline2_0\n").unwrap();
    std::fs::write(repo.path().join(&filenames[1]), "line1_1\nline2_1\n").unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = gitai_repo.head().unwrap().target().unwrap();

    // Small pathspec
    let small: HashSet<String> = filenames.iter().cloned().collect();
    let (all_small, ins_small) = gitai_repo
        .diff_workdir_added_lines_with_insertions(&head_sha, Some(&small))
        .unwrap();

    // Padded pathspec
    let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
    let large = padded_pathspecs(&refs);
    let (all_large, ins_large) = gitai_repo
        .diff_workdir_added_lines_with_insertions(&head_sha, Some(&large))
        .unwrap();

    assert_eq!(all_small, all_large, "all_added should be identical");
    assert_eq!(ins_small, ins_large, "pure_insertions should be identical");
}

#[test]
fn test_diff_workdir_insertions_both_maps_filtered() {
    let repo = TestRepo::new();

    // Commit a.txt and b.txt
    std::fs::write(repo.path().join("a.txt"), "a_line1\n").unwrap();
    std::fs::write(repo.path().join("b.txt"), "b_line1\n").unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "initial"]).unwrap();

    // Append lines to both in working dir
    std::fs::write(repo.path().join("a.txt"), "a_line1\na_line2\n").unwrap();
    std::fs::write(repo.path().join("b.txt"), "b_line1\nb_line2\n").unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = gitai_repo.head().unwrap().target().unwrap();

    // Padded pathspec containing only "a.txt"
    let pathspecs = padded_pathspecs(&["a.txt"]);
    let (all_added, pure_insertions) = gitai_repo
        .diff_workdir_added_lines_with_insertions(&head_sha, Some(&pathspecs))
        .unwrap();

    assert!(
        all_added.contains_key("a.txt"),
        "all_added should have a.txt"
    );
    assert!(
        !all_added.contains_key("b.txt"),
        "all_added should NOT have b.txt"
    );
    assert!(
        !pure_insertions.contains_key("b.txt"),
        "pure_insertions should NOT have b.txt"
    );
}

// ============================================================
// Test Group F: Boundary & edge cases
// ============================================================

#[test]
fn test_threshold_boundary_1000_vs_1001() {
    let repo = TestRepo::new();

    // Commit 3 files
    let filenames = create_files(&repo, 3, |i| format!("content_{}\n", i));
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "initial"]).unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = gitai_repo.head().unwrap().target().unwrap();

    // Exactly 1000 entries (3 real + 997 fake) → NOT greater, should use CLI-arg path
    let mut exactly_1000: HashSet<String> = filenames.iter().cloned().collect();
    for i in 0..(MAX_PATHSPEC_ARGS - filenames.len()) {
        exactly_1000.insert(format!("nonexistent/pad_{:04}.txt", i));
    }
    assert_eq!(exactly_1000.len(), MAX_PATHSPEC_ARGS);

    // Exactly 1001 entries (3 real + 998 fake) → greater, should use post-filter path
    let mut exactly_1001 = exactly_1000.clone();
    exactly_1001.insert("nonexistent/extra.txt".to_string());
    assert_eq!(exactly_1001.len(), MAX_PATHSPEC_ARGS + 1);

    let result_1000 = gitai_repo
        .list_commit_files(&head_sha, Some(&exactly_1000))
        .unwrap();
    let result_1001 = gitai_repo
        .list_commit_files(&head_sha, Some(&exactly_1001))
        .unwrap();

    assert_eq!(
        result_1000, result_1001,
        "results at boundary (1000 vs 1001) should be identical"
    );
    // Both should return the 3 real files
    let expected: HashSet<String> = filenames.into_iter().collect();
    assert_eq!(result_1000, expected);
}

#[test]
fn test_empty_pathspec_early_return() {
    let repo = TestRepo::new();

    // Commit a file
    std::fs::write(repo.path().join("a.txt"), "content\n").unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "initial"]).unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = gitai_repo.head().unwrap().target().unwrap();

    // Modify the file to create a diff
    std::fs::write(repo.path().join("a.txt"), "content\nnew line\n").unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "second"]).unwrap();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let sha2 = gitai_repo.head().unwrap().target().unwrap();

    // Empty pathspec should return empty HashMap immediately
    let empty: HashSet<String> = HashSet::new();
    let result = gitai_repo
        .diff_added_lines(&head_sha, &sha2, Some(&empty))
        .unwrap();
    assert!(
        result.is_empty(),
        "empty pathspec should return empty result"
    );
}

crate::reuse_tests_in_worktree!(
    test_list_commit_files_post_filter_equivalence,
    test_list_commit_files_post_filter_exclusion,
    test_list_commit_files_post_filter_no_matches,
    test_diff_added_lines_post_filter_equivalence,
    test_diff_added_lines_post_filter_exclusion,
    test_diff_added_lines_post_filter_correct_line_numbers,
    test_diff_workdir_insertions_post_filter_equivalence,
    test_diff_workdir_insertions_both_maps_filtered,
    test_threshold_boundary_1000_vs_1001,
    test_empty_pathspec_early_return,
);
