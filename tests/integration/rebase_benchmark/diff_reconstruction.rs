use super::{ExpectedLineExt, Instant, TestRepo, extract_timing, fs};

/// Benchmark diff-based attribution transfer with large files and content changes.
/// This tests the scenario where rebasing changes file content (main branch modifies
/// AI-tracked files), forcing the diff-based path instead of the fast-path note remap.
///
/// Scale: 50 commits × 10 files × 200 lines = significant AI-authored content.
/// The diff-based path should complete the per-commit processing loop in <10ms total.
#[test]
#[ignore]
fn benchmark_rebase_diff_based_large() {
    let num_feature_commits: usize = std::env::var("REBASE_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let num_ai_files: usize = std::env::var("REBASE_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let lines_per_file: usize = std::env::var("REBASE_BENCH_LINES_PER_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    println!("\n=== Diff-Based Large Rebase Benchmark ===");
    println!("Feature commits: {}", num_feature_commits);
    println!("AI files: {}", num_ai_files);
    println!("Lines per file: {}", lines_per_file);
    println!("==========================================\n");

    let repo = TestRepo::new();

    // Create initial commit with shared files (both branches will modify)
    {
        for file_idx in 0..num_ai_files {
            let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
            let mut file = repo.filename(&filename);
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            lines.push(format!("// Header for module {}", file_idx).into());
            lines.push("// Main branch will add lines above this marker".into());
            for line_idx in 0..lines_per_file {
                lines.push(format!("// Initial AI code mod{} line{}", file_idx, line_idx).ai());
            }
            file.set_contents(lines);
        }
        repo.stage_all_and_commit("Initial shared files").unwrap();
    }

    let default_branch = repo.current_branch();

    // Create feature branch with AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let setup_start = Instant::now();
    for commit_idx in 0..num_feature_commits {
        for file_idx in 0..num_ai_files {
            let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            let new_content = format!(
                "{}\n// AI addition v{} mod{}",
                current, commit_idx, file_idx
            );
            fs::write(&path, &new_content).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &filename]).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("AI feature {}", commit_idx))
            .unwrap();

        if (commit_idx + 1) % 10 == 0 {
            println!(
                "  Feature commit {}/{} ({:.1}s)",
                commit_idx + 1,
                num_feature_commits,
                setup_start.elapsed().as_secs_f64()
            );
        }
    }
    println!("Feature setup: {:.1}s", setup_start.elapsed().as_secs_f64());

    // Advance main branch with modifications to AI-tracked files (forces content changes on rebase)
    repo.git(&["checkout", &default_branch]).unwrap();
    for main_idx in 0..5 {
        for file_idx in 0..num_ai_files {
            let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            let new_content = current.replacen(
                "// Main branch will add lines above this marker",
                &format!(
                    "// Main addition {} for mod{}\n// Main branch will add lines above this marker",
                    main_idx, file_idx
                ),
                1,
            );
            fs::write(&path, &new_content).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("Main change {}", main_idx))
            .unwrap();
    }

    // Unrelated main commits
    for i in 0..10 {
        let filename = format!("main_only/change_{}.txt", i);
        let mut file = repo.filename(&filename);
        file.set_contents(crate::lines![format!("main only {}", i)]);
        repo.stage_all_and_commit(&format!("Main unrelated {}", i))
            .unwrap();
    }

    // Rebase feature onto main
    repo.git(&["checkout", "feature"]).unwrap();
    let timing_file = repo.path().join("..").join("rebase_timing_diff.txt");
    let timing_path = timing_file.to_str().unwrap().to_string();

    println!("\n--- Starting diff-based rebase ---");
    let rebase_start = Instant::now();
    let result = repo.git_with_env(
        &["rebase", &default_branch],
        &[
            ("GIT_AI_DEBUG_PERFORMANCE", "1"),
            ("GIT_AI_REBASE_TIMING_FILE", &timing_path),
        ],
        None,
    );
    let rebase_duration = rebase_start.elapsed();

    match &result {
        Ok(_) => println!("Rebase succeeded in {:.3}s", rebase_duration.as_secs_f64()),
        Err(e) => println!(
            "Rebase FAILED in {:.3}s: {}",
            rebase_duration.as_secs_f64(),
            e
        ),
    }
    result.unwrap();

    if let Ok(timing_data) = fs::read_to_string(&timing_file) {
        println!("\n=== PHASE TIMING BREAKDOWN ===");
        print!("{}", timing_data);
        println!("===============================");
    }

    println!("\n=== DIFF-BASED LARGE BENCHMARK RESULTS ===");
    println!(
        "Total rebase time: {:.3}s ({:.0}ms)",
        rebase_duration.as_secs_f64(),
        rebase_duration.as_millis()
    );
    println!(
        "Per-commit average: {:.1}ms",
        rebase_duration.as_millis() as f64 / num_feature_commits as f64
    );
    println!("============================================\n");
}

