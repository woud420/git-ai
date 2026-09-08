use super::{
    AgentId, Arc, BaseCommit, CheckpointFile, CheckpointKind, CheckpointRequest, HashMap, PathBuf,
    PreparedPathRole, ResolvedCheckpointExecution, execute_resolved_checkpoint_from_daemon,
    find_repository_in_path, setup_repo_with_base_commit,
};

#[test]
fn test_checkpoint_with_staged_changes() {
    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Make changes to the file
    let file_path = repo.path().join(&lines_file);
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("New line added by user\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();

    // Run checkpoint - it should track the changes even though they're staged
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();

    // Verify the checkpoint was created with correct entries
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
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();

    // The bug: when changes are staged, entries_len is 0 instead of 1
    assert_eq!(
        latest.entries.len(),
        1,
        "Should have 1 file entry in checkpoint (staged changes should be tracked)"
    );
}

#[test]
fn test_checkpoint_with_staged_changes_after_previous_checkpoint() {
    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Make first changes and checkpoint
    let file_path = repo.path().join(&lines_file);
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("First change\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();

    // Make second changes - these are staged
    let mut content = std::fs::read_to_string(&file_path).unwrap();
    content.push_str("Second change\n");
    std::fs::write(&file_path, &content).unwrap();
    repo.git(&["add", &lines_file]).unwrap();

    // Run checkpoint again - it should track the staged changes even after a previous checkpoint
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();

    // Verify the checkpoint was created with correct entries
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
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();

    assert_eq!(
        latest.entries.len(),
        1,
        "Second checkpoint: should have 1 file entry in checkpoint (staged changes should be tracked)"
    );
}

#[test]
fn test_checkpoint_with_only_staged_no_unstaged_changes() {
    use std::fs;

    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Get the file path
    let file_path = repo.path().join(&lines_file);

    // Manually modify the file (bypassing TmpFile's automatic staging)
    let mut content = fs::read_to_string(&file_path).unwrap();
    content.push_str("New line for staging test\n");
    fs::write(&file_path, &content).unwrap();

    // Now manually stage it using git (this is what "git add" does)
    repo.git(&["add", &lines_file]).unwrap();

    // At this point: HEAD has old content, index has new content, workdir has new content
    // And unstaged should be "Unmodified" because workdir == index

    // Now run checkpoint
    repo.git_ai(&["checkpoint", "mock_known_human", &lines_file])
        .unwrap();

    // Verify the checkpoint was created with correct entries
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
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();

    // This should work: we should see 1 file with 1 entry
    assert_eq!(
        latest.entries.len(),
        1,
        "Should track the staged changes in checkpoint"
    );
}

#[test]
fn test_checkpoint_with_only_unstaged_changes_for_ai_without_pathspec() {
    use std::fs;

    // Create a repo with an initial commit
    let (repo, lines_file, _) = setup_repo_with_base_commit();

    // Manually modify the file without staging it
    let file_path = repo.path().join(&lines_file);
    let mut content = fs::read_to_string(&file_path).unwrap();
    content.push_str("New unstaged AI line\n");
    fs::write(&file_path, &content).unwrap();

    // Trigger AI checkpoint without edited_filepaths (pathspec-less flow used by some agents)
    repo.git_ai(&["checkpoint", "mock_ai", &lines_file])
        .unwrap();

    // Verify the checkpoint was created
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
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let latest = checkpoints.last().unwrap();

    assert_eq!(
        latest.entries.len(),
        1,
        "Should create an AI checkpoint entry for unstaged changes without pathspecs"
    );
}

#[test]
fn test_checkpoint_base_override_controls_head_context_for_entry_generation() {
    use std::fs;

    let (repo, lines_file, _) = setup_repo_with_base_commit();
    let file_path = repo.path().join(&lines_file);

    fs::write(&file_path, "line from commit A\n").unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.stage_all_and_commit("commit A").unwrap();
    let base_commit = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    fs::write(&file_path, "line from commit B\n").unwrap();
    repo.git(&["add", &lines_file]).unwrap();
    repo.stage_all_and_commit("commit B").unwrap();

    // Keep the worktree dirty so git status returns this file, but inject deterministic
    // content from commit B via the CheckpointFile content field.
    fs::write(&file_path, "line from uncommitted edit\n").unwrap();

    let checkpoint_request = CheckpointRequest {
        trace_id: "base-override-regression".to_string(),
        checkpoint_kind: CheckpointKind::AiAgent,
        agent_id: Some(AgentId {
            tool: "mock_ai".to_string(),
            id: "base-override-regression".to_string(),
            model: "test".to_string(),
        }),
        files: vec![CheckpointFile {
            path: PathBuf::from(&lines_file),
            content: Some("line from commit B\n".to_string()),
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Sha(base_commit.clone()),
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
        delivery_id: None,
    };

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    let mut dirty_files = HashMap::new();
    dirty_files.insert(lines_file.clone(), Arc::from("line from commit B\n"));

    let resolved = ResolvedCheckpointExecution {
        base_commit,
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        files: vec![lines_file],
        dirty_files,
    };

    execute_resolved_checkpoint_from_daemon(
        &gitai_repo,
        "mock-ai",
        CheckpointKind::AiAgent,
        checkpoint_request,
        resolved,
    )
    .unwrap();
}
