use super::{DeterministicRng, ExpectedLineExt, Instant, TestRepo};

#[test]
fn deterministic_rng_sequence_and_ranges_are_stable() {
    let mut rng = DeterministicRng(42);
    assert_eq!(rng.next(), 45_454_805_674);
    assert_eq!(rng.next(), 11_532_217_803_599_905_471);
    assert_eq!(rng.next(), 10_021_416_941_527_320_954);
    assert_eq!(rng.gen_range(0), 0);
    assert_eq!(rng.gen_range(1), 0);
    assert_eq!(rng.gen_range(10), 7);
}

/// Benchmark: large rebase with many AI-authored commits
/// This simulates the real-world scenario reported by users in large monorepos
/// where rebases with AI authorship notes become extremely slow.
///
/// The test creates:
/// - A main branch that advances with N commits
/// - A feature branch with M commits, each touching AI-authored files
/// - Rebases the feature branch onto the advanced main branch
///
/// Run with: cargo test --package git-ai --test integration rebase_benchmark -- --ignored --nocapture
#[test]
#[ignore]
fn benchmark_rebase_many_ai_commits() {
    let num_feature_commits: usize = std::env::var("REBASE_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let num_main_commits: usize = std::env::var("REBASE_BENCH_MAIN_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let num_ai_files: usize = std::env::var("REBASE_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let lines_per_file: usize = std::env::var("REBASE_BENCH_LINES_PER_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);

    println!("\n=== Rebase Benchmark Configuration ===");
    println!("Feature commits: {}", num_feature_commits);
    println!("Main commits: {}", num_main_commits);
    println!("AI files per commit: {}", num_ai_files);
    println!("Lines per file: {}", lines_per_file);
    println!("=========================================\n");

    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    // Create feature branch with many AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    let setup_start = Instant::now();

    for commit_idx in 0..num_feature_commits {
        // Each commit touches several AI-authored files
        for file_idx in 0..num_ai_files {
            let filename = format!("feature/module_{}/file_{}.rs", file_idx, file_idx);
            let mut file = repo.filename(&filename);

            // Build content with AI-authored lines that change each commit
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            for line_idx in 0..lines_per_file {
                let line_content = format!(
                    "// AI code v{} module {} line {}",
                    commit_idx, file_idx, line_idx
                );
                lines.push(line_content.ai());
            }
            file.set_contents(lines);
        }
        repo.stage_all_and_commit(&format!("AI feature commit {}", commit_idx))
            .unwrap();

        if (commit_idx + 1) % 10 == 0 {
            println!(
                "  Created feature commit {}/{} ({:.1}s)",
                commit_idx + 1,
                num_feature_commits,
                setup_start.elapsed().as_secs_f64()
            );
        }
    }

    let feature_setup_time = setup_start.elapsed();
    println!(
        "Feature branch setup: {:.1}s ({} commits)",
        feature_setup_time.as_secs_f64(),
        num_feature_commits
    );

    // Advance main branch with non-conflicting commits
    repo.git(&["checkout", &default_branch]).unwrap();
    let main_setup_start = Instant::now();

    for commit_idx in 0..num_main_commits {
        let filename = format!("main/change_{}.txt", commit_idx);
        let mut file = repo.filename(&filename);
        file.set_contents(crate::lines![format!("main content {}", commit_idx)]);
        repo.stage_all_and_commit(&format!("Main commit {}", commit_idx))
            .unwrap();
    }

    let main_setup_time = main_setup_start.elapsed();
    println!(
        "Main branch setup: {:.1}s ({} commits)",
        main_setup_time.as_secs_f64(),
        num_main_commits
    );

    // Now perform the rebase and measure time
    repo.git(&["checkout", "feature"]).unwrap();

    println!("\n--- Starting rebase ---");
    let rebase_start = Instant::now();
    let result = repo.git(&["rebase", &default_branch]);
    let rebase_duration = rebase_start.elapsed();

    match &result {
        Ok(output) => {
            println!("Rebase succeeded in {:.3}s", rebase_duration.as_secs_f64());
            println!("Output: {}", output);
        }
        Err(e) => {
            println!(
                "Rebase failed in {:.3}s: {}",
                rebase_duration.as_secs_f64(),
                e
            );
        }
    }
    result.unwrap();

    println!("\n=== BENCHMARK RESULTS ===");
    println!(
        "Total rebase time: {:.3}s ({:.0}ms)",
        rebase_duration.as_secs_f64(),
        rebase_duration.as_millis()
    );
    println!(
        "Per-commit average: {:.1}ms",
        rebase_duration.as_millis() as f64 / num_feature_commits as f64
    );
    println!("=========================\n");
}

/// Smaller benchmark for quick iteration during optimization
#[test]
#[ignore]
fn benchmark_rebase_small() {
    let num_commits = 10;
    let num_ai_files = 3;
    let lines_per_file = 20;

    let repo = TestRepo::new();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();

    for commit_idx in 0..num_commits {
        for file_idx in 0..num_ai_files {
            let filename = format!("feat/mod_{}/f_{}.rs", file_idx, file_idx);
            let mut file = repo.filename(&filename);
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            for line_idx in 0..lines_per_file {
                lines.push(format!("// AI v{} m{} l{}", commit_idx, file_idx, line_idx).ai());
            }
            file.set_contents(lines);
        }
        repo.stage_all_and_commit(&format!("feat {}", commit_idx))
            .unwrap();
    }

    repo.git(&["checkout", &default_branch]).unwrap();
    for i in 0..5 {
        let mut f = repo.filename(&format!("main_{}.txt", i));
        f.set_contents(crate::lines![format!("main {}", i)]);
        repo.stage_all_and_commit(&format!("main {}", i)).unwrap();
    }

    repo.git(&["checkout", "feature"]).unwrap();

    let start = Instant::now();
    repo.git(&["rebase", &default_branch]).unwrap();
    let dur = start.elapsed();

    println!("\n=== SMALL REBASE BENCHMARK ===");
    println!(
        "Commits: {}, AI files: {}, Lines/file: {}",
        num_commits, num_ai_files, lines_per_file
    );
    println!(
        "Total: {:.3}s ({:.0}ms)",
        dur.as_secs_f64(),
        dur.as_millis()
    );
    println!(
        "Per-commit: {:.1}ms",
        dur.as_millis() as f64 / num_commits as f64
    );
    println!("===============================\n");
}

/// Benchmark with performance JSON output for precise phase timing
#[test]
#[ignore]
fn benchmark_rebase_with_perf_json() {
    let num_commits: usize = std::env::var("REBASE_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let num_ai_files: usize = std::env::var("REBASE_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let repo = TestRepo::new();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();

    for commit_idx in 0..num_commits {
        for file_idx in 0..num_ai_files {
            let filename = format!("feat/mod_{}/f_{}.rs", file_idx, file_idx);
            let mut file = repo.filename(&filename);
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            for line_idx in 0..30 {
                lines.push(
                    format!(
                        "// AI code v{} mod{} line{}",
                        commit_idx, file_idx, line_idx
                    )
                    .ai(),
                );
            }
            file.set_contents(lines);
        }
        repo.stage_all_and_commit(&format!("feat {}", commit_idx))
            .unwrap();
    }

    repo.git(&["checkout", &default_branch]).unwrap();
    for i in 0..10 {
        let mut f = repo.filename(&format!("main_{}.txt", i));
        f.set_contents(crate::lines![format!("main {}", i)]);
        repo.stage_all_and_commit(&format!("main {}", i)).unwrap();
    }

    repo.git(&["checkout", "feature"]).unwrap();

    // Use benchmark_git to get performance JSON
    println!("\n--- Starting instrumented rebase ---");
    let start = Instant::now();
    let result = repo.benchmark_git(&["rebase", &default_branch]);
    let dur = start.elapsed();

    match result {
        Ok(bench) => {
            println!("\n=== INSTRUMENTED REBASE BENCHMARK ===");
            println!("Commits: {}, AI files: {}", num_commits, num_ai_files);
            println!("Total wall time: {:.3}s", dur.as_secs_f64());
            println!("Git duration: {:.3}s", bench.git_duration.as_secs_f64());
            println!(
                "Pre-command: {:.3}s",
                bench.pre_command_duration.as_secs_f64()
            );
            println!(
                "Post-command: {:.3}s",
                bench.post_command_duration.as_secs_f64()
            );
            println!(
                "Overhead: {:.3}s ({:.1}%)",
                (bench.total_duration - bench.git_duration).as_secs_f64(),
                ((bench.total_duration - bench.git_duration).as_millis() as f64
                    / bench.git_duration.as_millis().max(1) as f64)
                    * 100.0
            );
            println!("======================================\n");
        }
        Err(e) => {
            println!(
                "Benchmark result: {} (wall time: {:.3}s)",
                e,
                dur.as_secs_f64()
            );
            // Still useful even without structured perf data
        }
    }
}
