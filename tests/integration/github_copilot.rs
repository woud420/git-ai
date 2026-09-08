use crate::test_utils::{fixture_path, load_fixture, read_jsonl_fixture};
use git_ai::error::GitAiError;
use git_ai::model::stream_watermark::{ByteOffsetWatermark, RecordIndexWatermark};
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use git_ai::operations::streams::agents::CopilotAgent;
use serde_json::json;
use std::fs;

fn parse_copilot(hook_input: &str) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    resolve_preset("github-copilot")?.parse(hook_input, "t_test")
}

/// Ensure CODESPACES and REMOTE_CONTAINERS are not set (they cause early return in transcript parsing)
fn ensure_clean_env() {
    unsafe {
        std::env::remove_var("CODESPACES");
        std::env::remove_var("REMOTE_CONTAINERS");
    }
}

// ============================================================================
// VS Code model lookup tests
// ============================================================================

const VS_CODE_LOOKUP_SESSION_ID: &str = "fixture-session-id";

fn setup_vscode_model_lookup_workspace(chat_session_fixture: &str) -> (tempfile::TempDir, String) {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_storage = temp_dir
        .path()
        .join("workspaceStorage")
        .join("workspace-model");
    let transcripts_dir = workspace_storage
        .join("GitHub.copilot-chat")
        .join("transcripts");
    let chat_sessions_dir = workspace_storage.join("chatSessions");
    fs::create_dir_all(&transcripts_dir).unwrap();
    fs::create_dir_all(&chat_sessions_dir).unwrap();

    let transcript_path = transcripts_dir.join(format!("{}.jsonl", VS_CODE_LOOKUP_SESSION_ID));
    fs::write(
        &transcript_path,
        load_fixture("copilot_transcript_session_lookup.jsonl"),
    )
    .unwrap();

    let fixture_p = fixture_path(chat_session_fixture);
    let ext = fixture_p
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("jsonl");
    let chat_session_path = chat_sessions_dir.join(format!("session-lookup.{}", ext));
    fs::write(chat_session_path, load_fixture(chat_session_fixture)).unwrap();

    (temp_dir, transcript_path.to_string_lossy().to_string())
}

fn vscode_post_tool_use_hook_input(transcript_path: &str) -> String {
    json!({
        "hookEventName": "PostToolUse",
        "cwd": "/Users/test/project",
        "toolName": "copilot_replaceString",
        "toolInput": { "file_path": "/Users/test/project/src/main.ts" },
        "sessionId": VS_CODE_LOOKUP_SESSION_ID,
        "transcript_path": transcript_path
    })
    .to_string()
}

mod hook_parsing;
mod vscode_hooks;
