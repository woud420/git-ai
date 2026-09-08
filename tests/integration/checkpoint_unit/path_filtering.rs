use super::{
    AgentId, Arc, BaseCommit, Checkpoint, CheckpointFile, CheckpointKind, CheckpointRequest,
    HashMap, PreparedPathRole, ResolvedCheckpointExecution, TestRepo, WorkingLogEntry,
    execute_resolved_checkpoint_from_daemon, find_repository_in_path, setup_repo_with_base_commit,
};

#[test]
fn test_checkpoint_with_paths_outside_repo() {
    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Make changes to the file
    let file_path = repo.path().join(&lines_file);
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("New line added\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let base_commit = gitai_repo.head().unwrap().target().unwrap();

    // Build a resolved checkpoint with only the valid file (outside paths filtered at resolution)
    let resolved = ResolvedCheckpointExecution {
        base_commit: base_commit.clone(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        files: vec![lines_file.clone()],
        dirty_files: HashMap::from([(lines_file.clone(), Arc::from(content.clone()))]),
    };

    let checkpoint_request = CheckpointRequest {
        trace_id: "test-outside-paths".to_string(),
        checkpoint_kind: CheckpointKind::AiAgent,
        agent_id: Some(AgentId {
            tool: "test_tool".to_string(),
            id: "test_session".to_string(),
            model: "test_model".to_string(),
        }),
        files: vec![CheckpointFile {
            path: file_path,
            content: Some(content.clone()),
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Sha(base_commit),
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
        delivery_id: None,
    };

    let result = execute_resolved_checkpoint_from_daemon(
        &gitai_repo,
        "test_user",
        CheckpointKind::AiAgent,
        checkpoint_request,
        resolved,
    );

    assert!(
        result.is_ok(),
        "Checkpoint should succeed: {:?}",
        result.err()
    );
}

#[test]
fn test_checkpoint_filters_external_paths_from_stored_checkpoints() {
    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Get access to the working log storage
    let gitai_repo =
        find_repository_in_path(repo.path().to_str().unwrap()).expect("Repository should exist");
    let base_commit = gitai_repo
        .head()
        .ok()
        .and_then(|head| head.target().ok())
        .unwrap_or_else(|| "initial".to_string());

    // Manually inject a checkpoint with an external file path (simulating the bug)
    // This is what happens when a file outside the repo was tracked before the fix
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();

    let external_blob = working_log
        .persist_file_version("external fixture\n")
        .expect("persist external checkpoint fixture");
    let external_entry = WorkingLogEntry::new(
        "/external/path/outside/repo.txt".to_string(),
        external_blob,
        vec![],
        vec![],
    );

    let fake_checkpoint = Checkpoint::new(
        CheckpointKind::Human,
        "fake_diff".to_string(),
        "test_author".to_string(),
        vec![external_entry],
    );

    // Store the checkpoint with external path
    working_log
        .append_checkpoint(&fake_checkpoint)
        .expect("Should be able to append checkpoint");

    // Now make actual changes to a file in the repo
    let file_path = repo.path().join(&lines_file);
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("New line for testing\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();

    // Run checkpoint - this should NOT crash even though there's an external path stored
    // Previously this would fail with: "fatal: /external/path/outside/repo.txt is outside repository"
    let result = repo.git_ai(&["checkpoint", "mock_known_human", &lines_file]);

    assert!(
        result.is_ok(),
        "Checkpoint should succeed even with external paths stored in previous checkpoints: {:?}",
        result.err()
    );

    // Verify the new checkpoint only processed the valid file
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap();
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();

    // Should only process the valid file in the repo
    assert_eq!(
        latest.entries.len(),
        1,
        "Should process 1 valid file (external path should be filtered)"
    );
}

#[test]
fn test_checkpoint_skips_default_ignored_files() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("README.md"), "# repo\n").unwrap();
    repo.git(&["add", "README.md"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    std::fs::write(repo.path().join("README.md"), "# repo\n\nupdated\n").unwrap();
    std::fs::write(repo.path().join("Cargo.lock"), "# lock\n# lock2\n").unwrap();

    // Checkpoint both files explicitly (CLI doesn't support "." the same way)
    repo.git(&["add", "README.md"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "README.md"])
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

    // Should have at least one checkpoint
    assert!(
        !checkpoints.is_empty(),
        "Should have at least one checkpoint"
    );
    let latest = checkpoints.last().unwrap();

    assert!(
        latest.entries.iter().any(|entry| entry.file == "README.md"),
        "Expected non-ignored source file to be checkpointed"
    );
    assert!(
        latest
            .entries
            .iter()
            .all(|entry| entry.file != "Cargo.lock"),
        "Expected Cargo.lock to be filtered by default ignore patterns"
    );
}

#[test]
fn test_checkpoint_skips_linguist_generated_files_from_root_gitattributes() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("README.md"), "# repo\n").unwrap();
    repo.git(&["add", "README.md"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    std::fs::write(
        repo.path().join(".gitattributes"),
        "generated/** linguist-generated\n",
    )
    .unwrap();
    repo.git(&["add", ".gitattributes"]).unwrap();
    repo.stage_all_and_commit("attrs").unwrap();

    std::fs::create_dir_all(repo.path().join("generated")).unwrap();
    std::fs::write(
        repo.path().join("generated").join("api.generated.ts"),
        "// generated\n// generated 2\n",
    )
    .unwrap();
    std::fs::write(repo.path().join("main.rs"), "fn main() {}\n").unwrap();
    repo.git(&["add", "main.rs"]).unwrap();

    // Checkpoint the non-generated file
    repo.git_ai(&["checkpoint", "mock_known_human", "main.rs"])
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

    // Should have at least one checkpoint
    assert!(
        !checkpoints.is_empty(),
        "Should have at least one checkpoint"
    );
    let latest = checkpoints.last().unwrap();

    assert!(
        latest.entries.iter().any(|entry| entry.file == "main.rs"),
        "Expected non-generated file to be checkpointed"
    );
    assert!(
        latest
            .entries
            .iter()
            .all(|entry| entry.file != "generated/api.generated.ts"),
        "Expected linguist-generated file to be filtered via .gitattributes"
    );
}

/// When an AI agent deletes a file, the checkpoint should still be recorded (not silently
/// dropped). The scoped post-edit checkpoint fires with the deleted file's path — the file
/// no longer exists on disk, so the orchestrator must set content = Some("") and pass it
/// through to the daemon so the deletion is tracked in the working log.
#[test]
fn test_scoped_checkpoint_records_file_deletion() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("to_delete.txt");

    // Create file and commit with known human attribution
    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "to_delete.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial commit").unwrap();

    // AI agent pre-edit snapshot (captures before state)
    repo.git_ai(&["checkpoint", "human", "to_delete.txt"])
        .unwrap();

    // AI deletes the file
    std::fs::remove_file(&file_path).unwrap();

    // AI agent post-edit checkpoint on the now-deleted file
    let checkpoint_result = repo.git_ai(&["checkpoint", "mock_ai", "to_delete.txt"]);
    assert!(
        checkpoint_result.is_ok(),
        "Checkpoint on deleted file should succeed, got: {:?}",
        checkpoint_result.err()
    );

    // Verify the checkpoint was recorded in the working log with deletion stats
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let working_log = gitai_repo
        .storage
        .working_log_for_base_commit(&head_sha)
        .unwrap();
    let checkpoints = working_log.read_all_checkpoints().unwrap();

    // The AI post-edit checkpoint should be recorded (human pre-edit is a no-op since
    // the file hadn't changed relative to HEAD at that point)
    assert!(
        !checkpoints.is_empty(),
        "At least one checkpoint should be recorded for the deletion"
    );

    // The AI checkpoint should reference to_delete.txt and record 3 deleted lines
    let ai_checkpoint = checkpoints
        .iter()
        .find(|cp| cp.kind.is_ai())
        .expect("Should have an AI checkpoint");
    assert!(
        ai_checkpoint.kind.is_ai(),
        "Last checkpoint should be AI, got {:?}",
        ai_checkpoint.kind
    );
    let has_file = ai_checkpoint
        .entries
        .iter()
        .any(|e| e.file == "to_delete.txt");
    assert!(
        has_file,
        "AI checkpoint should reference to_delete.txt, entries: {:?}",
        ai_checkpoint
            .entries
            .iter()
            .map(|e| &e.file)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        ai_checkpoint.line_stats.deletions, 3,
        "AI checkpoint should record 3 deleted lines, got {}",
        ai_checkpoint.line_stats.deletions
    );
}
