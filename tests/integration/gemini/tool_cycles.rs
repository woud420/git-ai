use super::{ExpectedLineExt, TestRepo, fixture_path, fs, json};

// ============================================================================
// End-to-end tests using TestRepo
// ============================================================================

#[test]
fn test_gemini_e2e_with_attribution() {
    let repo = TestRepo::new();
    let fixture_path_str = fixture_path("gemini-session-simple.jsonl")
        .to_string_lossy()
        .to_string();

    let src_dir = repo.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = repo.path().join("src/index.ts");
    let base_content = "console.log('Bonjour');\n\nconsole.log('hello world');\n";
    fs::write(&file_path, base_content).unwrap();

    repo.stage_all_and_commit("Initial commit").unwrap();

    let edited_content =
        "console.log('Bonjour');\n\nconsole.log('hello world');\nconsole.log('hello bob');\n";
    fs::write(&file_path, edited_content).unwrap();

    let hook_input = json!({
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "cwd": repo.canonical_path().to_string_lossy().to_string(),
        "hook_event_name": "AfterTool",
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        },
        "transcript_path": fixture_path_str
    })
    .to_string();

    repo.checkpoint_with_hook_input("gemini", &hook_input)
        .unwrap();

    let commit = repo.stage_all_and_commit("Add gemini edits").unwrap();

    let mut file = repo.filename("src/index.ts");
    file.assert_lines_and_blame(crate::lines![
        "console.log('Bonjour');".human(),
        "".human(),
        "console.log('hello world');".human(),
        "console.log('hello bob');".ai(),
    ]);

    assert!(!commit.authorship_log.attestations.is_empty());
    assert!(!commit.authorship_log.metadata.sessions.is_empty());

    let session_record = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("Should have at least one session record");

    assert_eq!(session_record.agent_id.model, "gemini-2.5-flash");
}

#[test]
fn test_gemini_e2e_human_checkpoint() {
    let repo = TestRepo::new();
    let fixture_path_str = fixture_path("gemini-session-simple.jsonl")
        .to_string_lossy()
        .to_string();

    let src_dir = repo.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = repo.path().join("src/index.ts");
    fs::write(&file_path, "console.log('hello');\n").unwrap();

    repo.stage_all_and_commit("Initial commit").unwrap();

    let hook_input = json!({
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "cwd": repo.canonical_path().to_string_lossy().to_string(),
        "hook_event_name": "BeforeTool",
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        },
        "transcript_path": fixture_path_str
    })
    .to_string();

    repo.checkpoint_with_hook_input("gemini", &hook_input)
        .unwrap();

    fs::write(
        &file_path,
        "console.log('hello');\nconsole.log('human edit');\n",
    )
    .unwrap();

    let commit = repo.stage_all_and_commit("Human edit").unwrap();

    let mut file = repo.filename("src/index.ts");
    file.assert_lines_and_blame(crate::lines![
        "console.log('hello');".human(),
        "console.log('human edit');".human(),
    ]);

    assert_eq!(commit.authorship_log.attestations.len(), 0);
}

