use super::*;

// ===========================================================================
// Section 5.3 — Edge Cases
// ===========================================================================

#[test]
fn test_bash_tool_files_outside_repo_ignored() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "inside.txt", "inside", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Modify a file outside the repo — this should not be detected.
    let outside = std::env::temp_dir().join("bash_tool_test_outside.txt");
    fs::write(&outside, "external change").expect("write outside repo should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(
        result.is_empty(),
        "changes outside the repo should not appear in the diff"
    );

    // Clean up
    let _ = fs::remove_file(&outside);
}

// ===========================================================================
// Gitignore Filtering
// ===========================================================================

#[test]
fn test_bash_tool_gitignore_excludes_new_untracked_files() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create a .gitignore that ignores *.log files, then commit it
    add_and_commit(&repo, ".gitignore", "*.log\n", "add gitignore");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Create both an ignored and a non-ignored file
    repo.write_file("debug.log", "log output");
    repo.write_file("result.txt", "result data");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        created.iter().any(|p| p.contains("result.txt")),
        "result.txt should be created; got {:?}",
        created
    );
    assert!(
        !created.iter().any(|p| p.contains("debug.log")),
        "debug.log should be excluded by gitignore; got {:?}",
        created
    );
}

#[test]
fn test_bash_tool_gitignore_excludes_directory_patterns() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Use glob patterns that match files (not just directory-trailing patterns),
    // since the snapshot walker checks individual file paths with is_dir=false.
    add_and_commit(
        &repo,
        ".gitignore",
        "*.o\n*.pyc\ntarget/\n",
        "add gitignore",
    );

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Create files matching glob-based ignore patterns
    repo.write_file("build/output.o", "binary");
    repo.write_file("cache/module.pyc", "bytecode");
    // Also create a non-ignored file
    repo.write_file("src/main.rs", "fn main() {}");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        created
            .iter()
            .any(|p| p.contains("src/main.rs") || p.contains("src\\main.rs")),
        "src/main.rs should be created; got {:?}",
        created
    );
    assert!(
        !created.iter().any(|p| p.contains("output.o")),
        "*.o files should be excluded by gitignore; got {:?}",
        created
    );
    assert!(
        !created.iter().any(|p| p.contains("module.pyc")),
        "*.pyc files should be excluded by gitignore; got {:?}",
        created
    );
}

// ===========================================================================
// build_gitignore
// ===========================================================================

#[test]
fn test_build_gitignore_parses_rules() {
    // build_gitignore covers git-ai-specific patterns only (defaults,
    // .git-ai-ignore, linguist-generated).  Standard .gitignore rules are
    // handled by WalkBuilder with git_ignore(true); they are NOT loaded into
    // the Gitignore returned here.
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, ".gitignore", "*.tmp\ntarget/\n", "add gitignore");

    let gitignore = build_gitignore(&root).expect("build_gitignore should succeed");

    // git-ai default patterns should be present (*.lock is in DEFAULT_IGNORE_PATTERNS)
    assert!(
        gitignore
            .matched(Path::new("Cargo.lock"), false)
            .is_ignore(),
        "Cargo.lock should match git-ai default patterns"
    );

    // Standard .gitignore rules (*.tmp) are NOT in build_gitignore — the
    // walker handles those via git_ignore(true).
    assert!(
        !gitignore.matched(Path::new("data.tmp"), false).is_ignore(),
        "*.tmp is in .gitignore but not in build_gitignore; walker handles it"
    );

    // .rs files should not be ignored
    assert!(
        !gitignore.matched(Path::new("main.rs"), false).is_ignore(),
        "*.rs should not match any git-ai default patterns"
    );
}

// ===========================================================================
// Nested subdirectory .gitignore
// ===========================================================================

// test_build_gitignore_nested_subdirectory_rules and
// test_build_gitignore_deeply_nested_rules were removed: they tested the old
// collect_gitignores pre-walk which loaded nested .gitignore files into the
// Gitignore returned by build_gitignore.  That behavior was removed because
// the pre-walk could not apply rules during traversal, so gitignored dirs
// outside the hardcoded skip list were still descended into.  Nested
// .gitignore support now lives entirely in WalkBuilder (git_ignore(true)),
// which applies rules correctly during the walk.  The equivalent coverage is
// provided by test_snapshot_nested_gitignore_excludes_matching_new_files and
// test_snapshot_walker_prunes_ignored_directories.

