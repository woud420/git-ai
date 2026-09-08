use super::NUM_ITERATIONS;
use super::{Duration, DurationStats, Instant, bash_tool, create_synthetic_repo, fs};

#[test]
#[ignore]
fn test_bash_tool_diff_performance() {
    // Benchmarks the diff() function in isolation by building two large
    // in-memory snapshots and diffing them.
    const FILE_COUNT: usize = 10_000;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let repo_root = tmp.path().join("diff_bench_repo");

    println!("\n========================================");
    println!("Bash Tool Diff-Only Benchmark ({} files)", FILE_COUNT);
    println!("========================================");

    create_synthetic_repo(&repo_root, FILE_COUNT);

    // Touch all source files so their mtimes are newer than the backdated
    // .git/index watermark (set by create_synthetic_repo), making them visible to snapshot().
    let now_ft = filetime::FileTime::now();
    let mut dirs = vec![repo_root.clone()];
    while let Some(dir) = dirs.pop() {
        if dir.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        for entry in fs::read_dir(&dir).expect("read_dir").flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                let _ = filetime::set_file_mtime(&p, now_ft);
            }
        }
    }

    // Take a baseline snapshot.
    let pre = bash_tool::snapshot(&repo_root, "diff-bench", "pre", None)
        .expect("pre-snapshot should succeed");

    // Modify 1% of files to simulate realistic edits.
    let files_to_modify = FILE_COUNT / 100;
    let mut modified_count = 0;
    let mut dirs_to_visit = vec![repo_root.clone()];
    'outer: while let Some(dir) = dirs_to_visit.pop() {
        if dir.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        let entries = fs::read_dir(&dir).expect("failed to read dir");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs_to_visit.push(path);
            } else if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                fs::write(
                    &path,
                    format!("// modified\nfn modified_{}() {{}}\n", modified_count),
                )
                .expect("failed to modify file");
                modified_count += 1;
                if modified_count >= files_to_modify {
                    break 'outer;
                }
            }
        }
    }
    println!("Modified {} files for diff benchmark", modified_count);

    // Take a post-snapshot.
    let post = bash_tool::snapshot(&repo_root, "diff-bench", "post", None)
        .expect("post-snapshot should succeed");

    // Benchmark diff() over multiple iterations.
    println!(
        "\n--- Diff-only benchmark ({} iterations) ---",
        NUM_ITERATIONS
    );
    let mut diff_durations: Vec<Duration> = Vec::with_capacity(NUM_ITERATIONS);

    for i in 1..=NUM_ITERATIONS {
        let start = Instant::now();
        let result = bash_tool::diff(&pre, &post);
        let elapsed = start.elapsed();

        println!(
            "  Iteration {}: diff={:.4}ms (created={}, modified={})",
            i,
            elapsed.as_secs_f64() * 1000.0,
            result.created.len(),
            result.modified.len(),
        );

        // Sanity: we should see roughly the number of files we modified.
        assert!(
            result.modified.len() >= modified_count / 2,
            "Expected at least {} modified files, got {}",
            modified_count / 2,
            result.modified.len(),
        );

        diff_durations.push(elapsed);
    }

    let stats = DurationStats::from_durations(&diff_durations);
    stats.print("Diff-Only (10K files, 1% modified)");

    // Diff should be very fast since it is purely in-memory HashSet operations.
    let p95_ms = stats.p95.as_secs_f64() * 1000.0;
    assert!(
        p95_ms < 50.0,
        "Diff P95 ({:.2}ms) should be under 50ms for 10K entries",
        p95_ms,
    );
}

#[test]
#[ignore]
fn test_bash_tool_git_status_fallback_benchmark() {
    // Benchmarks git_status_fallback() which shells out to `git status`.
    const FILE_COUNT: usize = 10_000;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let repo_root = tmp.path().join("fallback_bench_repo");

    println!("\n========================================");
    println!(
        "Bash Tool git_status_fallback Benchmark ({} files)",
        FILE_COUNT
    );
    println!("========================================");

    create_synthetic_repo(&repo_root, FILE_COUNT);

    // Create some uncommitted changes so git status has something to report.
    fs::write(repo_root.join("new_file.txt"), "new content").expect("failed to write new file");
    let modify_target = repo_root
        .join("src_0")
        .join("mod_0")
        .join("pkg_0")
        .join("file_0.rs");
    if modify_target.exists() {
        fs::write(&modify_target, "// modified\n").expect("failed to modify file");
    }

    println!(
        "\n--- git_status_fallback benchmark ({} iterations) ---",
        NUM_ITERATIONS
    );
    let mut durations: Vec<Duration> = Vec::with_capacity(NUM_ITERATIONS);

    for i in 1..=NUM_ITERATIONS {
        let start = Instant::now();
        let result =
            bash_tool::git_status_fallback(&repo_root).expect("git_status_fallback should succeed");
        let elapsed = start.elapsed();

        println!(
            "  Iteration {}: {:.2}ms ({} changed files)",
            i,
            elapsed.as_secs_f64() * 1000.0,
            result.len(),
        );

        assert!(
            !result.is_empty(),
            "git_status_fallback should detect uncommitted changes"
        );

        durations.push(elapsed);
    }

    let stats = DurationStats::from_durations(&durations);
    stats.print("git_status_fallback (10K files)");
}

#[test]
#[ignore]
fn test_bash_tool_snapshot_entry_count_accuracy() {
    // Verify that the snapshot captures exactly the files modified after the
    // watermark.  create_synthetic_repo backdates .git/index by 30 s, so
    // pre-existing files are covered; only files written after that appear.
    const NEW_FILE_COUNT: usize = 10;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let repo_root = tmp.path().join("accuracy_repo");

    println!("\n========================================");
    println!("Bash Tool Snapshot Accuracy ({} new files)", NEW_FILE_COUNT);
    println!("========================================");

    create_synthetic_repo(&repo_root, 100); // small base repo

    // Write NEW_FILE_COUNT new (untracked) files after the backdated watermark
    // (create_synthetic_repo backdates .git/index by 30s, so new files are clearly outside
    // the 2s grace window and appear in the snapshot).
    for i in 0..NEW_FILE_COUNT {
        fs::write(
            repo_root.join(format!("new_file_{}.txt", i)),
            format!("content {}", i),
        )
        .expect("failed to write new file");
    }

    let snap = bash_tool::snapshot(&repo_root, "accuracy", "check", None)
        .expect("snapshot should succeed");

    let entry_count = snap.entries.len();
    println!("Snapshot entries: {}", entry_count);

    assert!(
        entry_count >= NEW_FILE_COUNT,
        "Expected at least {} snapshot entries (the new files), got {}",
        NEW_FILE_COUNT,
        entry_count,
    );

    // All new files must be present; the pre-existing .rs files must not be.
    for i in 0..NEW_FILE_COUNT {
        let rel = std::path::PathBuf::from(format!("new_file_{}.txt", i));
        assert!(
            snap.entries.contains_key(&rel),
            "new_file_{}.txt should appear in snapshot",
            i,
        );
    }
}
