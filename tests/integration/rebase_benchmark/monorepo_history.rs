use super::{DeterministicRng, Instant, TestRepo, fs};

/// Realistic large monorepo benchmark modeling the key use case:
/// A developer rebases a small feature branch onto a busy main branch.
///
/// Key characteristics that differ from prior benchmarks:
/// - **Large repo tree**: 2000+ files across deep directory structure (makes tree diffs realistic)
/// - **Small feature branch**: 10 commits, each touching 2-5 AI-tracked files
/// - **Busy main branch**: 100 commits since feature diverged (touching various areas)
/// - **Sparse AI overlap**: Only 8 AI-tracked files, main mostly touches OTHER areas
/// - **Main occasionally touches same files**: Forces diff-based path (not fast-path note remap)
///
/// This models: "I've been working on a feature for a few days, main has moved forward
/// significantly, and I need to rebase before merging."
///
/// ## Repo caching
///
/// Setup takes ~15-20 minutes. To reuse a cached repo across runs:
///
/// ```sh
/// # First run: creates and saves the repo
/// MONO_BENCH_CACHE_DIR=/tmp/monorepo-bench-cache cargo test ... benchmark_monorepo_rebase ...
///
/// # Subsequent runs: restores from cache (~2s instead of ~15min)
/// MONO_BENCH_CACHE_DIR=/tmp/monorepo-bench-cache cargo test ... benchmark_monorepo_rebase ...
/// ```
///
/// Run with: cargo test --test integration benchmark_monorepo_rebase -- --ignored --nocapture
#[test]
#[ignore]
fn benchmark_monorepo_rebase() {
    let num_background_files: usize = std::env::var("MONO_BENCH_BG_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let num_ai_files: usize = std::env::var("MONO_BENCH_AI_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let lines_per_ai_file: usize = std::env::var("MONO_BENCH_LINES_PER_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let num_feature_commits: usize = std::env::var("MONO_BENCH_FEATURE_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let num_main_commits: usize = std::env::var("MONO_BENCH_MAIN_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    let cache_dir = std::env::var("MONO_BENCH_CACHE_DIR").ok();

    println!("\n=== Monorepo Rebase Benchmark ===");
    println!(
        "Background files:  {} (repo tree size)",
        num_background_files
    );
    println!("AI-tracked files:  {}", num_ai_files);
    println!("Lines per AI file: {}", lines_per_ai_file);
    println!("Feature commits:   {}", num_feature_commits);
    println!("Main commits:      {}", num_main_commits);
    if let Some(ref cd) = cache_dir {
        println!("Cache dir:         {}", cd);
    }
    println!("=================================\n");

    let repo = TestRepo::new();

    // --- Try to restore from cache ---
    let restored_from_cache = if let Some(ref cd) = cache_dir {
        let cache_path = std::path::Path::new(cd);
        if cache_path.join(".git").exists() {
            println!("Restoring repo from cache: {}", cd);
            let restore_start = Instant::now();
            // Copy cached working tree + .git into the test repo
            let status = std::process::Command::new("rsync")
                .args([
                    "-a",
                    "--delete",
                    &format!("{}/", cd),
                    &format!("{}/", repo.path().display()),
                ])
                .status()
                .expect("rsync failed");
            assert!(status.success(), "rsync restore from cache failed");
            println!(
                "Restored from cache in {:.1}s",
                restore_start.elapsed().as_secs_f64()
            );
            true
        } else {
            println!(
                "Cache dir not found, will create fresh setup and save to: {}",
                cd
            );
            false
        }
    } else {
        false
    };

    let mut rng = DeterministicRng(12345);
    let setup_start = Instant::now();

    // AI file paths used throughout setup and rebase
    let ai_file_paths: Vec<String> = (0..num_ai_files)
        .map(|i| format!("services/payments/src/handlers/payment_handler_{}.rs", i))
        .collect();

    if !restored_from_cache {
        // --- Step 1: Create a large repo with deep directory structure ---
        // All files are 100% AI-authored (worst case for rebase logic).
        // Write all files directly to disk, then do ONE bulk checkpoint.
        let dir_prefixes = [
            "services/auth/src",
            "services/billing/src",
            "services/notifications/src",
            "services/search/src",
            "services/analytics/src",
            "libs/common/src",
            "libs/database/src",
            "libs/cache/src",
            "libs/logging/src",
            "tools/cli/src",
            "tools/admin/src",
            "docs/api",
            "docs/internal",
            "config/deploy",
            "config/monitoring",
            "tests/e2e",
            "tests/integration",
            "tests/unit",
            "scripts/ci",
            "scripts/migration",
        ];

        // Create background files directly on disk (no per-file checkpoint)
        for file_idx in 0..num_background_files {
            let dir = dir_prefixes[file_idx % dir_prefixes.len()];
            let subdir = file_idx / dir_prefixes.len();
            let filename = format!("{}/mod_{}/file_{}.rs", dir, subdir % 10, file_idx);
            let file_path = repo.path().join(&filename);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(
                &file_path,
                format!(
                    "// Background file {}\npub fn bg_func_{}() {{}}\n",
                    file_idx, file_idx
                ),
            )
            .unwrap();
        }

        // Create AI-tracked files directly on disk
        for (file_idx, ai_path) in ai_file_paths.iter().enumerate() {
            let file_path = repo.path().join(ai_path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut content = String::new();
            content.push_str(&format!("// Payment handler {}\n", file_idx));
            content.push_str("// MAIN_INSERTION_POINT\n");
            content.push_str(&format!("pub mod payment_handler_{} {{\n", file_idx));
            for line_idx in 0..lines_per_ai_file {
                content.push_str(&format!(
                    "    pub fn process_{}() -> Result<(), PaymentError> {{ Ok(()) }}\n",
                    line_idx
                ));
            }
            content.push_str("    // FEATURE_INSERTION_POINT\n");
            content.push_str("}\n");
            fs::write(&file_path, &content).unwrap();
        }

        // Stage everything and do ONE bulk AI checkpoint for all files
        repo.git(&["add", "-A"]).unwrap();
        let checkpoint_start = Instant::now();
        repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
        println!(
            "Bulk AI checkpoint ({} files): {:.1}s",
            num_background_files + num_ai_files,
            checkpoint_start.elapsed().as_secs_f64()
        );

        repo.stage_all_and_commit("Initial monorepo setup").unwrap();
        let initial_time = setup_start.elapsed();
        println!(
            "Initial commit ({} files): {:.1}s",
            num_background_files + num_ai_files,
            initial_time.as_secs_f64()
        );

        let default_branch = repo.current_branch();

        // --- Step 2: Feature branch (small, focused, 100% AI) ---
        // Each commit checkpoints per-file (realistic: one prompt per file edit)
        repo.git(&["checkout", "-b", "feature/payments-refactor"])
            .unwrap();
        let feature_start = Instant::now();

        for commit_idx in 0..num_feature_commits {
            // Each feature commit touches 2-5 of the AI files (not all of them)
            let files_this_commit = 2 + rng.gen_range(4); // 2-5 files
            let start = rng.gen_range(num_ai_files);

            for i in 0..files_this_commit.min(num_ai_files) {
                let file_idx = (start + i) % num_ai_files;
                let path = repo.path().join(&ai_file_paths[file_idx]);
                let current = fs::read_to_string(&path).unwrap_or_default();

                // Append AI-authored code at the feature insertion point
                let num_new_lines = 5 + rng.gen_range(20); // 5-24 lines per file
                let mut addition = format!(
                    "    pub fn feature_{}_handler_{}() -> Result<(), PaymentError> {{\n",
                    commit_idx, file_idx
                );
                for j in 0..num_new_lines {
                    addition.push_str(&format!(
                        "        let step_{} = validate_payment({}, {});\n",
                        j, commit_idx, j
                    ));
                }
                addition.push_str("        Ok(())\n    }\n    // FEATURE_INSERTION_POINT");

                let new_content = current.replacen("    // FEATURE_INSERTION_POINT", &addition, 1);
                fs::write(&path, &new_content).unwrap();
                // Per-file checkpoint (realistic: one AI prompt per file)
                repo.git_ai(&["checkpoint", "mock_ai", &ai_file_paths[file_idx]])
                    .unwrap();
            }

            repo.git(&["add", "-A"]).unwrap();
            repo.stage_all_and_commit(&format!(
                "feat(payments): implement step {} of refactor",
                commit_idx
            ))
            .unwrap();
        }
        println!(
            "Feature branch: {:.1}s ({} commits)",
            feature_start.elapsed().as_secs_f64(),
            num_feature_commits
        );

        // --- Step 3: Busy main branch (all 100% AI-authored — worst case) ---
        // Models: many other developers merging AI-written code into main
        repo.git(&["checkout", &default_branch]).unwrap();
        let main_start = Instant::now();

        for main_idx in 0..num_main_commits {
            // Most main commits touch OTHER areas (not payments)
            let touch_ai_files = main_idx % 4 == 0; // ~25% of main commits touch AI files

            // Touch 3-10 background files per commit (simulates other developers' work)
            let bg_files_touched = 3 + rng.gen_range(8);
            let bg_start = rng.gen_range(num_background_files);
            for i in 0..bg_files_touched {
                let file_idx = (bg_start + i) % num_background_files;
                let dir = dir_prefixes[file_idx % dir_prefixes.len()];
                let subdir = file_idx / dir_prefixes.len();
                let filename = format!("{}/mod_{}/file_{}.rs", dir, subdir % 10, file_idx);
                let path = repo.path().join(&filename);
                if let Ok(current) = fs::read_to_string(&path) {
                    let new_content = format!(
                        "{}\npub fn main_change_{}_{}() {{}}",
                        current, main_idx, file_idx
                    );
                    fs::write(&path, &new_content).unwrap();
                }
            }

            // Occasionally touch AI-tracked files (forces slow path on rebase)
            if touch_ai_files {
                for ai_path in &ai_file_paths {
                    let path = repo.path().join(ai_path);
                    if let Ok(current) = fs::read_to_string(&path) {
                        let new_content = current.replacen(
                        "// MAIN_INSERTION_POINT",
                        &format!(
                            "// infra: config update v{}\nconst PAYMENT_CFG_{}: u32 = {};\n// MAIN_INSERTION_POINT",
                            main_idx, main_idx, main_idx * 42
                        ),
                        1,
                    );
                        fs::write(&path, &new_content).unwrap();
                    }
                }
            }

            // Stage and checkpoint as AI (all changes are AI-authored)
            repo.git(&["add", "-A"]).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
            repo.stage_all_and_commit(&format!("main: update {} from other team", main_idx))
                .unwrap();

            if (main_idx + 1) % 25 == 0 {
                println!(
                    "  Main commit {}/{} ({:.1}s)",
                    main_idx + 1,
                    num_main_commits,
                    main_start.elapsed().as_secs_f64()
                );
            }
        }
        println!(
            "Main branch: {:.1}s ({} commits)",
            main_start.elapsed().as_secs_f64(),
            num_main_commits
        );
        println!("Total setup: {:.1}s", setup_start.elapsed().as_secs_f64());

        // Save to cache if requested
        if let Some(ref cd) = cache_dir {
            let cache_path = std::path::Path::new(cd);
            if !cache_path.join(".git").exists() {
                println!("Saving repo to cache: {}", cd);
                let save_start = Instant::now();
                fs::create_dir_all(cache_path).expect("create cache dir");
                let status = std::process::Command::new("rsync")
                    .args([
                        "-a",
                        "--delete",
                        &format!("{}/", repo.path().display()),
                        &format!("{}/", cd),
                    ])
                    .status()
                    .expect("rsync failed");
                assert!(status.success(), "rsync save to cache failed");
                println!(
                    "Saved to cache in {:.1}s",
                    save_start.elapsed().as_secs_f64()
                );
            }
        }
    } // end if !restored_from_cache

    // Detect default branch (works whether fresh setup or cache restore — we're on main either way)
    let default_branch = repo.current_branch();

    // --- Step 4: Rebase with full timing instrumentation ---
    repo.git(&["checkout", "feature/payments-refactor"])
        .unwrap();
    let timing_file = std::path::PathBuf::from("/tmp/monorepo_rebase_timing.txt");
    let timing_path = timing_file.to_str().unwrap().to_string();

    // Count notes before rebase
    let pre_notes = repo
        .git(&["notes", "--ref=refs/notes/ai", "list"])
        .unwrap_or_default();
    let pre_count = pre_notes.lines().filter(|l| !l.is_empty()).count();
    println!("\nAI notes before rebase: {}", pre_count);

    println!(
        "\n--- Starting monorepo rebase ({} feature commits onto {} main commits) ---",
        num_feature_commits, num_main_commits
    );
    let rebase_start = Instant::now();
    let result = repo.git_with_env(
        &["rebase", &default_branch],
        &[
            ("GIT_AI_DEBUG_PERFORMANCE", "2"),
            ("GIT_AI_REBASE_TIMING_FILE", &timing_path),
        ],
        None,
    );
    let rebase_dur = rebase_start.elapsed();

    match &result {
        Ok(_) => println!("Rebase succeeded in {:.3}s", rebase_dur.as_secs_f64()),
        Err(e) => println!("Rebase FAILED in {:.3}s: {}", rebase_dur.as_secs_f64(), e),
    }
    result.unwrap();

    // Post-rebase note check
    let post_notes = repo
        .git(&["notes", "--ref=refs/notes/ai", "list"])
        .unwrap_or_default();
    let post_count = post_notes.lines().filter(|l| !l.is_empty()).count();
    println!("AI notes after rebase:  {}", post_count);

    // Phase timing breakdown
    if let Ok(timing_data) = fs::read_to_string(&timing_file) {
        println!("\n=== PHASE TIMING ===");
        print!("{}", timing_data);
        println!("====================\n");
    } else {
        println!("(No timing file — possibly fast-path or no notes to rewrite)");
    }

    println!("=== MONOREPO REBASE RESULTS ===");
    println!(
        "Repo: {} files, {} AI-tracked",
        num_background_files + num_ai_files,
        num_ai_files
    );
    println!(
        "Rebase: {} feature commits onto {} main commits",
        num_feature_commits, num_main_commits
    );
    println!(
        "Total: {:.3}s ({:.0}ms)",
        rebase_dur.as_secs_f64(),
        rebase_dur.as_millis()
    );
    println!(
        "Per feature commit: {:.1}ms",
        rebase_dur.as_millis() as f64 / num_feature_commits as f64
    );
    println!("================================\n");
}
