use super::{
    AgentId, Arc, BaseCommit, CheckpointFile, CheckpointKind, CheckpointRequest, ExpectedLineExt,
    HashMap, PathBuf, PreparedPathRole, ResolvedCheckpointExecution, TestRepo,
    execute_resolved_checkpoint_from_daemon, find_repository_in_path, is_ai_author_id,
    setup_repo_with_base_commit,
};

#[test]
fn test_checkpoint_identical_multi_file_content_shares_one_blob() {
    const SHARED_CONTENT: &str = "shared AI line\n";
    const SHARED_SHA: &str = "8103ca83e93b5ec9b0206005fe93af428048450769bca7216501f8f00a251f31";

    let repo = TestRepo::new();
    std::fs::write(repo.path().join("a.txt"), "base line\n").unwrap();
    std::fs::write(repo.path().join("b.txt"), "base line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "a.txt", "b.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial content").unwrap();

    let mut file_a = repo.filename("a.txt");
    let mut file_b = repo.filename("b.txt");
    file_a.assert_committed_lines(crate::lines!["base line".human()]);
    file_b.assert_committed_lines(crate::lines!["base line".human()]);

    repo.git_ai(&["checkpoint", "human", "a.txt", "b.txt"])
        .unwrap();
    let blobs_before = std::fs::read_dir(repo.current_working_logs().dir.join("blobs"))
        .map(|entries| entries.count())
        .unwrap_or(0);

    std::fs::write(repo.path().join("a.txt"), SHARED_CONTENT).unwrap();
    std::fs::write(repo.path().join("b.txt"), SHARED_CONTENT).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "a.txt", "b.txt"])
        .unwrap();

    let working_log = repo.current_working_logs();
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let checkpoint = checkpoints
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.kind.is_ai())
        .expect("AI checkpoint should be present");
    let mut entries = checkpoint
        .entries
        .iter()
        .map(|entry| (entry.file.as_str(), entry.blob_sha.as_str()))
        .collect::<Vec<_>>();
    entries.sort_unstable_by(|left, right| left.0.cmp(right.0));

    assert_eq!(entries, vec![("a.txt", SHARED_SHA), ("b.txt", SHARED_SHA)]);
    let blobs_dir = working_log.dir.join("blobs");
    assert_eq!(
        std::fs::read(blobs_dir.join(SHARED_SHA)).unwrap(),
        SHARED_CONTENT.as_bytes()
    );
    assert_eq!(
        std::fs::read_dir(blobs_dir).unwrap().count(),
        blobs_before + 1,
        "two identical file states should add exactly one blob"
    );

    repo.stage_all_and_commit("shared AI content").unwrap();
    file_a.assert_committed_lines(crate::lines!["shared AI line".ai()]);
    file_b.assert_committed_lines(crate::lines!["shared AI line".ai()]);
}

