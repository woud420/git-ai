use super::{ParsedHookEvent, fixture_path, json, parse_codex};

#[test]
fn test_codex_preset_bash_pre_tool_use_skips_checkpoint_after_capturing_snapshot() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let hook_input = json!({
        "session_id": "session-bash-pre",
        "cwd": "/tmp/test-project",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-1",
        "tool_input": {
            "command": "git status --short"
        },
        "transcript_path": fixture.to_str().unwrap()
    })
    .to_string();

    // In the new parse API, bash PreToolUse returns PreBashCall with SnapshotOnly strategy
    // instead of returning an error. The caller handles the side effects.
    let events = parse_codex(&hook_input).expect("should succeed with PreBashCall");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "codex");
            assert_eq!(e.context.external_session_id, "session-bash-pre");
            assert_eq!(e.tool_use_id, "bash-use-1");
            assert!(
                e.context.metadata.contains_key("transcript_path"),
                "metadata should preserve transcript path for commit-time recovery"
            );
        }
        _ => panic!("Expected PreBashCall for bash PreToolUse"),
    }
}

#[test]
fn test_codex_preset_bash_pre_tool_use_supports_camel_case_hook_event_name() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let hook_input = json!({
        "session_id": "session-bash-pre-camel",
        "cwd": "/tmp/test-project",
        "hookEventName": "PreToolUse",
        "toolName": "Bash",
        "toolUseId": "bash-use-camel-1",
        "tool_input": {
            "command": "git status --short"
        },
        "transcript_path": fixture.to_str().unwrap()
    })
    .to_string();

    // Camel-case fields should work the same as snake_case
    let events = parse_codex(&hook_input).expect("should succeed with PreBashCall");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "codex");
            assert_eq!(e.context.external_session_id, "session-bash-pre-camel");
            assert_eq!(e.tool_use_id, "bash-use-camel-1");
        }
        _ => panic!("Expected PreBashCall for camel-case PreToolUse"),
    }
}

#[test]
fn test_codex_preset_bash_post_tool_use_detects_changed_files() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let post_hook_input = json!({
        "session_id": "session-bash-post",
        "cwd": "/tmp/test-project",
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-2",
        "tool_input": {
            "command": "perl -0pi -e 's/fn main\\(\\) \\{\\}/fn main\\(\\) { println!(\"hello\"); }/' src/main.rs"
        },
        "transcript_path": fixture.to_str().unwrap()
    })
    .to_string();

    let events = parse_codex(&post_hook_input).expect("Codex preset post-hook should run");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostBashCall(e) => {
            assert!(e.stream_source.is_some());
            assert_eq!(e.context.agent_id.tool, "codex");
            assert_eq!(e.context.external_session_id, "session-bash-post");
            assert_eq!(e.tool_use_id, "bash-use-2");
        }
        _ => panic!("Expected PostBashCall"),
    }
}

crate::reuse_tests_in_worktree!(
    test_codex_preset_bash_pre_tool_use_skips_checkpoint_after_capturing_snapshot,
    test_codex_preset_bash_pre_tool_use_supports_camel_case_hook_event_name,
    test_codex_preset_bash_post_tool_use_detects_changed_files,
);
