use super::{
    ClaudeAgent, Path, PathBuf, TempDir, TestRepo, fs, json, make_stream_record,
    resolve_repo_url_from_path, setup_test_db, write_claude_transcript_cwd_on_later_line,
    write_claude_transcript_with_cwd, write_claude_transcript_without_cwd,
};
use git_ai::operations::streams::agent::Agent;

// === Test Group 2: Claude infer_cwd ===

#[test]
fn test_claude_infer_cwd_from_user_event() {
    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_with_cwd(&transcript, "/Users/dev/my-project");
    let agent = ClaudeAgent::new();
    let result = agent.infer_cwd(&transcript);
    assert_eq!(
        result,
        Some(PathBuf::from("/Users/dev/my-project")),
        "Must extract cwd from Claude transcript user event"
    );
}

#[test]
fn test_claude_infer_cwd_no_cwd_in_transcript() {
    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_without_cwd(&transcript);
    let agent = ClaudeAgent::new();
    let result = agent.infer_cwd(&transcript);
    assert_eq!(
        result, None,
        "Must return None when transcript has no cwd field"
    );
}

#[test]
fn test_claude_infer_cwd_empty_file() {
    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    fs::write(&transcript, "").unwrap();
    let agent = ClaudeAgent::new();
    let result = agent.infer_cwd(&transcript);
    assert_eq!(result, None, "Must return None for empty transcript file");
}

#[test]
fn test_claude_infer_cwd_missing_file() {
    let agent = ClaudeAgent::new();
    let result = agent.infer_cwd(Path::new("/nonexistent/path/session.jsonl"));
    assert_eq!(
        result, None,
        "Must return None for non-existent file without panicking"
    );
}

#[test]
fn test_claude_infer_cwd_not_on_first_line() {
    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_cwd_on_later_line(&transcript, "/home/user/repo");
    let agent = ClaudeAgent::new();
    let result = agent.infer_cwd(&transcript);
    assert_eq!(
        result,
        Some(PathBuf::from("/home/user/repo")),
        "Must find cwd even when first event lacks it"
    );
}

// === Test Group 3: DB schema and repo_work_dir persistence ===

#[test]
fn test_db_new_schema_has_repo_work_dir() {
    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let transcript = temp_dir.path().join("t.jsonl");
    fs::write(&transcript, "").unwrap();

    let record = make_stream_record("test-1", "claude", &transcript, Some("/Users/dev/project"));
    db.insert_stream(&record).unwrap();
    let retrieved = db
        .get_stream("test-1", "transcript", &transcript.display().to_string())
        .unwrap()
        .unwrap();
    assert_eq!(
        retrieved.repo_work_dir,
        Some("/Users/dev/project".to_string()),
        "repo_work_dir must round-trip through insert/get"
    );
}

#[test]
fn test_db_insert_session_without_repo_work_dir() {
    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let transcript = temp_dir.path().join("t.jsonl");
    fs::write(&transcript, "").unwrap();

    let record = make_stream_record("test-2", "claude", &transcript, None);
    db.insert_stream(&record).unwrap();
    let retrieved = db
        .get_stream("test-2", "transcript", &transcript.display().to_string())
        .unwrap()
        .unwrap();
    assert_eq!(
        retrieved.repo_work_dir, None,
        "repo_work_dir must be None when not provided"
    );
}

#[test]
fn test_db_update_repo_work_dir() {
    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let transcript = temp_dir.path().join("t.jsonl");
    fs::write(&transcript, "").unwrap();

    let record = make_stream_record("test-3", "claude", &transcript, None);
    db.insert_stream(&record).unwrap();

    db.update_repo_work_dir(
        "test-3",
        "transcript",
        &transcript.display().to_string(),
        "/Users/dev/my-project",
    )
    .unwrap();
    let retrieved = db
        .get_stream("test-3", "transcript", &transcript.display().to_string())
        .unwrap()
        .unwrap();
    assert_eq!(
        retrieved.repo_work_dir,
        Some("/Users/dev/my-project".to_string()),
        "update_repo_work_dir must set the column"
    );
}

#[test]
fn test_inferred_cwd_persisted_to_db() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:a/b.git"])
        .unwrap();

    let transcript = repo.path().join("session.jsonl");
    write_claude_transcript_with_cwd(&transcript, repo.path().to_str().unwrap());

    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record("test-persist", "claude", &transcript, None);
    db.insert_stream(&record).unwrap();

    let agent = ClaudeAgent::new();
    let inferred = agent.infer_cwd(&transcript).unwrap();
    db.update_repo_work_dir(
        "test-persist",
        "transcript",
        &transcript.display().to_string(),
        &inferred.display().to_string(),
    )
    .unwrap();

    let session = db
        .get_stream(
            "test-persist",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        session.repo_work_dir,
        Some(repo.path().display().to_string()),
        "Inferred cwd must be persisted to DB for future processing cycles"
    );
}

// === Test Group 6: Priority Resolution (hook > DB > infer) ===

