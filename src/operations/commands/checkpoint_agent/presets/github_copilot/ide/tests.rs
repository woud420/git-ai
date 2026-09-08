use super::super::super::AgentPreset;
use super::super::GithubCopilotPreset;
use super::*;
use serde_json::json;

// -----------------------------------------------------------------------
// Legacy extension path tests
// -----------------------------------------------------------------------

#[test]
fn test_copilot_legacy_before_edit() {
    let input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/home/user/project",
        "will_edit_filepaths": ["/home/user/project/src/main.rs"],
        "chat_session_id": "sess-123",
        "dirty_files": {"/home/user/project/src/main.rs": "old content"}
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(e.context.external_session_id, "sess-123");
            assert_eq!(e.context.cwd, PathBuf::from("/home/user/project"));
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/main.rs")]
            );
            assert!(e.dirty_files.is_some());
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_dirty_files_camel_case() {
    let input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/home/user/project",
        "will_edit_filepaths": ["/home/user/project/src/main.rs"],
        "chat_session_id": "sess-123",
        "dirtyFiles": {"/home/user/project/src/main.rs": "content"}
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert!(e.dirty_files.is_some());
            let df = e.dirty_files.as_ref().unwrap();
            assert!(df.contains_key(&PathBuf::from("/home/user/project/src/main.rs")));
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_legacy_after_edit() {
    let input = json!({
        "hook_event_name": "after_edit",
        "workspace_folder": "/home/user/project",
        "chat_session_path": "/home/user/.vscode/sessions/sess-123.json",
        "session_id": "sess-123",
        "edited_filepaths": ["src/main.rs"]
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(e.context.external_session_id, "sess-123");
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/main.rs")]
            );
            assert!(matches!(
                e.stream_source,
                Some(StreamSource {
                    format: StreamFormat::CopilotSessionJson,
                    ..
                })
            ));
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_copilot_legacy_before_edit_empty_filepaths() {
    let input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/home/user/project",
        "will_edit_filepaths": [],
        "chat_session_id": "sess-123"
    })
    .to_string();
    let result = GithubCopilotPreset.parse(&input, "t_test123456789a");
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// VS Code native path tests
// -----------------------------------------------------------------------

#[test]
fn test_copilot_native_pre_file_edit() {
    let input = json!({
        "hook_event_name": "PreToolUse",
        "cwd": "/home/user/project",
        "tool_name": "replace_string_in_file",
        "session_id": "sess-456",
        "tool_use_id": "tu-1",
        "tool_input": {"file_path": "/home/user/project/src/main.rs"},
        "transcript_path": "/home/user/.vscode/data/github.copilot-chat/transcripts/sess-456.json"
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(e.context.external_session_id, "sess-456");
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/main.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_native_post_file_edit() {
    let input = json!({
        "hook_event_name": "PostToolUse",
        "cwd": "/home/user/project",
        "tool_name": "create_file",
        "session_id": "sess-456",
        "tool_use_id": "tu-2",
        "tool_input": {"file_path": "/home/user/project/src/new.rs"},
        "transcript_path": "/home/user/.vscode/data/github.copilot-chat/transcripts/sess-456.json"
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/new.rs")]
            );
            assert!(matches!(
                e.stream_source,
                Some(StreamSource {
                    format: StreamFormat::CopilotSessionJson,
                    ..
                })
            ));
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_copilot_native_model_prefers_otel_selected_model_over_models_json_default() {
    let dir = tempfile::tempdir().unwrap();
    let user_dir = dir.path().join("User");
    let transcript_path = user_dir
        .join("workspaceStorage")
        .join("workspace-1")
        .join("GitHub.copilot-chat")
        .join("transcripts")
        .join("session-abc.jsonl");
    std::fs::create_dir_all(transcript_path.parent().unwrap()).unwrap();
    std::fs::write(
        &transcript_path,
        r#"{"type":"session.start","data":{"sessionId":"session-abc"}}"#,
    )
    .unwrap();

    let models_path = user_dir
        .join("workspaceStorage")
        .join("workspace-1")
        .join("GitHub.copilot-chat")
        .join("debug-logs")
        .join("session-abc")
        .join("models.json");
    std::fs::create_dir_all(models_path.parent().unwrap()).unwrap();
    std::fs::write(&models_path, r#"[{"id":"gpt-4.1","is_chat_default":true}]"#).unwrap();

    let otel_db_path = user_dir
        .join("globalStorage")
        .join("github.copilot-chat")
        .join("agent-traces.db");
    std::fs::create_dir_all(otel_db_path.parent().unwrap()).unwrap();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&otel_db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE spans (
                span_id TEXT PRIMARY KEY,
                chat_session_id TEXT,
                request_model TEXT,
                response_model TEXT,
                end_time_ms REAL NOT NULL
            );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO spans (span_id, chat_session_id, request_model, response_model, end_time_ms)
             VALUES ('span-1', 'session-abc', 'claude-sonnet-4', 'claude-sonnet-4-20250514', 1000)",
        [],
    )
    .unwrap();
    drop(conn);

    let input = json!({
        "hook_event_name": "PostToolUse",
        "cwd": "/home/user/project",
        "tool_name": "create_file",
        "session_id": "session-abc",
        "tool_use_id": "tu-2",
        "tool_input": {"file_path": "/home/user/project/src/new.rs"},
        "transcript_path": transcript_path
    })
    .to_string();
    let mut events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(
        events[0].preset_context_mut().unwrap().agent_id.model,
        "unknown"
    );
    GithubCopilotPreset
        .enrich_authorized_events(&input, &mut events)
        .unwrap();

    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.model, "claude-sonnet-4");
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_copilot_native_pre_bash_call() {
    let input = json!({
        "hook_event_name": "PreToolUse",
        "cwd": "/home/user/project",
        "tool_name": "run_in_terminal",
        "session_id": "sess-456",
        "tool_use_id": "tu-3",
        "transcript_path": "/home/user/.vscode/data/github.copilot-chat/transcripts/sess-456.json"
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(e.tool_use_id, "tu-3");
        }
        _ => panic!("Expected PreBashCall"),
    }
}

#[test]
fn test_copilot_native_post_bash_call() {
    let input = json!({
        "hook_event_name": "PostToolUse",
        "cwd": "/home/user/project",
        "tool_name": "run_in_terminal",
        "session_id": "sess-456",
        "tool_use_id": "tu-3",
        "transcript_path": "/home/user/.vscode/data/github.copilot-chat/transcripts/sess-456.json"
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(e.tool_use_id, "tu-3");
        }
        _ => panic!("Expected PostBashCall"),
    }
}

#[test]
fn test_copilot_native_create_file_pre_empty_dirty() {
    let input = json!({
        "hook_event_name": "PreToolUse",
        "cwd": "/home/user/project",
        "tool_name": "create_file",
        "session_id": "sess-456",
        "tool_input": {"file_path": "/home/user/project/src/new_file.rs"},
        "transcript_path": "/home/user/.vscode/data/github.copilot-chat/transcripts/sess-456.json"
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/new_file.rs")]
            );
            assert_eq!(
                e.dirty_files
                    .as_ref()
                    .unwrap()
                    .get(&PathBuf::from("/home/user/project/src/new_file.rs")),
                Some(&String::new())
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_skips_non_edit_tools() {
    let input = json!({
        "hook_event_name": "PreToolUse",
        "cwd": "/home/user/project",
        "tool_name": "search_files",
        "session_id": "sess-456",
        "transcript_path": "/home/user/.vscode/data/github.copilot-chat/transcripts/sess-456.json"
    })
    .to_string();
    let result = GithubCopilotPreset.parse(&input, "t_test123456789a");
    assert!(result.is_err());
}

#[test]
fn test_copilot_skips_claude_transcript() {
    let input = json!({
        "hook_event_name": "PreToolUse",
        "cwd": "/home/user/project",
        "tool_name": "create_file",
        "session_id": "sess-456",
        "tool_input": {"file_path": "src/main.rs"},
        "transcript_path": "/home/user/.claude/projects/test.json"
    })
    .to_string();
    let result = GithubCopilotPreset.parse(&input, "t_test123456789a");
    assert!(result.is_err());
}

#[test]
fn test_copilot_session_id_fallback() {
    let input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/home/user/project",
        "will_edit_filepaths": ["/home/user/project/src/main.rs"],
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.external_session_id, "unknown");
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

// -----------------------------------------------------------------------
// Helper function tests
// -----------------------------------------------------------------------

#[test]
fn test_classify_copilot_tool_bash() {
    assert_eq!(classify_copilot_tool("run_in_terminal"), ToolClass::Bash);
}

#[test]
fn test_classify_copilot_tool_file_edit() {
    assert_eq!(classify_copilot_tool("create_file"), ToolClass::FileEdit);
    assert_eq!(
        classify_copilot_tool("replace_string_in_file"),
        ToolClass::FileEdit
    );
    assert_eq!(classify_copilot_tool("apply_patch"), ToolClass::FileEdit);
    assert_eq!(classify_copilot_tool("delete_file"), ToolClass::FileEdit);
}

#[test]
fn test_classify_copilot_tool_heuristic() {
    assert_eq!(
        classify_copilot_tool("custom_edit_tool"),
        ToolClass::FileEdit
    );
    assert_eq!(classify_copilot_tool("write_changes"), ToolClass::FileEdit);
}

#[test]
fn test_classify_copilot_tool_skip() {
    assert_eq!(classify_copilot_tool("search_files"), ToolClass::Skip);
    assert_eq!(classify_copilot_tool("unknown_tool"), ToolClass::Skip);
}

#[test]
fn test_collect_apply_patch_paths() {
    let text = "*** Update File: /home/user/src/main.rs\n--- some diff ---\n*** Add File: /home/user/src/new.rs\n";
    let mut paths = Vec::new();
    parse::collect_apply_patch_paths_from_text(text, &mut paths);
    assert_eq!(
        paths,
        vec!["/home/user/src/main.rs", "/home/user/src/new.rs"]
    );
}

#[test]
fn test_looks_like_copilot_transcript_path() {
    assert!(looks_like_copilot_transcript_path(
        "/home/user/.vscode/data/github.copilot-chat/transcripts/test.json"
    ));
    assert!(looks_like_copilot_transcript_path(
        "/path/to/vscode-chat-session-123.json"
    ));
    assert!(!looks_like_copilot_transcript_path(
        "/home/user/.claude/projects/test.json"
    ));
}

#[test]
fn test_is_supported_vscode_edit_tool_name() {
    assert!(is_supported_vscode_edit_tool_name("create_file"));
    assert!(is_supported_vscode_edit_tool_name("run_in_terminal"));
    assert!(is_supported_vscode_edit_tool_name("replace_string_in_file"));
    assert!(is_supported_vscode_edit_tool_name("custom_edit_tool"));
    assert!(!is_supported_vscode_edit_tool_name("search_files"));
    assert!(!is_supported_vscode_edit_tool_name("read_file"));
}

#[test]
fn test_copilot_camel_case_keys() {
    let input = json!({
        "hookEventName": "before_edit",
        "workspaceFolder": "/home/user/project",
        "will_edit_filepaths": ["/home/user/project/src/main.rs"],
        "chatSessionId": "sess-789"
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.external_session_id, "sess-789");
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_default_after_edit_when_no_hook_event_name() {
    // When hook_event_name is missing, defaults to "after_edit"
    let input = json!({
        "workspace_folder": "/home/user/project",
        "chat_session_path": "/home/user/.vscode/sessions/sess-123.json",
        "session_id": "sess-123",
        "edited_filepaths": ["src/main.rs"]
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ParsedHookEvent::PostFileEdit(_)));
}

#[test]
fn test_copilot_native_workspace_storage_format() {
    let input = json!({
        "hook_event_name": "PostToolUse",
        "cwd": "/home/user/project",
        "tool_name": "create_file",
        "session_id": "sess-456",
        "tool_input": {"file_path": "/home/user/project/src/new.rs"},
        "transcript_path": "/home/user/.vscode/data/workspaceStorage/abc/chatSessions/sess-456.json"
    })
    .to_string();
    let events = GithubCopilotPreset
        .parse(&input, "t_test123456789a")
        .unwrap();
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(matches!(
                e.stream_source,
                Some(StreamSource {
                    format: StreamFormat::CopilotEventStreamJsonl,
                    ..
                })
            ));
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_vscode_apply_patch_real_payload() {
    let pre_input = json!({
        "hook_event_name": "PreToolUse",
        "session_id": "bad0027f-a716-4b05-82dc-c186eb655967",
        "transcript_path": "/Users/svarlamov/Library/Application Support/Code/User/workspaceStorage/e89dd309cf385022c02e2f1c9e8c403f/GitHub.copilot-chat/transcripts/bad0027f-a716-4b05-82dc-c186eb655967.jsonl",
        "tool_name": "apply_patch",
        "tool_input": {
            "explanation": "Change the warning message from 'oops' to 'oopsies'",
            "input": "*** Begin Patch\n*** Update File: /Users/svarlamov/testing-git-ai-sessions-v2-apr-20/testing-git-1/jokes-cli.ts\n@@ rl.question(\"Which joke do you want to hear (1-3)? (Press Enter for a random joke) \", (answer) => {\n-      console.warn(\"oops\");\n+      console.warn(\"oopsies\");\n*** End Patch"
        },
        "tool_use_id": "call_lEov1CG9mTy45oPQYT0VST80__vscode-1778541016875",
        "cwd": "/Users/svarlamov/testing-git-ai-sessions-v2-apr-20/testing-git-1"
    })
    .to_string();

    let events = GithubCopilotPreset
        .parse(&pre_input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from(
                    "/Users/svarlamov/testing-git-ai-sessions-v2-apr-20/testing-git-1/jokes-cli.ts"
                )]
            );
            assert_eq!(
                e.tool_use_id.as_deref(),
                Some("call_lEov1CG9mTy45oPQYT0VST80__vscode-1778541016875")
            );
        }
        other => panic!("Expected PreFileEdit, got {:?}", other),
    }

    let post_input = json!({
        "hook_event_name": "PostToolUse",
        "session_id": "bad0027f-a716-4b05-82dc-c186eb655967",
        "transcript_path": "/Users/svarlamov/Library/Application Support/Code/User/workspaceStorage/e89dd309cf385022c02e2f1c9e8c403f/GitHub.copilot-chat/transcripts/bad0027f-a716-4b05-82dc-c186eb655967.jsonl",
        "tool_name": "apply_patch",
        "tool_input": {
            "explanation": "Change the warning message from 'oops' to 'oopsies'",
            "input": "*** Begin Patch\n*** Update File: /Users/svarlamov/testing-git-ai-sessions-v2-apr-20/testing-git-1/jokes-cli.ts\n@@ rl.question(\"Which joke do you want to hear (1-3)? (Press Enter for a random joke) \", (answer) => {\n-      console.warn(\"oops\");\n+      console.warn(\"oopsies\");\n*** End Patch"
        },
        "tool_response": "",
        "tool_use_id": "call_lEov1CG9mTy45oPQYT0VST80__vscode-1778541016875",
        "cwd": "/Users/svarlamov/testing-git-ai-sessions-v2-apr-20/testing-git-1"
    })
    .to_string();

    let events = GithubCopilotPreset
        .parse(&post_input, "t_test123456789a")
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from(
                    "/Users/svarlamov/testing-git-ai-sessions-v2-apr-20/testing-git-1/jokes-cli.ts"
                )]
            );
            assert!(e.stream_source.is_some());
        }
        other => panic!("Expected PostFileEdit, got {:?}", other),
    }
}
