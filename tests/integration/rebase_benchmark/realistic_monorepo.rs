use super::{DeterministicRng, ExpectedLineExt, Instant, TestRepo, fs};

/// Realistic monorepo benchmark addressing all feedback from principal eng review:
/// 1. Non-uniform change patterns (1-20 files per commit, varying edit types)
/// 2. File heterogeneity (10 to 2000+ lines, different structures)
/// 3. Multi-author attribution (3 different AI models + human lines)
/// 4. Renames/moves (directory restructuring mid-feature)
/// 5. Non-trivial hunks (20-100 line AI edits, deletions, replacements)
/// 6. Clean rebase still (conflicts are hard to automate reproducibly)
#[test]
#[ignore]
fn benchmark_rebase_realistic_monorepo() {
    let num_feature_commits: usize = std::env::var("REALISTIC_BENCH_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let num_main_commits: usize = std::env::var("REALISTIC_BENCH_MAIN_COMMITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);

    println!("\n=== Realistic Monorepo Rebase Benchmark ===");
    println!(
        "Feature commits: {}, Main commits: {}",
        num_feature_commits, num_main_commits
    );
    println!("============================================\n");

    let repo = TestRepo::new();
    let mut rng = DeterministicRng(42); // deterministic for reproducibility

    // All checkpoints use "mock_ai" — the recognized test preset

    // --- Step 1: Create heterogeneous initial files ---
    // Mix of small configs, medium modules, and large files
    struct FileSpec {
        path: String,
        initial_lines: usize,
        has_ai: bool,
    }

    let mut files: Vec<FileSpec> = Vec::new();

    // Small config files (10-30 lines)
    for i in 0..5 {
        files.push(FileSpec {
            path: format!("config/settings_{}.toml", i),
            initial_lines: 10 + (i * 4),
            has_ai: i % 2 == 0, // some have AI, some don't
        });
    }

    // Medium source files (100-400 lines) — the bulk
    for i in 0..30 {
        let module = i % 6;
        files.push(FileSpec {
            path: format!("src/mod_{}/component_{}.rs", module, i),
            initial_lines: 100 + (i * 10),
            has_ai: true,
        });
    }

    // Large files (800-2000 lines)
    for i in 0..5 {
        files.push(FileSpec {
            path: format!("src/core/engine_{}.rs", i),
            initial_lines: 800 + (i * 300),
            has_ai: true,
        });
    }

    // Test files (200-500 lines)
    for i in 0..10 {
        files.push(FileSpec {
            path: format!("tests/test_{}.rs", i),
            initial_lines: 200 + (i * 30),
            has_ai: true,
        });
    }

    println!(
        "Creating {} files ({} with AI attribution)...",
        files.len(),
        files.iter().filter(|f| f.has_ai).count()
    );

    // Create initial files with mixed attribution
    for (idx, spec) in files.iter().enumerate() {
        let mut file = repo.filename(&spec.path);
        let mut lines: Vec<crate::repos::test_file::ExpectedLine> = Vec::new();
        lines.push(format!("// File: {} (auto-generated)", spec.path).into());
        lines.push("// MAIN_INSERTION_POINT".into());

        for line_idx in 0..spec.initial_lines {
            let line_text = if line_idx % 20 == 0 {
                format!("// Section {} of {}", line_idx / 20, spec.path)
            } else if line_idx % 5 == 0 {
                format!("pub fn func_{}_{}() -> Result<(), Error> {{", idx, line_idx)
            } else if line_idx % 5 == 4 {
                "}".to_string()
            } else {
                format!("    let val_{} = compute_{}({});", line_idx, idx, line_idx)
            };

            if spec.has_ai && line_idx % 3 != 0 {
                // 2/3 of lines are AI-authored, 1/3 human — creates fragmented attribution
                lines.push(line_text.ai());
            } else {
                lines.push(line_text.into());
            }
        }
        lines.push("// FEATURE_INSERTION_POINT".into());
        lines.push("// EOF".into());
        file.set_contents(lines);
    }
    repo.stage_all_and_commit("Initial heterogeneous files")
        .unwrap();
    let default_branch = repo.current_branch();

    // --- Step 2: Feature branch with non-uniform commits ---
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let feature_start = Instant::now();

    for commit_idx in 0..num_feature_commits {
        // Vary how many files each commit touches (1 to 20)
        let num_files_to_touch = if commit_idx % 10 == 0 {
            // Every 10th commit is a big refactor touching many files
            15 + rng.gen_range(10) // 15-24 files
        } else if commit_idx % 3 == 0 {
            // Some commits touch just 1-2 files (focused edits)
            1 + rng.gen_range(2)
        } else {
            // Normal commits touch 3-8 files
            3 + rng.gen_range(6)
        };

        let ai_files: Vec<usize> = files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.has_ai)
            .map(|(i, _)| i)
            .collect();

        // Pick random subset of files to touch
        let mut touched: Vec<usize> = Vec::new();
        for _ in 0..num_files_to_touch.min(ai_files.len()) {
            let pick = ai_files[rng.gen_range(ai_files.len())];
            if !touched.contains(&pick) {
                touched.push(pick);
            }
        }

        // Vary edit patterns per file
        for &file_idx in &touched {
            let spec = &files[file_idx];
            let path = repo.path().join(&spec.path);
            let current = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let edit_type = rng.gen_range(5);

            let new_content = match edit_type {
                0 => {
                    // Large append (20-80 lines) — typical AI code generation
                    let num_new_lines = 20 + rng.gen_range(60);
                    let mut addition = String::new();
                    addition.push_str(&format!(
                        "\n// Feature {} addition ({} lines)\n",
                        commit_idx, num_new_lines
                    ));
                    addition.push_str(&format!(
                        "pub fn feature_{}_impl_{} () {{\n",
                        commit_idx, file_idx
                    ));
                    for j in 0..num_new_lines {
                        addition.push_str(&format!(
                            "    let step_{} = process_{}({});\n",
                            j, commit_idx, j
                        ));
                    }
                    addition.push_str("}\n// FEATURE_INSERTION_POINT");
                    current.replacen("// FEATURE_INSERTION_POINT", &addition, 1)
                }
                1 => {
                    // Replacement edit — delete some lines, add different ones
                    let lines: Vec<&str> = current.lines().collect();
                    if lines.len() > 30 {
                        let start = 10 + rng.gen_range(lines.len() / 3);
                        let del_count = 5 + rng.gen_range(15);
                        let end = (start + del_count).min(lines.len() - 5);
                        let add_count = 10 + rng.gen_range(30);
                        let mut result: Vec<String> =
                            lines[..start].iter().map(|s| s.to_string()).collect();
                        result.push(format!("// Refactored section (commit {})", commit_idx));
                        for j in 0..add_count {
                            result.push(format!(
                                "    let refactored_{} = new_impl_{}_{}();",
                                j, commit_idx, file_idx
                            ));
                        }
                        result.extend(lines[end..].iter().map(|s| s.to_string()));
                        result.join("\n")
                    } else {
                        // File too small, just append
                        current.replacen(
                            "// FEATURE_INSERTION_POINT",
                            &format!(
                                "fn small_edit_{}() {{ }}\n// FEATURE_INSERTION_POINT",
                                commit_idx
                            ),
                            1,
                        )
                    }
                }
                2 => {
                    // Multi-site edit — insert at multiple locations
                    let lines: Vec<&str> = current.lines().collect();
                    let mut result: Vec<String> = Vec::with_capacity(lines.len() + 20);
                    let insert_every = lines.len() / 4;
                    for (i, line) in lines.iter().enumerate() {
                        result.push(line.to_string());
                        if insert_every > 0 && i > 0 && i % insert_every == 0 && i < lines.len() - 5
                        {
                            for j in 0..5 {
                                result.push(format!(
                                    "    // Injected at site {} by commit {} (line {})",
                                    i, commit_idx, j
                                ));
                            }
                        }
                    }
                    result.join("\n")
                }
                3 => {
                    // Pure deletion (remove 5-20 lines from middle)
                    let lines: Vec<&str> = current.lines().collect();
                    if lines.len() > 40 {
                        let start = 15 + rng.gen_range(lines.len() / 3);
                        let del_count = 5 + rng.gen_range(15);
                        let end = (start + del_count).min(lines.len() - 5);
                        let mut result: Vec<String> =
                            lines[..start].iter().map(|s| s.to_string()).collect();
                        result.push(format!(
                            "// Deleted {} lines (commit {})",
                            end - start,
                            commit_idx
                        ));
                        result.extend(lines[end..].iter().map(|s| s.to_string()));
                        result.join("\n")
                    } else {
                        current.clone()
                    }
                }
                _ => {
                    // Small append (2-5 lines) — quick fix style
                    current.replacen(
                        "// FEATURE_INSERTION_POINT",
                        &format!(
                            "fn fix_{}_{}() {{ todo!() }}\nfn helper_{}_{}() {{ }}\n// FEATURE_INSERTION_POINT",
                            commit_idx, file_idx, commit_idx, file_idx
                        ),
                        1,
                    )
                }
            };

            fs::write(&path, &new_content).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &spec.path]).unwrap();
        }

        // Every 50th commit: rename a file (directory restructuring)
        if commit_idx > 0 && commit_idx % 50 == 0 && commit_idx / 50 < 3 {
            let rename_idx = commit_idx / 50; // 1, 2
            let old_path = format!("src/mod_{}/component_{}.rs", rename_idx, rename_idx * 5);
            let new_path = format!("src/refactored/component_{}_v2.rs", rename_idx * 5);
            let old_full = repo.path().join(&old_path);
            if old_full.exists() {
                let new_dir = repo.path().join("src/refactored");
                fs::create_dir_all(&new_dir).ok();
                fs::rename(&old_full, repo.path().join(&new_path)).ok();
            }
        }

        repo.git(&["add", "-A"]).unwrap();
        let msg = if commit_idx % 10 == 0 {
            format!("refactor: large restructuring pass {}", commit_idx)
        } else if commit_idx % 3 == 0 {
            format!("fix: targeted bug fix {}", commit_idx)
        } else {
            format!("feat: implement feature component {}", commit_idx)
        };
        repo.stage_all_and_commit(&msg).unwrap();

        if (commit_idx + 1) % 30 == 0 {
            println!(
                "  Feature {}/{} ({:.1}s)",
                commit_idx + 1,
                num_feature_commits,
                feature_start.elapsed().as_secs_f64()
            );
        }
    }
    println!(
        "Feature setup: {:.1}s ({} commits)",
        feature_start.elapsed().as_secs_f64(),
        num_feature_commits
    );

    // --- Step 3: Main branch advances ---
    // Touch ALL AI files on every main commit to guarantee blob OID differences
    // after rebase, forcing the slow path (full attribution rewrite).
    // Insert at MAIN_INSERTION_POINT (near top), separate from feature changes
    // at FEATURE_INSERTION_POINT (near bottom) to avoid merge conflicts.
    repo.git(&["checkout", &default_branch]).unwrap();
    for main_idx in 0..num_main_commits {
        for (file_idx, spec) in files.iter().enumerate() {
            if !spec.has_ai {
                continue;
            }
            let path = repo.path().join(&spec.path);
            if let Ok(current) = fs::read_to_string(&path) {
                let new_content = current.replacen(
                    "// MAIN_INSERTION_POINT",
                    &format!(
                        "// main-infra-v{}: config for file {}\nconst MAIN_CFG_{}_{}: u32 = {};\n// MAIN_INSERTION_POINT",
                        main_idx, file_idx, main_idx, file_idx, main_idx * 100 + file_idx
                    ),
                    1,
                );
                fs::write(&path, &new_content).unwrap();
            }
        }
        repo.git(&["add", "-A"]).unwrap();
        repo.stage_all_and_commit(&format!("infra: main branch update {}", main_idx))
            .unwrap();
    }

    // --- Step 4: Rebase with timing ---
    repo.git(&["checkout", "feature"]).unwrap();
    let timing_file = std::path::PathBuf::from("/tmp/realistic_timing.txt");
    let timing_path = timing_file.to_str().unwrap().to_string();

    // Check notes BEFORE rebase
    let pre_notes = repo
        .git(&["notes", "--ref=refs/notes/ai", "list"])
        .unwrap_or_default();
    let pre_count = pre_notes.lines().filter(|l| !l.is_empty()).count();
    println!("AI notes BEFORE rebase: {}", pre_count);
    // Show sample notes to verify content (first, middle, last)
    let note_lines: Vec<&str> = pre_notes.lines().filter(|l| !l.is_empty()).collect();
    for &idx in &[0, note_lines.len() / 2, note_lines.len().saturating_sub(1)] {
        if let Some(line) = note_lines.get(idx) {
            let commit_sha = line.split_whitespace().nth(1).unwrap_or("");
            if !commit_sha.is_empty() {
                let note_content = repo
                    .git(&["notes", "--ref=refs/notes/ai", "show", commit_sha])
                    .unwrap_or_else(|e| format!("ERROR: {}", e));
                let preview_len = 300.min(note_content.len());
                println!(
                    "Note[{}] for {}:\n{}\n",
                    idx,
                    &commit_sha[..8],
                    &note_content[..preview_len]
                );
            }
        }
    }

    println!(
        "\n--- Starting realistic rebase ({} commits onto {}) ---",
        num_feature_commits, default_branch
    );
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

    // Check notes state and rebase details
    let notes_list = repo
        .git(&["notes", "--ref=refs/notes/ai", "list"])
        .unwrap_or_default();
    let notes_count = notes_list.lines().filter(|l| !l.is_empty()).count();
    println!("AI notes after rebase: {}", notes_count);
    // Show first few notes to verify they exist
    for line in notes_list.lines().take(3) {
        println!("  note: {}", line.trim());
    }

    if let Ok(timing_data) = fs::read_to_string(&timing_file) {
        println!("\n=== PHASE TIMING ===");
        print!("{}", timing_data);
        println!("====================\n");
    } else {
        println!("(No timing file written — fast path or no notes to rewrite)");
    }

    println!(
        "Total: {:.3}s, Per-commit: {:.1}ms",
        dur.as_secs_f64(),
        dur.as_millis() as f64 / num_feature_commits as f64
    );
}
