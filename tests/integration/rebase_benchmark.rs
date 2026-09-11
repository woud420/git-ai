use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use std::fs;
use std::time::Instant;

// Deterministic xorshift64 PRNG keeps benchmark workloads reproducible without rand.
struct DeterministicRng(u64);

impl DeterministicRng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn gen_range(&mut self, max: usize) -> usize {
        (self.next() as usize) % max.max(1)
    }
}

fn extract_timing(data: &str, key: &str) -> Option<u64> {
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key)
            && let Some(val) = trimmed.split('=').nth(1)
        {
            return val.trim_end_matches("ms").parse().ok();
        }
    }
    None
}

mod diff_reconstruction;
mod heavy_rebases;

mod monorepo_history;
mod plumbing_history;
mod realistic_monorepo;

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

/// Large-scale benchmark with mixed file sizes for PR comparison.
///
/// Creates:
/// - 200 AI-tracked files (150 × 1000 lines, 50 × 5000 lines)
/// - 150 feature commits, each modifying all files (ensuring AI attribution on every commit)
/// - Main branch also modifies the same files (forces diff-based path, not blob-copy fast path)
///
/// Run with: cargo test --package git-ai --test integration benchmark_large_scale_mixed -- --ignored --nocapture
#[test]
#[ignore]
fn benchmark_large_scale_mixed() {
    let num_small_files: usize = std::env::var("BENCH_SMALL_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let num_large_files: usize = std::env::var("BENCH_LARGE_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let small_file_lines: usize = std::env::var("BENCH_SMALL_LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let large_file_lines: usize = std::env::var("BENCH_LARGE_LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    let num_feature_commits: usize = std::env::var("BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let num_main_commits: usize = std::env::var("BENCH_MAIN_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let total_files = num_small_files + num_large_files;
    let total_initial_lines =
        num_small_files * small_file_lines + num_large_files * large_file_lines;

    println!("\n=== Large-Scale Mixed Benchmark ===");
    println!(
        "Small files: {} × {} lines",
        num_small_files, small_file_lines
    );
    println!(
        "Large files: {} × {} lines",
        num_large_files, large_file_lines
    );
    println!("Total files: {}", total_files);
    println!("Total initial lines: {}", total_initial_lines);
    println!("Feature commits: {}", num_feature_commits);
    println!("Main commits: {}", num_main_commits);
    println!("====================================\n");

    let repo = TestRepo::new();
    let setup_start = Instant::now();

    // Create initial commit with all files
    {
        for file_idx in 0..total_files {
            let lines_for_file = if file_idx < num_small_files {
                small_file_lines
            } else {
                large_file_lines
            };
            let filename = format!("src/mod_{}/file_{}.rs", file_idx % 20, file_idx);
            let mut file = repo.filename(&filename);
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            lines.push(format!("// Module {} header", file_idx).into());
            lines.push("// MAIN_MARKER".into());
            for line_idx in 0..lines_for_file {
                lines.push(
                    format!(
                        "fn func_{}_{}() {{ /* AI generated */ }}",
                        file_idx, line_idx
                    )
                    .ai(),
                );
            }
            file.set_contents(lines);
        }
        repo.stage_all_and_commit("Initial: all AI files").unwrap();
    }
    println!(
        "Initial commit setup: {:.1}s",
        setup_start.elapsed().as_secs_f64()
    );

    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let feature_start = Instant::now();

    for commit_idx in 0..num_feature_commits {
        // Each commit modifies a subset of files (rotating window of ~20 files)
        // but touches enough to exercise the diff path
        let files_per_commit = 20.min(total_files);
        let start_file = (commit_idx * 7) % total_files; // rotating start to vary which files

        for i in 0..files_per_commit {
            let file_idx = (start_file + i) % total_files;
            let filename = format!("src/mod_{}/file_{}.rs", file_idx % 20, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            // Append AI-authored line at the end
            let new_content = format!(
                "{}\nfn feature_{}_in_{}() {{ /* AI commit {} */ }}",
                current, commit_idx, file_idx, commit_idx
            );
            fs::write(&path, &new_content).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &filename]).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("AI feature {}", commit_idx))
            .unwrap();

        if (commit_idx + 1) % 25 == 0 {
            println!(
                "  Feature commit {}/{} ({:.1}s)",
                commit_idx + 1,
                num_feature_commits,
                feature_start.elapsed().as_secs_f64()
            );
        }
    }
    println!(
        "Feature branch setup: {:.1}s ({} commits)",
        feature_start.elapsed().as_secs_f64(),
        num_feature_commits
    );

    // Advance main branch — modify AI-tracked files to force diff-based path
    repo.git(&["checkout", &default_branch]).unwrap();
    let main_start = Instant::now();
    for main_idx in 0..num_main_commits {
        // Main modifies a different rotating set of files at the MARKER line
        let files_per_main = 30.min(total_files);
        let start_file = (main_idx * 13) % total_files;
        for i in 0..files_per_main {
            let file_idx = (start_file + i) % total_files;
            let filename = format!("src/mod_{}/file_{}.rs", file_idx % 20, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            let new_content = current.replacen(
                "// MAIN_MARKER",
                &format!(
                    "// Main change {} in file {}\n// MAIN_MARKER",
                    main_idx, file_idx
                ),
                1,
            );
            fs::write(&path, &new_content).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("Main {}", main_idx))
            .unwrap();
    }
    // Add unrelated main commits
    for i in 0..5 {
        let mut f = repo.filename(&format!("main_only/f_{}.txt", i));
        f.set_contents(crate::lines![format!("main only {}", i)]);
        repo.stage_all_and_commit(&format!("Main unrelated {}", i))
            .unwrap();
    }
    println!(
        "Main branch setup: {:.1}s",
        main_start.elapsed().as_secs_f64()
    );
    println!("Total setup: {:.1}s", setup_start.elapsed().as_secs_f64());

    // Rebase using benchmark_git for structured timing
    repo.git(&["checkout", "feature"]).unwrap();

    println!(
        "\n--- Starting rebase ({} commits onto {}) ---",
        num_feature_commits, default_branch
    );
    let wall_start = Instant::now();
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
            println!("║          LARGE-SCALE BENCHMARK RESULTS                  ║");
            println!("╠══════════════════════════════════════════════════════════╣");
            println!(
                "║  Files:          {} ({} × {}L + {} × {}L)",
                total_files, num_small_files, small_file_lines, num_large_files, large_file_lines
            );
            println!("║  Initial lines:  {}", total_initial_lines);
            println!("║  Commits:        {}", num_feature_commits);
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║  Wall time:      {:.3}s", wall_duration.as_secs_f64());
            println!("║  Total (wrapper): {}ms", total_ms);
            println!("║  Git rebase:     {}ms", git_ms);
            println!("║  Pre-command:    {}ms", pre_ms);
            println!("║  Post-command:   {}ms", post_ms);
            println!(
                "║  Overhead:       {}ms ({:.1}% of git time)",
                overhead_ms, overhead_pct
            );
            println!(
                "║  Per-commit avg: {:.1}ms total, {:.1}ms git, {:.1}ms overhead",
                total_ms as f64 / num_feature_commits as f64,
                git_ms as f64 / num_feature_commits as f64,
                overhead_ms as f64 / num_feature_commits as f64,
            );
            println!("╚══════════════════════════════════════════════════════════╝\n");
        }
        Err(e) => {
            println!(
                "Benchmark failed after {:.3}s: {}",
                wall_duration.as_secs_f64(),
                e
            );
            println!(
                "Wall time: {:.3}s ({:.0}ms)",
                wall_duration.as_secs_f64(),
                wall_duration.as_millis()
            );
            panic!("Benchmark failed: {}", e);
        }
    }
}
