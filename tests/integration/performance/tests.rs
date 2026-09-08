use super::{
    Duration, FindRandomFilesOptions, OpenOptions, Sampler, Write, find_random_files,
    find_random_files_with_options, get_performance_repos,
};
use rand::seq::IndexedRandom;
use rstest::rstest;

// Performance floor constant (270ms) - used to determine if overhead is acceptable
const PERFORMANCE_FLOOR_MS: Duration = Duration::from_millis(270);

#[rstest]
#[case("chromium")]
#[case("react")]
#[case("node")]
#[case("chakracore")]
#[ignore]
fn test_human_only_edits_then_commit(#[case] repo_name: &str) {
    use std::time::Instant;

    let repos = get_performance_repos();
    let test_repo = repos
        .get(repo_name)
        .unwrap_or_else(|| panic!("{} repo should be available", repo_name));
    // Find random files for testing
    println!("Finding random files for {}", repo_name);
    let start = Instant::now();
    let random_files = find_random_files(test_repo).expect("Should find random files");
    let duration = start.elapsed();
    println!("Time taken to find random files: {:?}", duration);
    // Select 3 random files (not large ones)
    let files_to_edit: Vec<String> = random_files.random_files.iter().take(3).cloned().collect();

    assert!(
        files_to_edit.len() >= 3,
        "Should have at least 3 random files to edit"
    );

    // Create a sampler that runs 10 times
    let sampler = Sampler::new(10);

    // Sample the performance of human-only edits + commit
    let result = sampler.sample(test_repo, |repo| {
        // Append "# Human Line" to each file
        for file_path in &files_to_edit {
            let full_path = repo.path().join(file_path);

            let mut file = OpenOptions::new()
                .append(true)
                .open(&full_path)
                .unwrap_or_else(|_| panic!("Should be able to open file: {}", file_path));

            file.write_all(b"\n# Human Line\n")
                .unwrap_or_else(|_| panic!("Should be able to write to file: {}", file_path));
        }

        // Stage the files (regular git, no benchmark)
        for file_path in &files_to_edit {
            repo.git(&["add", file_path])
                .unwrap_or_else(|_| panic!("Should be able to stage file: {}", file_path));
        }

        // Benchmark the commit operation (where pre-commit hook runs)
        repo.benchmark_git(&["commit", "-m", "Human-only edits"])
            .expect("Commit should succeed")
    });

    // Print the results
    result.print_summary(&format!("Human-only edits + commit ({})", repo_name));

    let (percent_overhead, average_overhead) = result.average_overhead();

    assert!(
        percent_overhead < 10.0 || average_overhead < PERFORMANCE_FLOOR_MS,
        "Average overhead should be less than 10% or under 70ms"
    );
}

#[rstest]
#[case("chromium")]
#[case("react")]
#[case("node")]
#[case("chakracore")]
#[ignore]
fn test_ai_and_human_edits(#[case] repo_name: &str) {
    let repos = get_performance_repos();
    let test_repo = repos
        .get(repo_name)
        .unwrap_or_else(|| panic!("{} repo should be available", repo_name));
    // Find random files for testing
    let random_files = find_random_files(test_repo).expect("Should find random files");

    // Select 3 random files (not large ones)
    let files_to_edit: Vec<String> = random_files.random_files.iter().take(3).cloned().collect();

    assert!(
        files_to_edit.len() >= 3,
        "Should have at least 3 random files to edit"
    );

    // Create a sampler that runs 10 times
    let sampler = Sampler::new(10);

    // Sample the performance of AI and human edits + commit
    let result = sampler.sample(test_repo, |repo| {
        for file_path in &files_to_edit {
            let full_path = repo.path().join(file_path);

            // Step 1: Append "# Human Line" to the file
            {
                let mut file = OpenOptions::new()
                    .append(true)
                    .open(&full_path)
                    .unwrap_or_else(|_| panic!("Should be able to open file: {}", file_path));

                file.write_all(b"\n# Human Line\n")
                    .unwrap_or_else(|_| panic!("Should be able to write to file: {}", file_path));
            }

            // Step 2: Run git-ai checkpoint
            repo.git_ai(&["checkpoint", file_path])
                .unwrap_or_else(|_| panic!("Should be able to checkpoint file: {}", file_path));

            // Step 3: Insert "# AI Line" at the top of the file
            {
                let content = std::fs::read_to_string(&full_path)
                    .unwrap_or_else(|_| panic!("Should be able to read file: {}", file_path));

                let new_content = format!("# AI Line\n{}", content);

                std::fs::write(&full_path, new_content)
                    .unwrap_or_else(|_| panic!("Should be able to write to file: {}", file_path));
            }

            // Step 4: Run git-ai mock_ai
            repo.git_ai(&["checkpoint", "mock_ai", file_path])
                .unwrap_or_else(|_| panic!("Should be able to mock_ai file: {}", file_path));
        }

        // Benchmark the commit operation (where pre-commit hook runs)
        repo.benchmark_git(&["commit", "-a", "-m", "AI and human edits"])
            .expect("Commit should succeed")
    });

    // Print the results
    result.print_summary(&format!("AI and human edits + commit ({})", repo_name));

    let (percent_overhead, average_overhead) = result.average_overhead();

    assert!(
        percent_overhead < 10.0 || average_overhead < PERFORMANCE_FLOOR_MS,
        "Average overhead should be less than 10% or under 70ms"
    );
}