#[test]
fn test_repo_work_dir_priority_hook_wins_over_db() {
    let repo_a = TestRepo::new();
    repo_a
        .git(&["remote", "add", "origin", "git@github.com:org/old.git"])
        .unwrap();
    let repo_b = TestRepo::new();
    repo_b
        .git(&["remote", "add", "origin", "git@github.com:org/new.git"])
        .unwrap();

    let transcript = repo_b.path().join("session.jsonl");
    write_claude_transcript_with_cwd(&transcript, repo_b.path().to_str().unwrap());

    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record(
        "test-priority",
        "claude",
        &transcript,
        Some(repo_a.path().to_str().unwrap()),
    );
    db.insert_stream(&record).unwrap();

    // Hook provides repo_b's path (should take priority)
    let task_repo_work_dir = Some(repo_b.path().to_path_buf());
    let session = db
        .get_stream(
            "test-priority",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    let db_repo_work_dir = session.repo_work_dir.as_ref().map(PathBuf::from);

    // Resolution order: task > db > infer
    let resolved_work_dir = task_repo_work_dir.or(db_repo_work_dir);
    let resolved_url = resolved_work_dir
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));

    assert_eq!(
        resolved_url,
        Some("https://github.com/org/new".to_string()),
        "Hook-provided repo_work_dir MUST take priority over DB-stored value"
    );
}

#[test]
fn test_repo_work_dir_priority_db_used_when_no_hook() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:org/stored.git"])
        .unwrap();

    let transcript = repo.path().join("session.jsonl");
    write_claude_transcript_without_cwd(&transcript);

    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record(
        "test-db-prio",
        "claude",
        &transcript,
        Some(repo.path().to_str().unwrap()),
    );
    db.insert_stream(&record).unwrap();

    let task_repo_work_dir: Option<PathBuf> = None;
    let session = db
        .get_stream(
            "test-db-prio",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    let db_repo_work_dir = session.repo_work_dir.as_ref().map(PathBuf::from);

    let resolved_work_dir = task_repo_work_dir.or(db_repo_work_dir);
    let resolved_url = resolved_work_dir
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));

    assert_eq!(
        resolved_url,
        Some("https://github.com/org/stored".to_string()),
        "DB-stored repo_work_dir must be used when no hook value present"
    );
}

#[test]
fn test_repo_work_dir_priority_infer_fallback() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:org/inferred.git"])
        .unwrap();

    let transcript = repo.path().join("session.jsonl");
    write_claude_transcript_with_cwd(&transcript, repo.path().to_str().unwrap());

    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record("test-infer-prio", "claude", &transcript, None);
    db.insert_stream(&record).unwrap();

    let task_repo_work_dir: Option<PathBuf> = None;
    let session = db
        .get_stream(
            "test-infer-prio",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    let db_repo_work_dir = session.repo_work_dir.as_ref().map(PathBuf::from);

    let agent = ClaudeAgent::new();
    let inferred_cwd = agent.infer_cwd(&PathBuf::from(&session.stream_path));

    let resolved_work_dir = task_repo_work_dir.or(db_repo_work_dir).or(inferred_cwd);
    let resolved_url = resolved_work_dir
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));

    assert_eq!(
        resolved_url,
        Some("https://github.com/org/inferred".to_string()),
        "infer_cwd must be used as fallback when hook and DB are both None"
    );
}

// === Test Group 7: Agents without cwd inference ===

#[test]
fn test_cursor_infer_cwd_returns_none() {
    use git_ai::operations::streams::agents::CursorAgent;

    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    fs::write(&transcript, r#"{"type":"user","message":{"content":"hi"}}"#).unwrap();
    let agent = CursorAgent::new();
    assert_eq!(
        agent.infer_cwd(&transcript),
        None,
        "CursorAgent must return None for infer_cwd (no cwd in format)"
    );
}

#[test]
fn test_copilot_infer_cwd_returns_none() {
    use git_ai::operations::streams::agents::CopilotAgent;

    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.json");
    fs::write(&transcript, r#"{"messages":[]}"#).unwrap();
    let agent = CopilotAgent::new();
    assert_eq!(
        agent.infer_cwd(&transcript),
        None,
        "CopilotAgent must return None for infer_cwd"
    );
}

// === Test Group 9: Codex infer_cwd ===

#[test]
fn test_codex_infer_cwd_from_session_meta() {
    use git_ai::operations::streams::agents::CodexAgent;

    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    let events = [
        json!({"timestamp":"2026-02-11T05:53:33.335Z","type":"session_meta",
               "payload":{"id":"019c4b43","timestamp":"2026-02-11T05:53:33.266Z",
                          "cwd":"/Users/test/projects/my-app","originator":"Codex Desktop",
                          "cli_version":"0.99.0","source":"vscode","model_provider":"openai"}}),
        json!({"timestamp":"2026-02-11T05:53:33.340Z","type":"turn_context",
               "payload":{"turn_id":"turn-1","cwd":"/Users/test/projects/my-app",
                          "model":"gpt-5.1-codex"}}),
    ];
    let content: String = events.iter().map(|e| format!("{}\n", e)).collect();
    fs::write(&transcript, content).unwrap();

    let agent = CodexAgent::new();
    let result = agent.infer_cwd(&transcript);
    assert_eq!(
        result,
        Some(PathBuf::from("/Users/test/projects/my-app")),
        "CodexAgent must extract cwd from session_meta payload"
    );
}

#[test]
fn test_codex_infer_cwd_no_cwd() {
    use git_ai::operations::streams::agents::CodexAgent;

    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    let events = [
        json!({"timestamp":"2026-02-11T05:53:33.335Z","type":"message",
                             "payload":{"role":"user","content":"hello"}}),
    ];
    let content: String = events.iter().map(|e| format!("{}\n", e)).collect();
    fs::write(&transcript, content).unwrap();

    let agent = CodexAgent::new();
    let result = agent.infer_cwd(&transcript);
    assert_eq!(
        result, None,
        "CodexAgent must return None when no cwd in payload"
    );
}