/// Benchmark comparing the notes-based fast path vs blame-based slow path.
/// Runs the same rebase twice: once with notes (fast) and once without (blame fallback).
///
/// Run with: cargo test --test integration benchmark_blame_vs_diff -- --ignored --nocapture
#[test]
#[ignore]
fn benchmark_blame_vs_diff() {
    let num_feature_commits: usize = std::env::var("REBASE_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let num_ai_files: usize = std::env::var("REBASE_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let lines_per_file: usize = std::env::var("REBASE_BENCH_LINES_PER_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);

    println!("\n=== Blame vs Diff-Based Benchmark ===");
    println!("Feature commits: {}", num_feature_commits);
    println!("AI files: {}", num_ai_files);
    println!("Lines per file: {}", lines_per_file);
    println!("======================================\n");

    // Helper closure to create a test repo with the same setup
    let create_repo = |strip_notes: bool| -> (std::time::Duration, String) {
        let repo = TestRepo::new();
        for file_idx in 0..num_ai_files {
            let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
            let mut file = repo.filename(&filename);
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            lines.push(format!("// Header for module {}", file_idx).into());
            lines.push("// Main branch marker".into());
            for line_idx in 0..lines_per_file {
                lines.push(format!("// AI code mod{} line{}", file_idx, line_idx).ai());
            }
            file.set_contents(lines);
        }
        repo.stage_all_and_commit("Initial shared files").unwrap();
        let default_branch = repo.current_branch();

        repo.git(&["checkout", "-b", "feature"]).unwrap();
        for commit_idx in 0..num_feature_commits {
            for file_idx in 0..num_ai_files {
                let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
                let path = repo.path().join(&filename);
                let current = fs::read_to_string(&path).unwrap_or_default();
                let new_content = format!(
                    "{}\n// AI addition v{} mod{}",
                    current, commit_idx, file_idx
                );
                fs::write(&path, &new_content).unwrap();
                repo.git_ai(&["checkpoint", "mock_ai", &filename]).unwrap();
            }
            repo.git(&["add", "-A"]).unwrap();
            repo.stage_all_and_commit(&format!("AI feature {}", commit_idx))
                .unwrap();
        }

        if strip_notes {
            // Delete the authorship notes ref to force the blame-based fallback
            let _ = repo.git(&["update-ref", "-d", "refs/notes/git-ai-authorship"]);
        }

        repo.git(&["checkout", &default_branch]).unwrap();
        for main_idx in 0..5 {
            for file_idx in 0..num_ai_files {
                let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
                let path = repo.path().join(&filename);
                let current = fs::read_to_string(&path).unwrap_or_default();
                let new_content = current.replacen(
                    "// Main branch marker",
                    &format!(
                        "// Main addition {} mod{}\n// Main branch marker",
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

        repo.git(&["checkout", "feature"]).unwrap();
        let timing_file = repo.path().join("..").join(if strip_notes {
            "timing_no_notes.txt"
        } else {
            "timing_with_notes.txt"
        });
        let timing_path = timing_file.to_str().unwrap().to_string();

        let rebase_start = Instant::now();
        repo.git_with_env(
            &["rebase", &default_branch],
            &[
                ("GIT_AI_DEBUG_PERFORMANCE", "1"),
                ("GIT_AI_REBASE_TIMING_FILE", &timing_path),
            ],
            None,
        )
        .unwrap();
        let duration = rebase_start.elapsed();

        let timing_data = fs::read_to_string(&timing_file).unwrap_or_default();
        (duration, timing_data)
    };

    // Run with notes (diff-based fast path)
    let (with_notes_dur, with_notes_timing) = create_repo(false);
    println!("--- WITH NOTES (diff-based path) ---");
    print!("{}", with_notes_timing);
    println!("Total rebase: {:.0}ms\n", with_notes_dur.as_millis());

    // Run without notes (blame-based slow path)
    let (no_notes_dur, no_notes_timing) = create_repo(true);
    println!("--- WITHOUT NOTES (blame-based fallback) ---");
    print!("{}", no_notes_timing);
    println!("Total rebase: {:.0}ms\n", no_notes_dur.as_millis());

    let authorship_with =
        extract_timing(&with_notes_timing, "TOTAL").unwrap_or(with_notes_dur.as_millis() as u64);
    let authorship_without =
        extract_timing(&no_notes_timing, "TOTAL").unwrap_or(no_notes_dur.as_millis() as u64);

    if authorship_without > 0 {
        let speedup = authorship_without as f64 / authorship_with.max(1) as f64;
        println!("=== COMPARISON ===");
        println!("Authorship rewrite with notes:    {}ms", authorship_with);
        println!("Authorship rewrite without notes: {}ms", authorship_without);
        println!("Speedup:                          {:.1}x", speedup);
        println!("==================\n");
    }
}

/// Benchmark that forces the SLOW path (VirtualAttributions + blame) by having
/// main branch also modify AI-touched files. This causes blob differences
/// between original and rebased commits, making the fast-path note remap fail.
///
/// This is the worst-case scenario and what we need to optimize.
#[test]
#[ignore]
fn benchmark_rebase_slow_path() {
    let num_feature_commits: usize = std::env::var("REBASE_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let num_ai_files: usize = std::env::var("REBASE_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let lines_per_file: usize = std::env::var("REBASE_BENCH_LINES_PER_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    println!("\n=== Slow-Path Rebase Benchmark ===");
    println!("Feature commits: {}", num_feature_commits);
    println!("AI files: {}", num_ai_files);
    println!("Lines per file: {}", lines_per_file);
    println!("===================================\n");

    let repo = TestRepo::new();

    // Create initial commit with the shared files that both branches will modify
    // This ensures both branches touch the same AI-tracked files
    {
        for file_idx in 0..num_ai_files {
            let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
            let mut file = repo.filename(&filename);
            let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
            // Initial content: a header that main will modify + body that feature will modify
            lines.push(format!("// Header for module {}", file_idx).into());
            lines.push("// Main branch will add lines above this marker".into());
            for line_idx in 0..lines_per_file {
                lines.push(format!("// Initial AI code mod{} line{}", file_idx, line_idx).ai());
            }
            file.set_contents(lines);
        }
        repo.stage_all_and_commit("Initial shared files").unwrap();
    }

    let default_branch = repo.current_branch();

    // Create feature branch with AI commits that modify the shared files
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    let setup_start = Instant::now();
    for commit_idx in 0..num_feature_commits {
        for file_idx in 0..num_ai_files {
            let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
            let path = repo.path().join(&filename);

            // Read current content and append AI lines at the bottom
            let current = fs::read_to_string(&path).unwrap_or_default();
            let new_content = format!(
                "{}\n// AI addition v{} mod{}",
                current, commit_idx, file_idx
            );
            fs::write(&path, &new_content).unwrap();

            // Checkpoint as AI
            repo.git_ai(&["checkpoint", "mock_ai", &filename]).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("AI feature {}", commit_idx))
            .unwrap();

        if (commit_idx + 1) % 10 == 0 {
            println!(
                "  Feature commit {}/{} ({:.1}s)",
                commit_idx + 1,
                num_feature_commits,
                setup_start.elapsed().as_secs_f64()
            );
        }
    }
    println!("Feature setup: {:.1}s", setup_start.elapsed().as_secs_f64());

    // Go back to main and modify the SAME AI-tracked files at the TOP
    // This creates non-conflicting changes (different regions) that still cause
    // different blob OIDs after rebase, forcing the slow path
    repo.git(&["checkout", &default_branch]).unwrap();

    for main_idx in 0..5 {
        for file_idx in 0..num_ai_files {
            let filename = format!("shared/mod_{}/f_{}.rs", file_idx, file_idx);
            let path = repo.path().join(&filename);
            let current = fs::read_to_string(&path).unwrap_or_default();
            // Insert at the top (before the marker)
            let new_content = current.replacen(
                "// Main branch will add lines above this marker",
                &format!(
                    "// Main addition {} for mod{}\n// Main branch will add lines above this marker",
                    main_idx, file_idx
                ),
                1,
            );
            fs::write(&path, &new_content).unwrap();
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("Main change {}", main_idx))
            .unwrap();
    }

    // Also add some unrelated main commits for realism
    for i in 0..10 {
        let filename = format!("main_only/change_{}.txt", i);
        let mut file = repo.filename(&filename);
        file.set_contents(crate::lines![format!("main only {}", i)]);
        repo.stage_all_and_commit(&format!("Main unrelated {}", i))
            .unwrap();
    }

    // Now rebase feature onto main - this should trigger the slow path
    // because the AI-tracked files have different blobs after rebase
    repo.git(&["checkout", "feature"]).unwrap();

    let timing_file = repo.path().join("..").join("rebase_timing.txt");
    let timing_path = timing_file.to_str().unwrap().to_string();

    println!("\n--- Starting slow-path rebase ---");
    let rebase_start = Instant::now();
    let result = repo.git_with_env(
        &["rebase", &default_branch],
        &[
            ("GIT_AI_DEBUG_PERFORMANCE", "1"),
            ("GIT_AI_REBASE_TIMING_FILE", &timing_path),
        ],
        None,
    );
    let rebase_duration = rebase_start.elapsed();

    match &result {
        Ok(output) => {
            println!("Rebase succeeded in {:.3}s", rebase_duration.as_secs_f64());
            // Print only last few lines of output to avoid noise
            let lines: Vec<&str> = output.lines().collect();
            let start = lines.len().saturating_sub(10);
            for line in &lines[start..] {
                println!("  {}", line);
            }
        }
        Err(e) => {
            println!(
                "Rebase FAILED in {:.3}s: {}",
                rebase_duration.as_secs_f64(),
                e
            );
        }
    }
    result.unwrap();

    // Read and display detailed timing breakdown
    if let Ok(timing_data) = fs::read_to_string(&timing_file) {
        println!("\n=== PHASE TIMING BREAKDOWN ===");
        print!("{}", timing_data);
        println!("===============================");
    }

    println!("\n=== SLOW-PATH BENCHMARK RESULTS ===");
    println!(
        "Total rebase time: {:.3}s ({:.0}ms)",
        rebase_duration.as_secs_f64(),
        rebase_duration.as_millis()
    );
    println!(
        "Per-commit average: {:.1}ms",
        rebase_duration.as_millis() as f64 / num_feature_commits as f64
    );
    println!("====================================\n");
}