#[test]
fn test_issue_1951_gemini_ignores_internal_files() {
    let repo = TestRepo::new();
    let tracked_path = repo.path().join("tracked.txt");
    fs::write(&tracked_path, "tracked\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut tracked_file = repo.filename("tracked.txt");
    tracked_file.assert_committed_lines(crate::lines!["tracked".unattributed_human()]);

    let gemini_home = tempfile::tempdir().unwrap();
    let internal_path = gemini_home
        .path()
        .join(".gemini/tmp/project/memory/skills/example/SKILL.md");
    fs::create_dir_all(internal_path.parent().unwrap()).unwrap();
    fs::write(&internal_path, "internal memory\n").unwrap();

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let reported_internal_path = internal_path.to_string_lossy().to_uppercase();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let reported_internal_path = internal_path.to_string_lossy().to_string();

    for hook_event_name in ["BeforeTool", "AfterTool"] {
        let hook_input = json!({
            "session_id": "gemini-internal-file-session",
            "cwd": repo.canonical_path().to_string_lossy().to_string(),
            "hook_event_name": hook_event_name,
            "tool_name": "write_file",
            "tool_input": {
                "file_path": reported_internal_path
            },
            "transcript_path": fixture_path("gemini-session-simple.jsonl")
                .to_string_lossy()
                .to_string(),
        })
        .to_string();

        let output = repo
            .git_ai_with_env(
                &["checkpoint", "gemini", "--hook-input", &hook_input],
                &[("GEMINI_CLI_HOME", gemini_home.path().to_str().unwrap())],
            )
            .unwrap();
        assert!(
            output.is_empty(),
            "Gemini internal file hooks should be ignored silently, got: {output}"
        );
    }

    assert!(
        repo.current_working_logs()
            .all_ai_touched_files()
            .unwrap_or_default()
            .is_empty(),
        "Gemini internal files should not create repository checkpoints"
    );
}

#[test]
fn test_gemini_e2e_multiple_tool_calls() {
    let repo = TestRepo::new();
    let fixture_path_str = fixture_path("gemini-session-simple.jsonl")
        .to_string_lossy()
        .to_string();

    let file_path = repo.path().join("test.ts");
    fs::write(&file_path, "const x = 1;\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(&file_path, "const x = 1;\nconst y = 2;\nconst z = 3;\n").unwrap();

    let hook_input = json!({
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "cwd": repo.canonical_path().to_string_lossy().to_string(),
        "hook_event_name": "AfterTool",
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        },
        "transcript_path": fixture_path_str
    })
    .to_string();

    repo.checkpoint_with_hook_input("gemini", &hook_input)
        .unwrap();

    let commit = repo.stage_all_and_commit("Add multiple lines").unwrap();

    let mut file = repo.filename("test.ts");
    file.assert_lines_and_blame(crate::lines![
        "const x = 1;".human(),
        "const y = 2;".ai(),
        "const z = 3;".ai(),
    ]);

    assert!(!commit.authorship_log.attestations.is_empty());
}

#[test]
fn test_gemini_e2e_with_resync() {
    let repo = TestRepo::new();
    let fixture_path_str = fixture_path("gemini-session-simple.jsonl")
        .to_string_lossy()
        .to_string();

    let file_path = repo.path().join("test.ts");
    fs::write(&file_path, "const x = 1;\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(&file_path, "const x = 1;\nconst y = 2;\n").unwrap();

    let hook_input = json!({
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "cwd": repo.canonical_path().to_string_lossy().to_string(),
        "hook_event_name": "AfterTool",
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        },
        "transcript_path": fixture_path_str
    })
    .to_string();

    repo.checkpoint_with_hook_input("gemini", &hook_input)
        .unwrap();

    let commit = repo.stage_all_and_commit("Add gemini edits").unwrap();

    let mut file = repo.filename("test.ts");
    file.assert_lines_and_blame(crate::lines!["const x = 1;".human(), "const y = 2;".ai(),]);

    assert!(!commit.authorship_log.metadata.sessions.is_empty());

    let _session_record = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("Should have at least one session record");
}

#[test]
fn test_gemini_e2e_partial_staging() {
    let repo = TestRepo::new();
    let fixture_path_str = fixture_path("gemini-session-simple.jsonl")
        .to_string_lossy()
        .to_string();

    let file_path = repo.path().join("test.ts");
    fs::write(&file_path, "line1\nline2\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(&file_path, "line1\nline2\nai_line3\nai_line4\n").unwrap();

    repo.git(&["add", "test.ts"]).unwrap();

    fs::write(&file_path, "line1\nline2\nai_line3\nai_line4\nai_line5\n").unwrap();

    let hook_input = json!({
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "cwd": repo.canonical_path().to_string_lossy().to_string(),
        "hook_event_name": "AfterTool",
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        },
        "transcript_path": fixture_path_str
    })
    .to_string();

    repo.checkpoint_with_hook_input("gemini", &hook_input)
        .unwrap();

    let commit = repo.commit("Partial staging").unwrap();

    assert!(!commit.authorship_log.attestations.is_empty());

    let mut file = repo.filename("test.ts");
    file.assert_committed_lines(crate::lines![
        "line1".human(),
        "line2".human(),
        "ai_line3".ai(),
        "ai_line4".ai(),
    ]);
}

#[test]
fn test_gemini_preset_bash_tool_aftertool_detects_changes() {
    let repo = TestRepo::new();
    let fixture_path_str = fixture_path("gemini-session-simple.jsonl")
        .to_string_lossy()
        .to_string();
    let cwd = repo.canonical_path().to_string_lossy().to_string();
    let session_id = "gemini-bash-test-session";
    let tool_use_id = "tool-call-001";

    let file_path = repo.path().join("script.sh");
    fs::write(&file_path, "#!/bin/sh\necho hello\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let pre_hook_input = json!({
        "session_id": session_id,
        "tool_use_id": tool_use_id,
        "cwd": cwd,
        "hook_event_name": "BeforeTool",
        "tool_name": "shell",
        "tool_input": { "command": "echo modified > output.txt" },
        "transcript_path": fixture_path_str,
    })
    .to_string();
    repo.checkpoint_with_hook_input("gemini", &pre_hook_input)
        .unwrap();

    let output_path = repo.path().join("output.txt");
    fs::write(&output_path, "modified\n").unwrap();

    let post_hook_input = json!({
        "session_id": session_id,
        "tool_use_id": tool_use_id,
        "cwd": cwd,
        "hook_event_name": "AfterTool",
        "tool_name": "shell",
        "tool_input": { "command": "echo modified > output.txt" },
        "transcript_path": fixture_path_str,
    })
    .to_string();
    repo.checkpoint_with_hook_input("gemini", &post_hook_input)
        .unwrap();

    let commit = repo.stage_all_and_commit("Gemini bash edit").unwrap();
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "AfterTool with shell should produce AI attestations"
    );
}

crate::reuse_tests_in_worktree!(
    test_gemini_e2e_with_attribution,
    test_gemini_e2e_human_checkpoint,
    test_issue_1951_gemini_ignores_internal_files,
    test_gemini_e2e_multiple_tool_calls,
    test_gemini_e2e_with_resync,
    test_gemini_e2e_partial_staging,
    test_gemini_preset_bash_tool_aftertool_detects_changes,
);