#[rstest]
#[case("chromium")]
#[case("react")]
#[case("node")]
#[case("chakracore")]
#[ignore]
fn test_git_reset_head_5_mixed(#[case] repo_name: &str) {
    let repos = get_performance_repos();
    let test_repo = repos
        .get(repo_name)
        .unwrap_or_else(|| panic!("{} repo should be available", repo_name));

    // Create a sampler that runs 10 times
    let sampler = Sampler::new(10);

    // Sample the performance of git reset HEAD~5 --mixed
    let result = sampler.sample(test_repo, |repo| {
        // Benchmark the reset operation with explicit --mixed flag
        repo.benchmark_git(&["reset", "HEAD~5", "--mixed"])
            .expect("Reset should succeed")
    });

    // Print the results
    result.print_summary(&format!("git reset HEAD~5 --mixed ({})", repo_name));

    let (percent_overhead, _) = result.average_overhead();

    assert!(
        percent_overhead < 20.0,
        "Average overhead should be less than 20%"
    );
}

#[rstest]
#[case("chromium")]
#[case("react")]
#[case("node")]
#[case("chakracore")]
#[ignore]
fn test_human_only_edits_in_big_files_then_commit(#[case] repo_name: &str) {
    let repos = get_performance_repos();
    let test_repo = repos
        .get(repo_name)
        .unwrap_or_else(|| panic!("{} repo should be available", repo_name));

    // Find random files for testing
    let random_files = find_random_files(test_repo).expect("Should find random files");

    // Use large files for testing
    let files_to_edit: Vec<String> = random_files.large_files.clone();

    assert!(
        !files_to_edit.is_empty(),
        "Should have at least 1 large file to edit"
    );

    // Create a sampler that runs 10 times
    let sampler = Sampler::new(10);

    // Sample the performance of human-only edits + commit on large files
    let result = sampler.sample(test_repo, |repo| {
        // Append "# Human Line" to each file
        for file_path in &files_to_edit {
            let full_path = repo.path().join(file_path);

            let mut file = OpenOptions::new()
                .append(true)
                .open(&full_path)
                .unwrap_or_else(|_| panic!("Should be able to open file: {}", file_path));

            file.write_all(b"\n# Human Line\n")
                .unwrap_or_else(|_| panic!("Should be able to write to file: {}", file_path));
        }

        // Stage the files (regular git, no benchmark)
        for file_path in &files_to_edit {
            repo.git(&["add", file_path])
                .unwrap_or_else(|_| panic!("Should be able to stage file: {}", file_path));
        }

        // Benchmark the commit operation (where pre-commit hook runs)
        repo.benchmark_git(&["commit", "-m", "Human-only edits in big files"])
            .expect("Commit should succeed")
    });

    // Print the results
    result.print_summary(&format!(
        "Human-only edits in big files + commit ({})",
        repo_name
    ));

    let (percent_overhead, average_overhead) = result.average_overhead();

    assert!(
        percent_overhead < 10.0 || average_overhead < PERFORMANCE_FLOOR_MS,
        "Average overhead should be less than 10% or under 70ms"
    );
}

#[rstest]
#[case("chromium")]
#[case("react")]
#[case("node")]
#[case("chakracore")]
#[ignore]
fn test_git_reset_head_5(#[case] repo_name: &str) {
    let repos = get_performance_repos();
    let test_repo = repos
        .get(repo_name)
        .unwrap_or_else(|| panic!("{} repo should be available", repo_name));

    // Create a sampler that runs 10 times
    let sampler = Sampler::new(10);

    // Sample the performance of git reset HEAD~5
    let result = sampler.sample(test_repo, |repo| {
        // Benchmark the reset operation (--mixed is the default)
        repo.benchmark_git(&["reset", "HEAD~5"])
            .expect("Reset should succeed")
    });

    // Print the results
    result.print_summary(&format!("git reset HEAD~5 ({})", repo_name));

    let (percent_overhead, _) = result.average_overhead();

    assert!(
        percent_overhead < 20.0,
        "Average overhead should be less than 20%"
    );
}

