use super::*;

#[test]
fn test_bash_tool_empty_stat_diff() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");
    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(
        result.is_empty(),
        "empty stat-diff should produce no changes"
    );
    assert!(result.all_changed_paths().is_empty());
}

// ===========================================================================
// cleanup_stale_snapshots
// ===========================================================================

// test_cleanup_stale_snapshots_does_not_error_on_empty was removed:
// cleanup_stale_snapshots has been deleted from the codebase.

// ===========================================================================
// normalize_path consistency
// ===========================================================================

#[test]
fn test_normalize_path_idempotent() {
    let path = Path::new("src/lib.rs");
    let once = normalize_path(path);
    let twice = normalize_path(&once);
    assert_eq!(once, twice, "normalize_path should be idempotent");
}

#[test]
fn test_normalize_path_handles_nested() {
    let path = Path::new("deeply/nested/dir/file.rs");
    let normalized = normalize_path(path);
    // On any platform, normalizing twice should give the same result
    assert_eq!(normalized, normalize_path(&normalized));
}

// ===========================================================================
// Snapshot invocation key
// ===========================================================================

#[test]
fn test_snapshot_invocation_key_format() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    let snap = snapshot(&root, "my-session", "my-tool", None).expect("snapshot should succeed");
    assert_eq!(
        snap.invocation_key, "my-session:my-tool",
        "invocation_key should be session_id:tool_use_id"
    );
}

// ===========================================================================
// DiffResult helpers
// ===========================================================================

#[test]
fn test_diff_result_all_changed_paths_combines_categories() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "modify.txt", "original", "initial");
    add_and_commit(&repo, "delete.txt", "doomed", "add delete target");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    thread::sleep(Duration::from_millis(50));
    repo.write_file("modify.txt", "changed");
    repo.write_file("create.txt", "new");
    fs::remove_file(repo.path().join("delete.txt")).expect("delete should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let all = result.all_changed_paths();
    // Deletions are not tracked; only modify.txt and create.txt are reported.
    assert!(
        all.len() >= 2,
        "Should have at least 2 changed paths; got {}",
        all.len()
    );
    assert!(all.iter().any(|p| p.contains("modify.txt")));
    assert!(all.iter().any(|p| p.contains("create.txt")));
}

#[test]
fn test_diff_result_is_empty_true_when_no_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");
    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(result.is_empty());
    assert!(result.all_changed_paths().is_empty());
}

// ===========================================================================
// normalize_path — case folding
// ===========================================================================

#[test]
fn test_normalize_path_case_folding() {
    let mixed = Path::new("Src/Main.RS");
    let normalized = normalize_path(mixed);
    // On macOS/Windows, should be lowercased; on Linux, unchanged
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        assert_eq!(
            normalized,
            PathBuf::from("src/main.rs"),
            "normalize_path should lowercase on case-insensitive platforms"
        );
    } else {
        assert_eq!(
            normalized,
            PathBuf::from("Src/Main.RS"),
            "normalize_path should preserve case on case-sensitive platforms"
        );
    }
}

// ===========================================================================
// StatDiffResult::is_empty with single non-empty category
// ===========================================================================

#[test]
fn test_stat_diff_result_is_empty_single_category() {
    let created_only = StatDiffResult {
        created: vec![PathBuf::from("new.txt")],
        modified: vec![],
    };
    assert!(!created_only.is_empty());

    let modified_only = StatDiffResult {
        created: vec![],
        modified: vec![PathBuf::from("changed.txt")],
    };
    assert!(!modified_only.is_empty());

    assert!(StatDiffResult::default().is_empty());
}

// ===========================================================================
// StatEntry — symlink file type
// ===========================================================================

#[cfg(unix)]
#[test]
fn test_stat_entry_symlink_type() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let target = tmp.path().join("target.txt");
    let link = tmp.path().join("link.txt");
    fs::write(&target, "target content").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let meta = fs::symlink_metadata(&link).unwrap();
    let entry = StatEntry::from_metadata(&meta);
    assert_eq!(entry.file_type, StatFileType::Symlink);
    assert!(entry.exists);
}

// ===========================================================================
// StatEntry — ctime is populated
// ===========================================================================

#[test]
fn test_stat_entry_has_ctime() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    fs::write(tmp.path(), "hello").unwrap();
    let meta = fs::symlink_metadata(tmp.path()).unwrap();
    let entry = StatEntry::from_metadata(&meta);
    assert!(
        entry.ctime.is_some(),
        "ctime should be populated on real files"
    );
}

// ===========================================================================
// Snapshot — hidden files (dotfiles) are included
// ===========================================================================

#[test]
fn test_snapshot_includes_hidden_files() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, ".hidden_config", "secret=val", "add hidden file");

    let snap = snapshot(&root, "sess", "t1", None).expect("snapshot should succeed");
    assert!(
        snap.entries
            .keys()
            .any(|p| p.display().to_string().contains(".hidden_config")),
        "snapshot should include hidden (dotfiles); got keys: {:?}",
        snap.entries.keys().collect::<Vec<_>>()
    );
}

// ===========================================================================
// Walker error — permission denied on subdirectory
// ===========================================================================

#[cfg(unix)]
#[test]
fn test_snapshot_handles_permission_denied_directory() {
    use std::os::unix::fs::PermissionsExt;

    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "accessible.txt", "ok", "initial");
    add_and_commit(&repo, "restricted/file.txt", "restricted", "add restricted");

    // Remove read/execute permission on the restricted directory
    let restricted_dir = repo.path().join("restricted");
    let mut perms = fs::metadata(&restricted_dir).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&restricted_dir, perms).expect("chmod should succeed");

    // Snapshot should still succeed (walker errors are skipped)
    let snap = snapshot(&root, "sess", "t1", None);

    // Restore permissions before assertion (for cleanup)
    let mut perms = fs::metadata(&restricted_dir)
        .unwrap_or_else(|_| fs::symlink_metadata(&restricted_dir).unwrap())
        .permissions();
    perms.set_mode(0o755);
    let _ = fs::set_permissions(&restricted_dir, perms);

    let snap = snap.expect("snapshot should succeed despite permission errors");
    // accessible.txt should be in the snapshot
    assert!(
        snap.entries
            .keys()
            .any(|p| p.display().to_string().contains("accessible.txt")),
        "accessible.txt should be in snapshot"
    );
}
