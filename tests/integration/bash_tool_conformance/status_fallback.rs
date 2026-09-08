use super::{TestRepo, add_and_commit, fs, git_status_fallback, repo_root};

// ===========================================================================
// git_status_fallback
// ===========================================================================

#[test]
fn test_git_status_fallback_detects_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "tracked.txt", "original", "initial");
    repo.write_file("tracked.txt", "modified");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");

    assert!(
        changed.iter().any(|p| p.contains("tracked.txt")),
        "git_status_fallback should report tracked.txt; got {:?}",
        changed
    );
}

#[test]
fn test_git_status_fallback_detects_untracked() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Make an initial commit so we have a valid repo
    add_and_commit(&repo, "base.txt", "base", "init");
    repo.write_file("untracked.txt", "new file");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");

    assert!(
        changed.iter().any(|p| p.contains("untracked.txt")),
        "git_status_fallback should report untracked.txt; got {:?}",
        changed
    );
}

#[test]
fn test_git_status_fallback_clean_repo() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "clean.txt", "clean", "initial");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.is_empty(),
        "clean repo should report no changes; got {:?}",
        changed
    );
}

// ===========================================================================
// git_status_fallback — unmerged/conflict files (u prefix)
// ===========================================================================

#[test]
fn test_git_status_fallback_merge_conflict() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create a file on main branch
    add_and_commit(&repo, "conflict.txt", "main content", "initial");

    // Create a branch, modify the file, commit
    repo.git_og(&["checkout", "-b", "feature"])
        .expect("checkout should succeed");
    repo.write_file("conflict.txt", "feature content");
    repo.git_og(&["add", "conflict.txt"])
        .expect("add should succeed");
    repo.git_og(&["commit", "-m", "feature change"])
        .expect("commit should succeed");

    // Go back to main, modify the same file differently, commit
    repo.git_og(&["checkout", "master"])
        .or_else(|_| repo.git_og(&["checkout", "main"]))
        .expect("checkout main should succeed");
    repo.write_file("conflict.txt", "main diverged content");
    repo.git_og(&["add", "conflict.txt"])
        .expect("add should succeed");
    repo.git_og(&["commit", "-m", "main diverged"])
        .expect("commit should succeed");

    // Attempt merge — this should produce a conflict
    let merge_result = repo.git_og(&["merge", "feature", "--no-edit"]);
    // If merge succeeds (auto-resolved), skip the test
    if merge_result.is_ok() {
        return; // Auto-resolved, no conflict to test
    }

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.iter().any(|p| p.contains("conflict.txt")),
        "git_status_fallback should report conflicted file; got {:?}",
        changed
    );
}

// ===========================================================================
// git_status_fallback — staged deletion
// ===========================================================================

#[test]
fn test_git_status_fallback_staged_deletion() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "to-delete.txt", "content", "initial");
    repo.git_og(&["rm", "to-delete.txt"])
        .expect("git rm should succeed");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.iter().any(|p| p.contains("to-delete.txt")),
        "git_status_fallback should report staged deletion; got {:?}",
        changed
    );
}

// ===========================================================================
// git_status_fallback — rename with spaces in both paths
// ===========================================================================

#[test]
fn test_git_status_fallback_rename_with_spaces() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "old file name.txt", "content", "add spaced file");
    fs::rename(
        root.join("old file name.txt"),
        root.join("new file name.txt"),
    )
    .expect("rename should succeed");
    repo.git_og(&["add", "-A"]).expect("git add should succeed");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.iter().any(|p| p == "new file name.txt"),
        "should report new path with spaces; got {:?}",
        changed
    );
    assert!(
        changed.iter().any(|p| p == "old file name.txt"),
        "should report original path with spaces; got {:?}",
        changed
    );
}