#[rstest]
#[case("chromium")]
#[case("react")]
#[case("node")]
#[case("chakracore")]
#[ignore]
fn test_large_checkpoints(#[case] repo_name: &str) {
    use std::time::Instant;

    let repos = get_performance_repos();
    let test_repo = repos
        .get(repo_name)
        .unwrap_or_else(|| panic!("{} repo should be available", repo_name));

    // Find 1000 random files for testing
    println!("Finding 1000 random files for {}", repo_name);
    let start = Instant::now();
    let random_files = find_random_files_with_options(
        test_repo,
        FindRandomFilesOptions {
            random_file_count: 2200,
            large_file_count: 0,
        },
    )
    .expect("Should find random files");
    let duration = start.elapsed();
    println!("Time taken to find random files: {:?}", duration);

    let all_files: Vec<String> = random_files.random_files;
    println!("Found {} files to edit", all_files.len());

    // Create a sampler that runs 5 times (fewer due to the large number of files)
    let sampler = Sampler::new(5);

    // Sample the performance of large checkpoint operations
    let result = sampler.sample(test_repo, |repo| {
        // Step 1: Edit all 1000 files (simulating AI edits)
        println!("Editing {} files...", all_files.len());
        for file_path in &all_files {
            let full_path = repo.path().join(file_path);

            let mut file = OpenOptions::new()
                .append(true)
                .open(&full_path)
                .unwrap_or_else(|_| panic!("Should be able to open file: {}", file_path));

            file.write_all(b"\n# AI Generated Line\n")
                .unwrap_or_else(|_| panic!("Should be able to write to file: {}", file_path));
        }

        // Step 2: Run git-ai checkpoint mock_ai -- <all pathspecs>
        println!("Running checkpoint mock_ai on {} files...", all_files.len());
        let mut checkpoint_args: Vec<&str> = vec!["checkpoint", "mock_ai", "--"];
        let all_files_refs: Vec<&str> = all_files.iter().map(|s| s.as_str()).collect();
        checkpoint_args.extend(all_files_refs.iter());

        repo.git_ai(&checkpoint_args)
            .expect("Checkpoint mock_ai should succeed");

        // Step 3: Select 100 random files from the 1000 and edit them (simulating human edits)
        let mut rng = rand::rng();
        let files_to_re_edit: Vec<String> = all_files
            .sample(&mut rng, 100.min(all_files.len()))
            .cloned()
            .collect();

        println!(
            "Re-editing {} files (human edits)...",
            files_to_re_edit.len()
        );
        for file_path in &files_to_re_edit {
            let full_path = repo.path().join(file_path);

            let mut file = OpenOptions::new()
                .append(true)
                .open(&full_path)
                .unwrap_or_else(|_| panic!("Should be able to open file: {}", file_path));

            file.write_all(b"\n# Human Line\n")
                .unwrap_or_else(|_| panic!("Should be able to write to file: {}", file_path));
        }

        // Step 4: Benchmark the checkpoint on the 100 human-edited files
        println!(
            "Benchmarking checkpoint on {} files...",
            files_to_re_edit.len()
        );
        let mut final_checkpoint_args: Vec<&str> = vec!["checkpoint", "--"];
        let files_to_re_edit_refs: Vec<&str> =
            files_to_re_edit.iter().map(|s| s.as_str()).collect();
        final_checkpoint_args.extend(files_to_re_edit_refs.iter());

        repo.benchmark_git_ai(&final_checkpoint_args)
            .expect("Checkpoint should succeed")
    });

    // Print the results
    result.print_summary(&format!("Large checkpoints ({})", repo_name));

    // For checkpoint operations, we measure time per file
    // The benchmark is on 100 files, so we calculate ms per file
    let files_benchmarked = 100;
    let avg_total_ms = result.average.total_duration.as_millis() as f64;
    let ms_per_file = avg_total_ms / files_benchmarked as f64;

    println!(
        "Average total time: {:.2}ms, Files: {}, Time per file: {:.2}ms",
        avg_total_ms, files_benchmarked, ms_per_file
    );

    // Assert that checkpoint takes less than 50ms per file on average
    assert!(
        ms_per_file < 50.0,
        "Checkpoint should take less than 50ms per file, got {:.2}ms per file",
        ms_per_file
    );
}
