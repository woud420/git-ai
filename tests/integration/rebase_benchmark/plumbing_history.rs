use super::{DeterministicRng, Instant, TestRepo, fs};

/// Same monorepo scenario as `benchmark_monorepo_rebase`, but uses plumbing
/// commands (`git commit-tree` + `git update-ref`) instead of `git rebase`.
///
/// Each feature commit is replayed one at a time via:
///   1. `git merge-tree` to compute the rebased tree
///   2. `git commit-tree` to create the new commit object
///   3. `git update-ref` to advance the branch (triggers trace2 rewrite detection)
///
/// This models a generic plumbing-based restack and tests whether Git AI's
/// trace2-driven detection is as fast as the standard porcelain rebase path.
///
/// Run with: cargo test --test integration benchmark_monorepo_plumbing_rebase -- --ignored --nocapture
#[test]
#[ignore]
fn benchmark_monorepo_plumbing_rebase() {
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
    // Use a separate cache dir so it doesn't conflict with the standard rebase cache
    let cache_dir = std::env::var("MONO_BENCH_CACHE_DIR")
        .ok()
        .map(|d| format!("{}-plumbing", d));

    println!("\n=== Monorepo Plumbing Rebase Benchmark ===");
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
    println!("=================================================\n");

    let repo = TestRepo::new();

    // --- Try to restore from cache ---
    let restored_from_cache = if let Some(ref cd) = cache_dir {
        let cache_path = std::path::Path::new(cd);
        if cache_path.join(".git").exists() {
            println!("Restoring repo from cache: {}", cd);
            let restore_start = Instant::now();
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

    let mut rng = DeterministicRng(12345); // Same seed as standard benchmark for identical repo

    let ai_file_paths: Vec<String> = (0..num_ai_files)
        .map(|i| format!("services/payments/src/handlers/payment_handler_{}.rs", i))
        .collect();

    if !restored_from_cache {
        let setup_start = Instant::now();

        // --- Identical setup to benchmark_monorepo_rebase ---
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

        repo.git(&["add", "-A"]).unwrap();
        repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
        repo.stage_all_and_commit("Initial monorepo setup").unwrap();

        let default_branch = repo.current_branch();

        // Feature branch
        repo.git(&["checkout", "-b", "feature/payments-refactor"])
            .unwrap();

        for commit_idx in 0..num_feature_commits {
            let files_this_commit = 2 + rng.gen_range(4);
            let start = rng.gen_range(num_ai_files);
            for i in 0..files_this_commit.min(num_ai_files) {
                let file_idx = (start + i) % num_ai_files;
                let path = repo.path().join(&ai_file_paths[file_idx]);
                let current = fs::read_to_string(&path).unwrap_or_default();
                let num_new_lines = 5 + rng.gen_range(20);
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

        // Busy main branch
        repo.git(&["checkout", &default_branch]).unwrap();
        for main_idx in 0..num_main_commits {
            let touch_ai_files = main_idx % 4 == 0;
            let bg_files_touched = 3 + rng.gen_range(8);
            let bg_start = rng.gen_range(num_background_files);
            for i in 0..bg_files_touched {
                let file_idx = (bg_start + i) % num_background_files;
                let dir = dir_prefixes[file_idx % dir_prefixes.len()];
                let subdir = file_idx / dir_prefixes.len();
                let filename = format!("{}/mod_{}/file_{}.rs", dir, subdir % 10, file_idx);
                let path = repo.path().join(&filename);
                if let Ok(current) = fs::read_to_string(&path) {
                    fs::write(
                        &path,
                        format!(
                            "{}\npub fn main_change_{}_{}() {{}}",
                            current, main_idx, file_idx
                        ),
                    )
                    .unwrap();
                }
            }
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
            repo.git(&["add", "-A"]).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
            repo.stage_all_and_commit(&format!("main: update {} from other team", main_idx))
                .unwrap();

            if (main_idx + 1) % 100 == 0 {
                println!("  Main commit {}/{}", main_idx + 1, num_main_commits);
            }
        }

        // Save to cache
        if let Some(ref cd) = cache_dir {
            let cache_path = std::path::Path::new(cd);
            if !cache_path.join(".git").exists() {
                println!("Saving repo to cache: {}", cd);
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
            }
        }

        println!("Total setup: {:.1}s", setup_start.elapsed().as_secs_f64());
    } // end if !restored_from_cache

    let default_branch = repo.current_branch();

    // --- Step 4: Rebase using plumbing commands ---
    // Collect feature branch commits (oldest to newest)
    repo.git(&["checkout", "feature/payments-refactor"])
        .unwrap();
    let feature_commits_str = repo
        .git(&[
            "rev-list",
            "--reverse",
            &format!("{}..HEAD", default_branch),
        ])
        .unwrap();
    let feature_commits: Vec<&str> = feature_commits_str
        .trim()
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    println!(
        "Feature commits to replay: {} (onto {} main commits)",
        feature_commits.len(),
        num_main_commits
    );

    // Get the main branch tip as our onto target
    let main_tip = repo
        .git(&["rev-parse", &default_branch])
        .unwrap()
        .trim()
        .to_string();

    let pre_notes = repo
        .git(&["notes", "--ref=refs/notes/ai", "list"])
        .unwrap_or_default();
    let pre_count = pre_notes.lines().filter(|l| !l.is_empty()).count();
    println!("AI notes before rebase: {}", pre_count);

    let timing_file = std::path::PathBuf::from("/tmp/monorepo_plumbing_timing.txt");
    let timing_path = timing_file.to_str().unwrap().to_string();

    println!(
        "\n--- Starting plumbing rebase ({} commits via commit-tree + update-ref) ---",
        feature_commits.len()
    );
    let rebase_start = Instant::now();

    // Replay each feature commit onto the new base using plumbing commands.
    //
    // Create every replacement commit first, then move the branch once. The
    // daemon sees the same N-commit rewrite shape as a porcelain rebase.
    let old_tip = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let mut new_parent = main_tip.clone();
    for (idx, &feature_sha) in feature_commits.iter().enumerate() {
        let commit_start = Instant::now();

        // Get the old parent of this feature commit
        let old_parent = repo
            .git(&["rev-parse", &format!("{}^", feature_sha)])
            .unwrap()
            .trim()
            .to_string();

        // Use merge-tree to compute the rebased tree:
        // merge-tree --write-tree --merge-base <old_parent> <new_parent> <feature_commit>
        // This 3-way merge cherry-picks the diff (old_parent→feature) onto new_parent
        let merged_tree_output = repo
            .git(&[
                "merge-tree",
                "--write-tree",
                "--merge-base",
                &old_parent,
                &new_parent,
                feature_sha,
            ])
            .unwrap();
        let merged_tree = merged_tree_output
            .trim()
            .lines()
            .next()
            .unwrap()
            .to_string();

        // Get the original commit message
        let message = repo
            .git(&["log", "-1", "--format=%s", feature_sha])
            .unwrap()
            .trim()
            .to_string();

        // Create the new commit with commit-tree while leaving the ref unchanged.
        let new_commit = repo
            .git(&[
                "commit-tree",
                &merged_tree,
                "-p",
                &new_parent,
                "-m",
                &message,
            ])
            .unwrap()
            .trim()
            .to_string();

        new_parent = new_commit;

        if (idx + 1) % 10 == 0 || idx == feature_commits.len() - 1 {
            println!(
                "  Replayed commit {}/{} ({:.0}ms this, {:.1}s total)",
                idx + 1,
                feature_commits.len(),
                commit_start.elapsed().as_millis(),
                rebase_start.elapsed().as_secs_f64()
            );
        }
    }

    // One atomic update-ref moves the branch from old tip to new tip. This is
    // the single point where the daemon observes the rewrite.
    let new_tip = new_parent;
    println!(
        "\nRunning single update-ref: {} -> {}",
        &old_tip[..12],
        &new_tip[..12]
    );
    repo.git_with_env(
        &[
            "update-ref",
            "refs/heads/feature/payments-refactor",
            &new_tip,
            &old_tip,
        ],
        &[
            ("GIT_AI_DEBUG_PERFORMANCE", "2"),
            ("GIT_AI_REBASE_TIMING_FILE", &timing_path),
        ],
        None,
    )
    .unwrap();

    // Update the working tree to match the rewritten branch.
    repo.git(&["reset", "--hard", &new_tip]).unwrap();
    let rebase_dur = rebase_start.elapsed();

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

    println!("\n=== MONOREPO PLUMBING REBASE RESULTS ===");
    println!(
        "Repo: {} files, {} AI-tracked",
        num_background_files + num_ai_files,
        num_ai_files
    );
    println!(
        "Rebase: {} feature commits onto {} main commits",
        feature_commits.len(),
        num_main_commits
    );
    println!(
        "Total: {:.3}s ({:.0}ms)",
        rebase_dur.as_secs_f64(),
        rebase_dur.as_millis()
    );
    println!(
        "Per feature commit: {:.1}ms",
        rebase_dur.as_millis() as f64 / feature_commits.len() as f64
    );
    println!("=========================================\n");
}
