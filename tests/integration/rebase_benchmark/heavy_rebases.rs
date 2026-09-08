use super::{ExpectedLineExt, Instant, TestRepo, fs};

/// HEAVY benchmark designed to stress-test rebase performance at scale.
///
/// This creates a realistic monorepo-style scenario:
/// - 50 AI-tracked files across multiple modules (200-500 lines each)
/// - 200 feature commits, EVERY commit touches ALL AI files (no skipping)
/// - Every single change has AI attribution (checkpoint for each file in each commit)
/// - Main branch also modifies the same AI-tracked files (forces slow path)
/// - 20 main branch commits creating content conflicts that shift line ranges
///
/// This ensures:
/// 1. No fast-path shortcuts (blob OIDs differ due to main branch changes)
/// 2. Every commit must have its attribution rewritten (100% AI content)
/// 3. Line attribution transfer must handle shifting ranges
/// 4. Large note payloads (50 files × many line ranges per commit)
///
/// Run with: cargo test --package git-ai --test integration benchmark_rebase_heavy -- --ignored --nocapture
#[test]
#[ignore]
fn benchmark_rebase_heavy() {
    let num_ai_files: usize = std::env::var("HEAVY_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let lines_per_file: usize = std::env::var("HEAVY_BENCH_LINES_PER_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let num_feature_commits: usize = std::env::var("HEAVY_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let num_main_commits: usize = std::env::var("HEAVY_BENCH_MAIN_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let files_per_commit: usize = std::env::var("HEAVY_BENCH_FILES_PER_COMMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(num_ai_files); // default: touch ALL files every commit

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║             HEAVY REBASE BENCHMARK                      ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!(
        "║  AI files:            {:<10}                        ║",
        num_ai_files
    );
    println!(
        "║  Lines per file:      {:<10}                        ║",
        lines_per_file
    );
    println!(
        "║  Feature commits:     {:<10}                        ║",
        num_feature_commits
    );
    println!(
        "║  Main commits:        {:<10}                        ║",
        num_main_commits
    );
    println!(
        "║  Files per commit:    {:<10}                        ║",
        files_per_commit
    );
    println!(
        "║  Total initial lines: {:<10}                        ║",
        num_ai_files * lines_per_file
    );
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let repo = TestRepo::new();
    let setup_start = Instant::now();

    // Step 1: Create initial commit with all AI-tracked files
    {
        for file_idx in 0..num_ai_files {
            let module = file_idx % 10;
            let filename = format!("src/modules/mod_{}/component_{}.rs", module, file_idx);
            let mut file = repo.filename(&filename);
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            // Header region (will be modified by main branch)
            lines.push(
                format!(
                    "// Module {} Component {} - Auto-generated",
                    module, file_idx
                )
                .into(),
            );
            lines.push("// MAIN_INSERTION_POINT".into());
            lines.push(format!("pub mod component_{} {{", file_idx).into());
            // AI-generated body
            for line_idx in 0..lines_per_file {
                let line = format!(
                    "    pub fn func_{}_{}() -> i32 {{ {} }} // AI generated",
                    file_idx,
                    line_idx,
                    line_idx * file_idx + 1
                );
                lines.push(line.ai());
            }
            lines.push("} // end module".into());
            file.set_contents(lines);
        }
        repo.stage_all_and_commit("Initial: all AI-tracked files")
            .unwrap();
    }
    println!(
        "Initial commit: {:.1}s",
        setup_start.elapsed().as_secs_f64()
    );

    let default_branch = repo.current_branch();

    // Step 2: Create feature branch with many AI commits
    // EVERY commit touches files and EVERY change has AI attribution
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let feature_start = Instant::now();

    for commit_idx in 0..num_feature_commits {
        let start_file = (commit_idx * 3) % num_ai_files;
        for i in 0..files_per_commit {
            let file_idx = (start_file + i) % num_ai_files;
            let module = file_idx % 10;
            let filename = format!("src/modules/mod_{}/component_{}.rs", module, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();

            // Append AI-authored code at end (before closing brace)
            let new_content = current.replacen(
                "} // end module",
                &format!(
                    "    pub fn feature_{}_in_comp_{}() -> String {{ String::from(\"v{}\") }} // AI commit {}\n}} // end module",
                    commit_idx, file_idx, commit_idx, commit_idx
                ),
                1,
            );
            fs::write(&path, &new_content).unwrap();
            // Checkpoint EVERY file as AI-authored
            repo.git_ai(&["checkpoint", "mock_ai_agent", &filename])
                .unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("AI feature commit {}", commit_idx))
            .unwrap();

        if (commit_idx + 1) % 25 == 0 {
            println!(
                "  Feature commit {}/{} ({:.1}s, {:.0}ms/commit)",
                commit_idx + 1,
                num_feature_commits,
                feature_start.elapsed().as_secs_f64(),
                feature_start.elapsed().as_millis() as f64 / (commit_idx + 1) as f64,
            );
        }
    }
    println!(
        "Feature branch setup: {:.1}s ({} commits, {:.0}ms/commit)",
        feature_start.elapsed().as_secs_f64(),
        num_feature_commits,
        feature_start.elapsed().as_millis() as f64 / num_feature_commits as f64,
    );

    // Step 3: Advance main branch - modify the SAME AI-tracked files
    // This forces the slow path because blob OIDs will differ after rebase
    repo.git(&["checkout", &default_branch]).unwrap();
    let main_start = Instant::now();

    for main_idx in 0..num_main_commits {
        // Each main commit modifies a rotating set of AI files at the header
        let files_per_main = (num_ai_files / 2).max(5);
        let start_file = (main_idx * 7) % num_ai_files;
        for i in 0..files_per_main {
            let file_idx = (start_file + i) % num_ai_files;
            let module = file_idx % 10;
            let filename = format!("src/modules/mod_{}/component_{}.rs", module, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            // Insert at the MAIN_INSERTION_POINT - this shifts ALL line numbers
            let new_content = current.replacen(
                "// MAIN_INSERTION_POINT",
                &format!(
                    "// Main branch change {} in component {}\n// Added config: SETTING_{}={}\n// MAIN_INSERTION_POINT",
                    main_idx, file_idx, main_idx, file_idx
                ),
                1,
            );
            fs::write(&path, &new_content).unwrap();
        }
        // Also add unrelated files for realism
        for i in 0..3 {
            let filename = format!("docs/main_change_{}_{}.md", main_idx, i);
            let mut file = repo.filename(&filename);
            file.set_contents(crate::lines![format!("Main doc {} {}", main_idx, i)]);
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("Main change {}", main_idx))
            .unwrap();
    }
    println!(
        "Main branch setup: {:.1}s ({} commits)",
        main_start.elapsed().as_secs_f64(),
        num_main_commits,
    );
    println!(
        "Total setup time: {:.1}s",
        setup_start.elapsed().as_secs_f64()
    );

    // Step 4: Rebase feature onto main with full instrumentation
    repo.git(&["checkout", "feature"]).unwrap();

    let timing_file = repo.path().join("..").join("heavy_rebase_timing.txt");

    println!(
        "\n━━━ Starting HEAVY rebase ({} commits onto {}) ━━━",
        num_feature_commits, default_branch
    );
    let wall_start = Instant::now();

    // Use benchmark_git for structured timing (captures pre/git/post breakdown)
    let bench_result = repo.benchmark_git(&["rebase", &default_branch]);
    let wall_duration = wall_start.elapsed();

    match &bench_result {
        Ok(bench) => {
            let git_ms = bench.git_duration.as_millis();
            let total_ms = bench.total_duration.as_millis();
            let pre_ms = bench.pre_command_duration.as_millis();
            let post_ms = bench.post_command_duration.as_millis();
            let overhead_ms = total_ms.saturating_sub(git_ms);
            let overhead_pct = if git_ms > 0 {
                overhead_ms as f64 / git_ms as f64 * 100.0
            } else {
                0.0
            };

            println!("\n╔══════════════════════════════════════════════════════════╗");
            println!("║            HEAVY BENCHMARK RESULTS                      ║");
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║  Configuration:                                         ║");
            println!(
                "║    AI files:          {}                            ",
                num_ai_files
            );
            println!(
                "║    Lines/file:        {}                           ",
                lines_per_file
            );
            println!(
                "║    Feature commits:   {}                           ",
                num_feature_commits
            );
            println!(
                "║    Main commits:      {}                           ",
                num_main_commits
            );
            println!(
                "║    Files/commit:      {}                           ",
                files_per_commit
            );
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║  Timing:                                                ║");
            println!(
                "║    Wall time:         {:.3}s                       ",
                wall_duration.as_secs_f64()
            );
            println!(
                "║    Total (wrapper):   {}ms                        ",
                total_ms
            );
            println!(
                "║    Git rebase:        {}ms                        ",
                git_ms
            );
            println!(
                "║    Pre-command:       {}ms                        ",
                pre_ms
            );
            println!(
                "║    Post-command:      {}ms                        ",
                post_ms
            );
            println!(
                "║    Overhead:          {}ms ({:.1}% of git)        ",
                overhead_ms, overhead_pct
            );
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║  Per-commit averages:                                   ║");
            println!(
                "║    Total:             {:.1}ms                     ",
                total_ms as f64 / num_feature_commits as f64
            );
            println!(
                "║    Git:               {:.1}ms                     ",
                git_ms as f64 / num_feature_commits as f64
            );
            println!(
                "║    Overhead:          {:.1}ms                     ",
                overhead_ms as f64 / num_feature_commits as f64
            );
            println!("╚══════════════════════════════════════════════════════════╝\n");
        }
        Err(e) => {
            println!(
                "Benchmark failed after {:.3}s: {}",
                wall_duration.as_secs_f64(),
                e
            );
            panic!("Heavy benchmark failed: {}", e);
        }
    }

    // Also read timing file if available
    if let Ok(timing_data) = fs::read_to_string(&timing_file) {
        println!("=== PHASE TIMING BREAKDOWN ===");
        print!("{}", timing_data);
        println!("===============================\n");
    }
}

/// Same as heavy benchmark but with timing file output for phase analysis
#[test]
#[ignore]
fn benchmark_rebase_heavy_with_timing() {
    let num_ai_files: usize = std::env::var("HEAVY_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let lines_per_file: usize = std::env::var("HEAVY_BENCH_LINES_PER_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let num_feature_commits: usize = std::env::var("HEAVY_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let num_main_commits: usize = std::env::var("HEAVY_BENCH_MAIN_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);

    println!("\n=== Heavy Rebase Benchmark (with timing) ===");
    println!(
        "AI files: {}, Lines/file: {}, Feature commits: {}, Main commits: {}",
        num_ai_files, lines_per_file, num_feature_commits, num_main_commits
    );
    println!("=============================================\n");

    let repo = TestRepo::new();

    // Create initial files
    for file_idx in 0..num_ai_files {
        let module = file_idx % 8;
        let filename = format!("src/mod_{}/file_{}.rs", module, file_idx);
        let mut file = repo.filename(&filename);
        let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
        lines.push(format!("// File {} header", file_idx).into());
        lines.push("// MAIN_MARKER".into());
        for line_idx in 0..lines_per_file {
            lines.push(format!("fn f_{}_{}() {{ /* AI */ }}", file_idx, line_idx).ai());
        }
        lines.push("// EOF".into());
        file.set_contents(lines);
    }
    repo.stage_all_and_commit("Initial AI files").unwrap();
    let default_branch = repo.current_branch();

    // Feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let feature_start = Instant::now();
    for commit_idx in 0..num_feature_commits {
        for file_idx in 0..num_ai_files {
            let module = file_idx % 8;
            let filename = format!("src/mod_{}/file_{}.rs", module, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            let new_content = current.replacen(
                "// EOF",
                &format!(
                    "fn feat_{}_{}() {{ /* AI v{} */ }}\n// EOF",
                    commit_idx, file_idx, commit_idx
                ),
                1,
            );
            fs::write(&path, &new_content).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &filename]).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("feat {}", commit_idx))
            .unwrap();
        if (commit_idx + 1) % 20 == 0 {
            println!(
                "  Feature {}/{} ({:.1}s)",
                commit_idx + 1,
                num_feature_commits,
                feature_start.elapsed().as_secs_f64()
            );
        }
    }
    println!(
        "Feature setup: {:.1}s",
        feature_start.elapsed().as_secs_f64()
    );

    // Main branch modifications
    repo.git(&["checkout", &default_branch]).unwrap();
    for main_idx in 0..num_main_commits {
        for file_idx in 0..num_ai_files {
            let module = file_idx % 8;
            let filename = format!("src/mod_{}/file_{}.rs", module, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            let new_content = current.replacen(
                "// MAIN_MARKER",
                &format!("// main change {} f{}\n// MAIN_MARKER", main_idx, file_idx),
                1,
            );
            fs::write(&path, &new_content).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("main {}", main_idx))
            .unwrap();
    }

    // Rebase with timing
    repo.git(&["checkout", "feature"]).unwrap();
    let timing_file = repo.path().join("..").join("heavy_timing.txt");
    let timing_path = timing_file.to_str().unwrap().to_string();

    println!("\n--- Starting rebase ---");
    let start = Instant::now();
    let result = repo.git_with_env(
        &["rebase", &default_branch],
        &[
            ("GIT_AI_DEBUG_PERFORMANCE", "2"),
            ("GIT_AI_REBASE_TIMING_FILE", &timing_path),
        ],
        None,
    );
    let dur = start.elapsed();

    match &result {
        Ok(_) => println!("Rebase succeeded in {:.3}s", dur.as_secs_f64()),
        Err(e) => println!("Rebase FAILED in {:.3}s: {}", dur.as_secs_f64(), e),
    }
    result.unwrap();

    if let Ok(timing_data) = fs::read_to_string(&timing_file) {
        println!("\n=== PHASE TIMING ===");
        print!("{}", timing_data);
        println!("====================\n");
    }

    println!(
        "Total: {:.3}s, Per-commit: {:.1}ms",
        dur.as_secs_f64(),
        dur.as_millis() as f64 / num_feature_commits as f64
    );
}
