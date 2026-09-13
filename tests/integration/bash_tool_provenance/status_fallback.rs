use super::*;

// ===========================================================================
// Additional: Direct snapshot/diff API tests with real commands
// ===========================================================================

#[test]
fn test_bash_provenance_snapshot_diff_echo_redirect() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    let pre = snapshot(&root, "snap-echo", "t1", None).expect("pre-snapshot should succeed");

    run_bash(&repo, "sh", &["-c", "echo 'snap test' > snap_created.txt"]);

    let post = snapshot(&root, "snap-echo", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        created.iter().any(|p| p.contains("snap_created.txt")),
        "snap_created.txt should appear in created via direct snapshot/diff; got {:?}",
        created
    );
    assert!(
        result.modified.is_empty(),
        "no files should be modified; got {:?}",
        result.modified
    );
}

#[test]
fn test_bash_provenance_snapshot_diff_sed_modification() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "editable.txt", "old text old text", "initial commit");

    let pre = snapshot(&root, "snap-sed", "t1", None).expect("pre-snapshot should succeed");

    thread::sleep(Duration::from_millis(50));
    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "sed -i.bak 's/old/new/g' editable.txt && rm -f editable.txt.bak",
        ],
    );

    let post = snapshot(&root, "snap-sed", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let modified: Vec<String> = result
        .modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        modified.iter().any(|p| p.contains("editable.txt")),
        "editable.txt should appear in modified via direct snapshot/diff; got {:?}",
        modified
    );
}

// ───────────────────────────────────────────────────────────────────
// 13. git_status_fallback parsing correctness
// ───────────────────────────────────────────────────────────────────

#[test]
fn test_git_status_fallback_files_with_spaces() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create and track a file with spaces in its name
    add_and_commit(&repo, "file with spaces.txt", "original", "add spaced file");

    // Modify it so git status reports it
    repo.write_file("file with spaces.txt", "modified");

    let changed = git_status_fallback(&root).unwrap();
    assert!(
        changed.iter().any(|p| p == "file with spaces.txt"),
        "git_status_fallback should return full path with spaces; got {:?}",
        changed
    );
}

#[test]
fn test_git_status_fallback_new_untracked_with_spaces() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create an untracked file with spaces
    repo.write_file("my new file.rs", "content");

    let changed = git_status_fallback(&root).unwrap();
    assert!(
        changed.iter().any(|p| p == "my new file.rs"),
        "git_status_fallback should return full untracked path with spaces; got {:?}",
        changed
    );
}

#[test]
fn test_git_status_fallback_rename_reports_both_paths() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create and track a file, then rename it (staged rename)
    add_and_commit(&repo, "before.txt", "content", "add file");
    std::fs::rename(root.join("before.txt"), root.join("after.txt")).unwrap();
    repo.git_og(&["add", "-A"]).expect("git add should succeed");

    let changed = git_status_fallback(&root).unwrap();
    assert!(
        changed.iter().any(|p| p == "after.txt"),
        "git_status_fallback should report new rename path; got {:?}",
        changed
    );
    assert!(
        changed.iter().any(|p| p == "before.txt"),
        "git_status_fallback should report original rename path for attribution preservation; got {:?}",
        changed
    );
}