#[test]
fn test_snapshot_walker_prunes_ignored_directories() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Gitignore that ignores an entire directory (like node_modules/)
    add_and_commit(&repo, ".gitignore", "ignored_dir/\n", "ignore a directory");
    add_and_commit(&repo, "tracked.txt", "tracked", "add tracked file");

    // Create the ignored directory with many files
    let ignored_dir = root.join("ignored_dir");
    fs::create_dir_all(&ignored_dir).expect("create ignored dir");
    for i in 0..100 {
        fs::write(ignored_dir.join(format!("file_{}.txt", i)), "noise").expect("write file");
    }

    let snap = snapshot(&root, "sess", "t1", None).expect("snapshot should succeed");

    // Tracked file should be in the snapshot
    assert!(
        snap.entries
            .keys()
            .any(|p| p.display().to_string().contains("tracked.txt")),
        "tracked.txt should be in snapshot"
    );

    // None of the ignored_dir files should be in the snapshot
    let ignored_count = snap
        .entries
        .keys()
        .filter(|p| p.display().to_string().contains("ignored_dir"))
        .count();
    assert_eq!(
        ignored_count, 0,
        "files in ignored_dir/ should not appear in snapshot"
    );
}

#[test]
fn test_snapshot_nested_gitignore_excludes_matching_new_files() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, ".gitignore", "", "root gitignore");
    add_and_commit(&repo, "src/.gitignore", "*.generated\n", "nested gitignore");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Create both an ignored and a non-ignored file under src/
    repo.write_file("src/output.generated", "generated code");
    repo.write_file("src/real.rs", "fn real() {}");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        created.iter().any(|p| p.contains("real.rs")),
        "real.rs should be created; got {:?}",
        created
    );
    assert!(
        !created.iter().any(|p| p.contains("output.generated")),
        "output.generated should be excluded by nested gitignore; got {:?}",
        created
    );
}

// ===========================================================================
// Snapshot save/load round-trip and snapshot consumption
// ===========================================================================

// test_snapshot_save_load_round_trip was removed:
// save_snapshot and load_and_consume_snapshot have been deleted from the codebase.

// test_gitignore_filtering_through_save_load_round_trip was removed:
// save_snapshot and load_and_consume_snapshot have been deleted from the codebase.
// Gitignore filtering is still tested via the snapshot/diff tests above.

// ===========================================================================
// Stale snapshot cleanup — actually removes old snapshots
// ===========================================================================

// test_cleanup_stale_snapshots_removes_old_files was removed:
// cleanup_stale_snapshots and save_snapshot have been deleted from the codebase.

// ===========================================================================
// diff with gitignore=None passes all new files through
// ===========================================================================

#[test]
fn test_diff_no_gitignore_includes_all_new_files() {
    let now = SystemTime::now();
    let pre = StatSnapshot {
        entries: HashMap::new(),
        taken_at: None,
        invocation_key: "test:1".to_string(),
        repo_root: PathBuf::from("/tmp"),
        effective_worktree_wm: None,
        per_file_wm: HashMap::new(),
    };

    let mut post_entries = HashMap::new();
    // A file that would normally be gitignored (*.log)
    post_entries.insert(
        normalize_path(Path::new("debug.log")),
        StatEntry {
            exists: true,
            mtime: Some(now),
            ctime: Some(now),
            size: 100,
            mode: 0o644,
            file_type: StatFileType::Regular,
        },
    );
    // A normal file
    post_entries.insert(
        normalize_path(Path::new("main.rs")),
        StatEntry {
            exists: true,
            mtime: Some(now),
            ctime: Some(now),
            size: 50,
            mode: 0o644,
            file_type: StatFileType::Regular,
        },
    );

    let post = StatSnapshot {
        entries: post_entries,
        taken_at: None,
        invocation_key: "test:2".to_string(),
        repo_root: PathBuf::from("/tmp"),
        effective_worktree_wm: None,
        per_file_wm: HashMap::new(),
    };

    let result = diff(&pre, &post);
    // Both files appear as created (filter applied at snapshot time, not in diff).
    assert_eq!(
        result.created.len(),
        2,
        "Both files should be created when gitignore is None; got {:?}",
        result.created
    );
}