#[test]
fn test_ai_checkpoint_without_agent_id_is_rejected() {
    let (repo, lines_file, _) = setup_repo_with_base_commit();
    let file_path = repo.path().join(&lines_file);
    let base_commit = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let content = "changed without agent identity\n";
    std::fs::write(&file_path, content).unwrap();

    let checkpoint_request = CheckpointRequest {
        trace_id: "missing-agent-regression".to_string(),
        checkpoint_kind: CheckpointKind::AiAgent,
        agent_id: None,
        files: vec![CheckpointFile {
            path: PathBuf::from(&lines_file),
            content: Some(content.to_string()),
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Sha(base_commit.clone()),
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
        delivery_id: None,
    };

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let resolved = ResolvedCheckpointExecution {
        base_commit,
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        files: vec![lines_file.clone()],
        dirty_files: HashMap::from([(lines_file, Arc::from(content))]),
    };

    let error = execute_resolved_checkpoint_from_daemon(
        &gitai_repo,
        "mock-ai",
        CheckpointKind::AiAgent,
        checkpoint_request,
        resolved,
    )
    .expect_err("AI checkpoints must carry an agent_id");

    assert!(matches!(error, git_ai::error::GitAiError::Persistence(_)));
    assert_eq!(
        error.to_string(),
        "Generic error: AI checkpoint is missing agent_id"
    );
}

#[test]
fn test_checkpoint_without_captured_file_content_uses_structured_error() {
    let (repo, lines_file, _) = setup_repo_with_base_commit();
    let base_commit = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let checkpoint_request = CheckpointRequest {
        trace_id: "missing-captured-content-regression".to_string(),
        checkpoint_kind: CheckpointKind::AiAgent,
        agent_id: Some(AgentId {
            tool: "mock_ai".to_string(),
            id: "missing-captured-content-regression".to_string(),
            model: "test".to_string(),
        }),
        files: vec![CheckpointFile {
            path: PathBuf::from(&lines_file),
            content: None,
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Sha(base_commit.clone()),
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
        delivery_id: None,
    };

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let resolved = ResolvedCheckpointExecution {
        base_commit,
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        files: vec![lines_file.clone()],
        dirty_files: HashMap::new(),
    };

    let error = execute_resolved_checkpoint_from_daemon(
        &gitai_repo,
        "mock-ai",
        CheckpointKind::AiAgent,
        checkpoint_request,
        resolved,
    )
    .expect_err("checkpoint processing must not fall back to the live filesystem");

    assert!(matches!(error, git_ai::error::GitAiError::Persistence(_)));
    assert_eq!(
        error.to_string(),
        format!(
            "Generic error: save_current_file_states: file '{}' not found in dirty_files snapshot (filesystem fallback is not allowed in checkpoint flow)",
            lines_file
        )
    );
}

#[test]
fn test_checkpoint_stale_crlf_blob_causes_ai_reattribution() {
    // Regression coverage: when a CRLF-only change is
    // skipped (preserving a stale CRLF blob), the NEXT AI checkpoint compares
    // the stale CRLF blob against the LF working tree. Because
    // capture_diff_slices sees "line\r\n" ≠ "line\n", ALL lines appear changed.
    // With force_split=true in AI checkpoints, every "changed" line gets
    // re-attributed to AI — even human-written lines.
    //
    // The fix: when content differs only in line endings, update the blob
    // to LF (preserving attributions) so future diffs are LF-vs-LF.
    let repo = TestRepo::new();
    let crlf_initial = "human_line1\r\nhuman_line2\r\nhuman_line3\r\n";
    std::fs::write(repo.path().join("test.txt"), crlf_initial).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.stage_all_and_commit("initial commit with CRLF")
        .unwrap();

    // Step 1: Human checkpoint on CRLF file → creates entry with CRLF blob
    // (need to add a line so the checkpoint creates an entry)
    let crlf_with_edit = "human_line1\r\nhuman_line2\r\nhuman_line3\r\nhuman_line4\r\n";
    std::fs::write(repo.path().join("test.txt"), crlf_with_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Step 2: Convert file to LF (same content, only line endings change)
    let lf_with_edit = "human_line1\nhuman_line2\nhuman_line3\nhuman_line4\n";
    std::fs::write(repo.path().join("test.txt"), lf_with_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Step 3: AI adds one line (LF) → AI checkpoint
    let lf_with_ai = "human_line1\nhuman_line2\nhuman_line3\nhuman_line4\nai_new_line\n";
    std::fs::write(repo.path().join("test.txt"), lf_with_ai).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Read the AI checkpoint
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

    // Find the AI checkpoint entry for test.txt
    let ai_checkpoint = checkpoints
        .iter()
        .rev()
        .find(|cp| cp.kind.is_ai() && cp.entries.iter().any(|e| e.file == "test.txt"))
        .expect("Should have an AI checkpoint with test.txt");
    let test_entry = ai_checkpoint
        .entries
        .iter()
        .find(|e| e.file == "test.txt")
        .unwrap();

    // The key assertion: the AI checkpoint should NOT attribute all lines to AI.
    // Only the actually-added line should be AI-attributed.
    let ai_line_attrs: Vec<_> = test_entry
        .line_attributions
        .iter()
        .filter(|la| is_ai_author_id(&la.author_id))
        .collect();

    // Count total lines covered by AI attributions
    let ai_line_count: u32 = ai_line_attrs
        .iter()
        .map(|la| la.end_line - la.start_line + 1)
        .sum();

    // AI should only attribute 1 line (the new ai_new_line), not all 5 lines.
    // If the stale CRLF blob caused full re-attribution, ai_line_count would be 5.
    assert!(
        ai_line_count <= 2,
        "AI should attribute at most 1-2 lines (the actual addition), \
         but attributed {} lines — stale CRLF blob caused full re-attribution. \
         AI attributions: {:?}, all attributions: {:?}",
        ai_line_count,
        ai_line_attrs,
        test_entry.line_attributions
    );
}

/// Regression test: INITIAL attributions without stored file_blobs are invalid.
/// Line attributions are only meaningful relative to the exact file snapshot
/// they describe, so checkpointing must fail loudly instead of guessing from
/// dirty-file content.
#[test]
fn test_checkpoint_fails_with_initial_missing_blobs() {
    let repo = TestRepo::new();
    let file_a = repo.path().join("file_a.txt");
    let file_b = repo.path().join("file_b.txt");

    // Create both files and commit
    std::fs::write(&file_a, "line1\nline2\n").unwrap();
    std::fs::write(&file_b, "hello\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_a.txt"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "file_b.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial commit").unwrap();

    // Edit BOTH files and commit (so both end up in INITIAL after reset)
    std::fs::write(&file_a, "line1\nline2\nai on a\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_a.txt"])
        .unwrap();
    std::fs::write(&file_b, "hello\nai added\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_b.txt"])
        .unwrap();
    repo.stage_all_and_commit("second commit with AI on both files")
        .unwrap();

    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();
    repo.sync_daemon_force();

    // Strip file_blobs from INITIAL to simulate legacy data (pre-March-2026)
    let git_ai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let head_sha = git_ai_repo.head().unwrap().target().unwrap();
    let working_log = git_ai_repo
        .storage
        .working_log_for_base_commit(&head_sha)
        .unwrap();
    let initial = working_log.read_initial_attributions();
    assert!(
        initial.files.contains_key("file_a.txt"),
        "INITIAL must contain file_a.txt for this test"
    );
    assert!(
        initial.files.contains_key("file_b.txt"),
        "INITIAL must contain file_b.txt for this test"
    );

    let mut legacy_initial = initial.clone();
    legacy_initial.file_blobs.clear();
    let json = serde_json::to_string(&legacy_initial).unwrap();
    std::fs::write(&working_log.initial_file, &json).unwrap();

    // Directly invoke checkpoint daemon logic on file_b only. The no-blob INITIAL
    // state is invalid even though dirty_files contains file_b, because INITIAL
    // also references file_a and neither attribution can be resolved against the
    // exact snapshot it describes.
    let mut dirty_files = HashMap::new();
    dirty_files.insert(
        "file_b.txt".to_string(),
        Arc::from("hello\nai added\nnew line\n"),
    );

    let resolved = ResolvedCheckpointExecution {
        base_commit: head_sha.clone(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        files: vec!["file_b.txt".to_string()],
        dirty_files,
    };

    let checkpoint_request = CheckpointRequest {
        trace_id: "test-trace".to_string(),
        checkpoint_kind: CheckpointKind::AiAgent,
        agent_id: Some(AgentId {
            tool: "test".to_string(),
            id: "test-id".to_string(),
            model: "test-model".to_string(),
        }),
        files: vec![CheckpointFile {
            path: file_b.clone(),
            content: Some("hello\nai added\nnew line\n".to_string()),
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Sha(head_sha),
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
        delivery_id: None,
    };

    let result = execute_resolved_checkpoint_from_daemon(
        &git_ai_repo,
        "test",
        CheckpointKind::AiAgent,
        checkpoint_request,
        resolved,
    );
    let error = result.expect_err("checkpoint should reject INITIAL without persisted blobs");
    assert!(
        error
            .to_string()
            .contains("INITIAL missing persisted file snapshot"),
        "unexpected error: {error}"
    );
}
