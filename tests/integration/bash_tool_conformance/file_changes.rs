use super::{Duration, TestRepo, add_and_commit, diff, fs, repo_root, snapshot, thread};

// ===========================================================================
// Section 5.1 — File Mutations
// ===========================================================================

#[test]
fn test_bash_tool_detect_file_creation() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    repo.write_file("new.txt", "hello");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        created.iter().any(|p| p.contains("new.txt")),
        "new.txt should appear in created; got {:?}",
        created
    );
    assert!(result.modified.is_empty(), "no files should be modified");
}

#[test]
fn test_bash_tool_detect_modification() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "existing.txt", "foo", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Allow filesystem time granularity to advance so the stat-tuple changes.
    thread::sleep(Duration::from_millis(50));
    repo.write_file("existing.txt", "bar");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let modified: Vec<String> = result
        .modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        modified.iter().any(|p| p.contains("existing.txt")),
        "existing.txt should appear in modified; got {:?}",
        modified
    );
    assert!(result.created.is_empty(), "no files should be created");
}

#[cfg(unix)]
#[test]
fn test_bash_tool_detect_permission_change() {
    use std::os::unix::fs::PermissionsExt;

    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "script.sh", "#!/bin/bash\necho hi", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // chmod +x
    let abs = repo.path().join("script.sh");
    let mut perms = fs::metadata(&abs).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&abs, perms).expect("chmod should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let modified: Vec<String> = result
        .modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        modified.iter().any(|p| p.contains("script.sh")),
        "script.sh should appear in modified after chmod; got {:?}",
        modified
    );
}

#[test]
fn test_bash_tool_detect_rename() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "old.txt", "data", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    fs::rename(repo.path().join("old.txt"), repo.path().join("new.txt"))
        .expect("rename should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    // After rename: old.txt no longer exists (deletion not tracked), new.txt appears as created.
    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        created.iter().any(|p| p.contains("new.txt")),
        "new.txt should appear in created after rename; got {:?}",
        created
    );
}

#[test]
fn test_bash_tool_detect_copy() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "source.txt", "copy-me", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    fs::copy(repo.path().join("source.txt"), repo.path().join("dest.txt"))
        .expect("copy should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        created.iter().any(|p| p.contains("dest.txt")),
        "dest.txt should appear in created (or modified) after copy; got {:?}",
        created
    );
    // source.txt should NOT appear as modified since we only read it
    let modified: Vec<String> = result
        .modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        !modified.iter().any(|p| p.contains("source.txt")),
        "source.txt should not be modified by a copy; got {:?}",
        modified
    );
}

// ===========================================================================
// Section 5.2 — Read-Only Operations
// ===========================================================================

#[test]
fn test_bash_tool_no_changes_detected() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "stable.txt", "unchanged", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");
    // No mutations between snapshots.
    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(
        result.is_empty(),
        "diff should be empty when nothing changed"
    );
    assert!(result.created.is_empty());
    assert!(result.modified.is_empty());
}

#[test]
fn test_bash_tool_empty_repo_no_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");
    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(result.is_empty(), "empty repo diff should be empty");
}

#[test]
fn test_bash_tool_multiple_mutations_combined() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "modify-me.txt", "original", "initial");
    add_and_commit(&repo, "delete-me.txt", "gone-soon", "add delete target");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Perform multiple mutations
    thread::sleep(Duration::from_millis(50));
    repo.write_file("modify-me.txt", "changed");
    repo.write_file("brand-new.txt", "fresh");
    fs::remove_file(repo.path().join("delete-me.txt")).expect("delete should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(
        !result.is_empty(),
        "diff should not be empty after multiple mutations"
    );

    let all_paths = result.all_changed_paths();
    assert!(
        all_paths.iter().any(|p| p.contains("modify-me.txt")),
        "modify-me.txt should be in changed paths; got {:?}",
        all_paths
    );
    assert!(
        all_paths.iter().any(|p| p.contains("brand-new.txt")),
        "brand-new.txt should be in changed paths; got {:?}",
        all_paths
    );
    // delete-me.txt is not tracked (deletions are not reported)
}

// ===========================================================================
// Subdirectory file operations
// ===========================================================================

#[test]
fn test_bash_tool_detect_file_in_subdirectory() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "src/lib.rs", "pub fn foo() {}", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    thread::sleep(Duration::from_millis(50));
    repo.write_file("src/lib.rs", "pub fn bar() {}");
    repo.write_file("src/nested/deep/module.rs", "mod deep;");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let all = result.all_changed_paths();
    assert!(
        all.iter()
            .any(|p| p.contains("src/lib.rs") || p.contains("src\\lib.rs")),
        "src/lib.rs should be detected; got {:?}",
        all
    );
    assert!(
        all.iter().any(|p| p.contains("module.rs")),
        "deeply nested module.rs should be detected; got {:?}",
        all
    );
}
