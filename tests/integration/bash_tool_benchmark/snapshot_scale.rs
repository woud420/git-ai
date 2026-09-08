use super::{
    AgentId, BenchDaemon, Duration, DurationStats, Instant, bash_tool, create_synthetic_repo, fs,
    run_benchmark,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_bash_tool_snapshot_benchmark_small() {
    const FILE_COUNT: usize = 1_000;
    // Targets are ~50% higher than the old snapshot-only targets to account for
    // the JSON save/load I/O that handle_bash_tool adds to each hook event.
    const TARGET_PRE_P95_MS: f64 = 15.0;
    const TARGET_POST_P95_MS: f64 = 15.0;
    // CI margin: 10x to account for slow CI runners
    const CI_MARGIN: f64 = 10.0;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let repo_root = tmp.path().join("small_repo");

    println!("\n========================================");
    println!("Bash Tool Benchmark: SMALL ({} files)", FILE_COUNT);
    println!(
        "Target pre P95: < {}ms, post P95: < {}ms",
        TARGET_PRE_P95_MS, TARGET_POST_P95_MS
    );
    println!("(end-to-end handle_bash_tool: cleanup+walk+JSON I/O+diff)");
    println!("========================================");

    let setup_start = Instant::now();
    create_synthetic_repo(&repo_root, FILE_COUNT);
    println!(
        "Repo setup: {:.2}ms",
        setup_start.elapsed().as_secs_f64() * 1000.0
    );

    let daemon_home = tmp.path().join("small_daemon");
    let _daemon = BenchDaemon::start(&repo_root, &daemon_home);

    let (pre_stats, post_stats) = run_benchmark(&repo_root, "Small (1K)");

    let pre_p95_ms = pre_stats.p95.as_secs_f64() * 1000.0;
    let post_p95_ms = post_stats.p95.as_secs_f64() * 1000.0;
    println!(
        "\nSmall repo pre P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
        pre_p95_ms,
        TARGET_PRE_P95_MS,
        TARGET_PRE_P95_MS * CI_MARGIN,
    );
    println!(
        "Small repo post P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
        post_p95_ms,
        TARGET_POST_P95_MS,
        TARGET_POST_P95_MS * CI_MARGIN,
    );
    assert!(
        pre_p95_ms < TARGET_PRE_P95_MS * CI_MARGIN,
        "Small repo pre-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
        pre_p95_ms,
        TARGET_PRE_P95_MS * CI_MARGIN,
    );
    assert!(
        post_p95_ms < TARGET_POST_P95_MS * CI_MARGIN,
        "Small repo post-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
        post_p95_ms,
        TARGET_POST_P95_MS * CI_MARGIN,
    );
}

#[test]
#[ignore]
fn test_bash_tool_snapshot_benchmark_medium() {
    const FILE_COUNT: usize = 10_000;
    const TARGET_PRE_P95_MS: f64 = 75.0;
    const TARGET_POST_P95_MS: f64 = 75.0;
    const CI_MARGIN: f64 = 10.0;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let repo_root = tmp.path().join("medium_repo");

    println!("\n========================================");
    println!("Bash Tool Benchmark: MEDIUM ({} files)", FILE_COUNT);
    println!(
        "Target pre P95: < {}ms, post P95: < {}ms",
        TARGET_PRE_P95_MS, TARGET_POST_P95_MS
    );
    println!("(end-to-end handle_bash_tool: cleanup+walk+JSON I/O+diff)");
    println!("========================================");

    let setup_start = Instant::now();
    create_synthetic_repo(&repo_root, FILE_COUNT);
    println!(
        "Repo setup: {:.2}ms",
        setup_start.elapsed().as_secs_f64() * 1000.0
    );

    let daemon_home = tmp.path().join("medium_daemon");
    let _daemon = BenchDaemon::start(&repo_root, &daemon_home);

    let (pre_stats, post_stats) = run_benchmark(&repo_root, "Medium (10K)");

    let pre_p95_ms = pre_stats.p95.as_secs_f64() * 1000.0;
    let post_p95_ms = post_stats.p95.as_secs_f64() * 1000.0;
    println!(
        "\nMedium repo pre P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
        pre_p95_ms,
        TARGET_PRE_P95_MS,
        TARGET_PRE_P95_MS * CI_MARGIN,
    );
    println!(
        "Medium repo post P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
        post_p95_ms,
        TARGET_POST_P95_MS,
        TARGET_POST_P95_MS * CI_MARGIN,
    );
    assert!(
        pre_p95_ms < TARGET_PRE_P95_MS * CI_MARGIN,
        "Medium repo pre-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
        pre_p95_ms,
        TARGET_PRE_P95_MS * CI_MARGIN,
    );
    assert!(
        post_p95_ms < TARGET_POST_P95_MS * CI_MARGIN,
        "Medium repo post-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
        post_p95_ms,
        TARGET_POST_P95_MS * CI_MARGIN,
    );
}

#[test]
#[ignore]
fn test_bash_tool_snapshot_benchmark_large() {
    const FILE_COUNT: usize = 100_000;
    const TARGET_PRE_P95_MS: f64 = 750.0;
    const TARGET_POST_P95_MS: f64 = 750.0;
    const CI_MARGIN: f64 = 10.0;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let repo_root = tmp.path().join("large_repo");

    println!("\n========================================");
    println!("Bash Tool Benchmark: LARGE ({} files)", FILE_COUNT);
    println!(
        "Target pre P95: < {}ms, post P95: < {}ms",
        TARGET_PRE_P95_MS, TARGET_POST_P95_MS
    );
    println!("(end-to-end handle_bash_tool: cleanup+walk+JSON I/O+diff)");
    println!("========================================");

    let setup_start = Instant::now();
    create_synthetic_repo(&repo_root, FILE_COUNT);
    println!("Repo setup: {:.2}s", setup_start.elapsed().as_secs_f64());

    let daemon_home = tmp.path().join("large_daemon");
    let _daemon = BenchDaemon::start(&repo_root, &daemon_home);

    let (pre_stats, post_stats) = run_benchmark(&repo_root, "Large (100K)");

    let pre_p95_ms = pre_stats.p95.as_secs_f64() * 1000.0;
    let post_p95_ms = post_stats.p95.as_secs_f64() * 1000.0;
    println!(
        "\nLarge repo pre P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
        pre_p95_ms,
        TARGET_PRE_P95_MS,
        TARGET_PRE_P95_MS * CI_MARGIN,
    );
    println!(
        "Large repo post P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
        post_p95_ms,
        TARGET_POST_P95_MS,
        TARGET_POST_P95_MS * CI_MARGIN,
    );
    assert!(
        pre_p95_ms < TARGET_PRE_P95_MS * CI_MARGIN,
        "Large repo pre-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
        pre_p95_ms,
        TARGET_PRE_P95_MS * CI_MARGIN,
    );
    assert!(
        post_p95_ms < TARGET_POST_P95_MS * CI_MARGIN,
        "Large repo post-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
        post_p95_ms,
        TARGET_POST_P95_MS * CI_MARGIN,
    );
}

#[test]
#[ignore]
fn test_bash_tool_snapshot_benchmark_xlarge() {
    // This test creates 500K files and is too slow for CI.  It validates
    // graceful degradation: handle_bash_tool should either complete within the
    // timeout budget or degrade gracefully (error path is fast).
    const FILE_COUNT: usize = 500_000;
    const TARGET_PRE_P95_MS: f64 = 7_500.0;
    const TARGET_POST_P95_MS: f64 = 7_500.0;
    const CI_MARGIN: f64 = 4.0;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let repo_root = tmp.path().join("xlarge_repo");

    println!("\n========================================");
    println!("Bash Tool Benchmark: XLARGE ({} files)", FILE_COUNT);
    println!(
        "Target pre/post P95: < {}ms (with graceful degradation)",
        TARGET_PRE_P95_MS
    );
    println!("WARNING: This test creates 500K files and may take several minutes to set up.");
    println!("========================================");

    let setup_start = Instant::now();
    create_synthetic_repo(&repo_root, FILE_COUNT);
    println!("Repo setup: {:.2}s", setup_start.elapsed().as_secs_f64());

    let daemon_home = tmp.path().join("xlarge_daemon");
    let _daemon = BenchDaemon::start(&repo_root, &daemon_home);

    // For XLarge we run fewer iterations since setup is so expensive.
    println!("\n--- XLarge benchmark (3 iterations) ---");
    let mut pre_durations: Vec<Duration> = Vec::new();
    let mut post_durations: Vec<Duration> = Vec::new();
    let session_id = "bench-session-xl";

    for i in 1..=3 {
        let tool_use_id = format!("xl-{}", i);

        let agent_id = AgentId {
            tool: "bench".to_string(),
            id: "bench".to_string(),
            model: String::new(),
        };

        let pre_start = Instant::now();
        let pre_result = bash_tool::handle_bash_pre_tool_use_with_context(
            &repo_root,
            session_id,
            &tool_use_id,
            &agent_id,
            None,
            "t_test123456789a",
            None,
        );
        let pre_elapsed = pre_start.elapsed();

        match pre_result {
            Ok(_) => {
                println!(
                    "  Iteration {} pre-hook: {:.2}ms",
                    i,
                    pre_elapsed.as_secs_f64() * 1000.0,
                );
                pre_durations.push(pre_elapsed);
            }
            Err(e) => {
                // Graceful degradation: verify failure was fast (no spin).
                println!(
                    "  Iteration {} pre-hook: error after {:.2}ms -- {} (graceful degradation)",
                    i,
                    pre_elapsed.as_secs_f64() * 1000.0,
                    e,
                );
                assert!(
                    pre_elapsed < Duration::from_secs(10),
                    "Graceful degradation should be fast; took {:.2}s",
                    pre_elapsed.as_secs_f64(),
                );
                return;
            }
        }

        // Modify a file so the diff has something to find
        let marker = repo_root.join("bench_marker.txt");
        fs::write(&marker, format!("xl iteration {}", i)).expect("failed to write marker");

        let post_start = Instant::now();
        let post_result = bash_tool::handle_bash_post_tool_use(
            &repo_root,
            session_id,
            &tool_use_id,
            &agent_id,
            None,
            "t_test123456789a",
            None,
        );
        let post_elapsed = post_start.elapsed();
        let _ = fs::remove_file(&marker);

        match post_result {
            Ok(_) => {
                println!(
                    "  Iteration {} post-hook: {:.2}ms",
                    i,
                    post_elapsed.as_secs_f64() * 1000.0,
                );
                post_durations.push(post_elapsed);
            }
            Err(e) => {
                println!(
                    "  Iteration {} post-hook: error after {:.2}ms -- {} (graceful degradation)",
                    i,
                    post_elapsed.as_secs_f64() * 1000.0,
                    e,
                );
                assert!(
                    post_elapsed < Duration::from_secs(10),
                    "Graceful degradation should be fast; took {:.2}s",
                    post_elapsed.as_secs_f64(),
                );
                return;
            }
        }
    }

    if !pre_durations.is_empty() {
        let pre_stats = DurationStats::from_durations(&pre_durations);
        let post_stats = DurationStats::from_durations(&post_durations);
        pre_stats.print("XLarge (500K) Pre-hook");
        post_stats.print("XLarge (500K) Post-hook");

        let pre_p95_ms = pre_stats.p95.as_secs_f64() * 1000.0;
        let post_p95_ms = post_stats.p95.as_secs_f64() * 1000.0;
        println!(
            "\nXLarge repo pre P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
            pre_p95_ms,
            TARGET_PRE_P95_MS,
            TARGET_PRE_P95_MS * CI_MARGIN,
        );
        println!(
            "XLarge repo post P95: {:.2}ms (target: {}ms, CI limit: {}ms)",
            post_p95_ms,
            TARGET_POST_P95_MS,
            TARGET_POST_P95_MS * CI_MARGIN,
        );
        if pre_p95_ms > TARGET_PRE_P95_MS {
            println!(
                "WARNING: XLarge pre P95 ({:.2}ms) exceeded ideal target -- acceptable for large repos",
                pre_p95_ms,
            );
        }
        assert!(
            pre_p95_ms < TARGET_PRE_P95_MS * CI_MARGIN,
            "XLarge repo pre-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
            pre_p95_ms,
            TARGET_PRE_P95_MS * CI_MARGIN,
        );
        assert!(
            post_p95_ms < TARGET_POST_P95_MS * CI_MARGIN,
            "XLarge repo post-hook P95 ({:.2}ms) exceeded CI limit ({}ms)",
            post_p95_ms,
            TARGET_POST_P95_MS * CI_MARGIN,
        );
    }
}
