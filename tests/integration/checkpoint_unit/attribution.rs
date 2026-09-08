use super::{TestRepo, find_repository_in_path, setup_repo_with_base_commit};

#[test]
fn test_checkpoint_records_conflicted_files() {
    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Get the current branch name (whatever the default is)
    let base_branch = repo.current_branch();

    // Create a branch and make different changes on each branch to create a conflict
    repo.git(&["checkout", "-b", "feature-branch"]).unwrap();

    // On feature branch, modify the file
    let file_path = repo.path().join(&lines_file);
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("Feature branch change\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();
    repo.stage_all_and_commit("Feature commit").unwrap();

    // Switch back to base branch and make conflicting changes
    repo.git(&["checkout", &base_branch]).unwrap();
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("Main branch change\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();
    repo.stage_all_and_commit("Main commit").unwrap();

    // Attempt to merge feature-branch into base branch - this should create a conflict
    let output = repo.git_og(&["merge", "feature-branch"]);
    let has_conflicts = output.is_err();
    assert!(has_conflicts, "Should have merge conflicts");

    // Try to checkpoint while there are conflicts
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let base_commit = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints_before = working_log.read_all_checkpoints().unwrap();
    let count_before = checkpoints_before.len();

    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();

    // Checkpoints record conflicted files so conflict-resolution attribution can be
    // merged into the eventual rebase/merge commit.
    let checkpoints_after = working_log.read_all_checkpoints().unwrap();
    assert!(
        checkpoints_after.len() > count_before,
        "Should create a checkpoint for conflicted files"
    );
    let latest = checkpoints_after.last().unwrap();
    assert!(
        latest.entries.iter().any(|entry| entry.file == lines_file),
        "Should record an entry for the conflicted file"
    );
}

#[test]
fn test_checkpoint_works_after_conflict_resolution_maintains_authorship() {
    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Get the current branch name (whatever the default is)
    let base_branch = repo.current_branch();

    // Checkpoint initial state to track the base authorship
    let file_path = repo.path().join(&lines_file);
    let initial_content = std::fs::read_to_string(&file_path).unwrap();
    println!("Initial content:\n{}", initial_content);

    // Create a branch and make changes
    repo.git(&["checkout", "-b", "feature-branch"]).unwrap();
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("Feature line 1\n");
    content.push_str("Feature line 2\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", &lines_file])
        .unwrap();
    repo.stage_all_and_commit("Feature commit").unwrap();

    // Switch back to base branch and make conflicting changes
    repo.git(&["checkout", &base_branch]).unwrap();
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("Main line 1\n");
    content.push_str("Main line 2\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();
    repo.stage_all_and_commit("Main commit").unwrap();

    // Attempt to merge feature-branch into base branch - this should create a conflict
    let output = repo.git_og(&["merge", "feature-branch"]);
    let has_conflicts = output.is_err();
    assert!(has_conflicts, "Should have merge conflicts");

    // While there are conflicts, checkpoint should still record the file so the
    // eventual resolution can carry explicit attribution.
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let base_commit = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints_before_conflict_checkpoint = working_log.read_all_checkpoints().unwrap();
    let count_before = checkpoints_before_conflict_checkpoint.len();

    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();

    // Checkpoint should record conflicted files during the conflict.
    let checkpoints_after_conflict_checkpoint = working_log.read_all_checkpoints().unwrap();
    assert!(
        checkpoints_after_conflict_checkpoint.len() > count_before,
        "Should create a checkpoint for conflicted files"
    );
    let checkpoint_during_conflict = checkpoints_after_conflict_checkpoint.last().unwrap();
    assert!(
        checkpoint_during_conflict
            .entries
            .iter()
            .any(|entry| entry.file == lines_file),
        "Should record conflicted files during conflict"
    );

    // Resolve the conflict by choosing "ours" (base branch)
    repo.git_og(&["checkout", "--ours", &lines_file]).unwrap();
    repo.git(&["add", &lines_file]).unwrap();

    // Verify content to ensure the resolution was applied correctly
    let resolved_content = std::fs::read_to_string(&file_path).unwrap();
    println!("Resolved content after resolution:\n{}", resolved_content);
    assert!(
        resolved_content.contains("Main line 1"),
        "Should contain base branch content (we chose 'ours')"
    );
    assert!(
        resolved_content.contains("Main line 2"),
        "Should contain base branch content (we chose 'ours')"
    );
    assert!(
        !resolved_content.contains("Feature line 1"),
        "Should not contain feature branch content (we chose 'ours')"
    );

    // After resolution, make additional changes to test that checkpointing works again
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("Post-resolution line 1\n");
    content.push_str("Post-resolution line 2\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();

    // Now checkpoint should work and track the new changes
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();

    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();

    println!(
        "After resolution and new changes: entries_len={}",
        latest.entries.len()
    );

    // The file should be tracked with the new changes
    assert_eq!(
        latest.entries.len(),
        1,
        "Should create 1 entry for new changes after conflict resolution"
    );
}

#[test]
fn test_known_human_checkpoint_without_ai_history_records_h_hash_attributions() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("simple.txt"), "one\n").unwrap();
    repo.git(&["add", "simple.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "simple.txt"])
        .unwrap();
    repo.stage_all_and_commit("seed commit").unwrap();

    let file_path = repo.path().join("simple.txt");
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("two\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", "simple.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "simple.txt"])
        .unwrap();

    let gitai_repo =
        find_repository_in_path(repo.path().to_str().unwrap()).expect("Repository should exist");
    let base_commit = gitai_repo
        .head()
        .ok()
        .and_then(|head| head.target().ok())
        .unwrap_or_else(|| "initial".to_string());
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();
    let entry = latest
        .entries
        .iter()
        .find(|entry| entry.file == "simple.txt")
        .unwrap();

    // KnownHuman checkpoints always record h_<hash> line attributions, even with no AI history.
    // This allows downstream stats to count these lines as human_additions.
    assert!(
        !entry.line_attributions.is_empty(),
        "KnownHuman checkpoint should record line-level h_<hash> attributions"
    );
    assert!(
        entry
            .line_attributions
            .iter()
            .all(|la| la.author_id.starts_with("h_")),
        "All line attributions should be h_<hash> IDs"
    );
    assert!(
        latest.line_stats.additions > 0,
        "KnownHuman checkpoint should record line stats"
    );
}

#[test]
fn test_human_checkpoint_keeps_attributions_for_ai_touched_file() {
    let (repo, lines_file, alphabet_file) = setup_repo_with_base_commit();

    let lines_path = repo.path().join(&lines_file);
    let alphabet_path = repo.path().join(&alphabet_file);

    let mut content = std::fs::read_to_string(&lines_path).unwrap();
    content.push_str("ai change\n");
    std::fs::write(&lines_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", &lines_file])
        .unwrap();

    let mut lines_content = std::fs::read_to_string(&lines_path).unwrap();
    lines_content.push_str("human after ai\n");
    std::fs::write(&lines_path, &lines_content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();

    let mut alphabet_content = std::fs::read_to_string(&alphabet_path).unwrap();
    alphabet_content.push_str("human only\n");
    std::fs::write(&alphabet_path, &alphabet_content).unwrap();
    repo.git(&["add", &alphabet_file]).unwrap();

    repo.git_ai(&[
        "checkpoint",
        "mock_known_human",
        &lines_file,
        &alphabet_file,
    ])
    .unwrap();

    let gitai_repo =
        find_repository_in_path(repo.path().to_str().unwrap()).expect("Repository should exist");
    let base_commit = gitai_repo
        .head()
        .ok()
        .and_then(|head| head.target().ok())
        .unwrap_or_else(|| "initial".to_string());
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();

    let ai_touched_entry = latest
        .entries
        .iter()
        .find(|entry| entry.file == "lines.md")
        .unwrap();
    assert!(
        !ai_touched_entry.attributions.is_empty() || !ai_touched_entry.line_attributions.is_empty(),
        "AI-touched file should keep attribution tracking"
    );

    let human_only_entry = latest
        .entries
        .iter()
        .find(|entry| entry.file == "alphabet.md")
        .unwrap();
    // KnownHuman checkpoints record h_<hash> attributions for all files, including
    // files with no AI history. This ensures human lines are counted correctly in stats.
    assert!(
        !human_only_entry.line_attributions.is_empty(),
        "KnownHuman checkpoint should record line attributions for human-only files"
    );
    assert!(
        human_only_entry
            .line_attributions
            .iter()
            .all(|la| la.author_id.starts_with("h_")),
        "Human-only file attributions should all be h_<hash> IDs"
    );
}
