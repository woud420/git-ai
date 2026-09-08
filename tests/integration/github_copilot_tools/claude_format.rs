use super::{ExpectedLineExt, TestRepo, json};

/// Test Copilot CLI Claude-format `Edit` tool updating an existing file.
/// Copilot CLI >= 1.0.62 emits Claude-style PascalCase tool names (`Edit` instead of
/// `edit`/`str_replace`) and the edit `tool_input` is an apply-patch STRING rather than
/// an object with a `path` field. Both must still attribute to AI.
#[test]
fn test_copilot_cli_claude_format_edit_update_attribution() {
    let repo = TestRepo::new();

    let file_path = repo.path().join("jokes.csv");
    std::fs::write(
        &file_path,
        "id,setup,punchline\n1,Why do programmers prefer dark mode?,Because light attracts bugs.\n",
    )
    .unwrap();
    repo.git(&["add", "jokes.csv"]).unwrap();
    repo.git(&["commit", "-m", "Initial jokes"]).unwrap();
    repo.sync_daemon();

    let mut file = repo.filename("jokes.csv");
    file.assert_committed_lines(crate::lines![
        "id,setup,punchline".unattributed_human(),
        "1,Why do programmers prefer dark mode?,Because light attracts bugs.".unattributed_human(),
    ]);

    let session_id = "a5f5793e-0cae-4cfd-9414-b35bce1c127f";
    // apply-patch string referencing the file via its repo-relative path.
    let patch = "*** Begin Patch\n*** Update File: jokes.csv\n@@\n 1,Why do programmers prefer dark mode?,Because light attracts bugs.\n+2,Why did the developer go broke?,Because he used up all his cache.\n*** End Patch\n";

    let pre_hook_input = json!({
        "hook_event_name": "PreToolUse",
        "session_id": session_id,
        "timestamp": "2026-06-22T20:10:33.166Z",
        "cwd": repo.path().to_str().unwrap(),
        "tool_name": "Edit",
        "tool_input": patch,
    });
    repo.checkpoint_with_hook_input("github-copilot", &pre_hook_input.to_string())
        .unwrap();

    // AI writes the edit to disk before PostToolUse.
    std::fs::write(
        &file_path,
        "id,setup,punchline\n1,Why do programmers prefer dark mode?,Because light attracts bugs.\n2,Why did the developer go broke?,Because he used up all his cache.\n",
    )
    .unwrap();

    let post_hook_input = json!({
        "hook_event_name": "PostToolUse",
        "session_id": session_id,
        "timestamp": "2026-06-22T20:10:33.212Z",
        "cwd": repo.path().to_str().unwrap(),
        "tool_name": "Edit",
        "tool_input": patch,
        "tool_result": {
            "result_type": "success",
            "text_result_for_llm": "Updated 1 file(s): jokes.csv"
        }
    });
    repo.checkpoint_with_hook_input("github-copilot", &post_hook_input.to_string())
        .unwrap();

    repo.sync_daemon();
    repo.git(&["add", "jokes.csv"]).unwrap();
    repo.git(&["commit", "-m", "Add joke via Claude-format Edit"])
        .unwrap();
    repo.sync_daemon();

    let mut file = repo.filename("jokes.csv");
    file.assert_lines_and_blame(crate::lines![
        "id,setup,punchline".human(),
        "1,Why do programmers prefer dark mode?,Because light attracts bugs.".human(),
        "2,Why did the developer go broke?,Because he used up all his cache.".ai(),
    ]);
}

/// Test Copilot CLI Claude-format `Edit` tool creating a NEW file via an
/// `*** Add File:` apply-patch string (the shape captured from a live 1.0.64 session).
#[test]
fn test_copilot_cli_claude_format_edit_create_attribution() {
    let repo = TestRepo::new();

    // Initial commit so HEAD exists.
    let existing = repo.path().join("readme.md");
    std::fs::write(&existing, "# Hello\n").unwrap();
    repo.git(&["add", "readme.md"]).unwrap();
    repo.git(&["commit", "-m", "Initial"]).unwrap();
    repo.sync_daemon();

    let mut existing_file = repo.filename("readme.md");
    existing_file.assert_committed_lines(crate::lines!["# Hello".unattributed_human()]);

    let session_id = "a5f5793e-0cae-4cfd-9414-b35bce1c127f";
    let new_file = repo.path().join("hello.txt");
    let patch = "*** Begin Patch\n*** Add File: hello.txt\n+hi\n+bye\n*** End Patch\n";

    let pre_hook_input = json!({
        "hook_event_name": "PreToolUse",
        "session_id": session_id,
        "timestamp": "2026-06-22T20:10:33.166Z",
        "cwd": repo.path().to_str().unwrap(),
        "tool_name": "Edit",
        "tool_input": patch,
    });
    repo.checkpoint_with_hook_input("github-copilot", &pre_hook_input.to_string())
        .unwrap();

    // Copilot CLI writes the new file.
    std::fs::write(&new_file, "hi\nbye\n").unwrap();

    let post_hook_input = json!({
        "hook_event_name": "PostToolUse",
        "session_id": session_id,
        "timestamp": "2026-06-22T20:10:33.212Z",
        "cwd": repo.path().to_str().unwrap(),
        "tool_name": "Edit",
        "tool_input": patch,
        "tool_result": {
            "result_type": "success",
            "text_result_for_llm": "Added 1 file(s): hello.txt"
        }
    });
    repo.checkpoint_with_hook_input("github-copilot", &post_hook_input.to_string())
        .unwrap();

    repo.sync_daemon();
    repo.git(&["add", "hello.txt"]).unwrap();
    repo.git(&["commit", "-m", "Add file via Claude-format Edit"])
        .unwrap();
    repo.sync_daemon();

    let mut file = repo.filename("hello.txt");
    file.assert_lines_and_blame(crate::lines!["hi".ai(), "bye".ai()]);
}

/// Test Copilot CLI Claude-format `Write` treats the pre-edit state as empty,
/// matching legacy `create` semantics even if the path already exists on disk.
#[test]
fn test_copilot_cli_claude_format_write_overwrite_attribution() {
    let repo = TestRepo::new();

    let file_path = repo.path().join("story.txt");
    std::fs::write(&file_path, "old draft\n").unwrap();
    repo.git(&["add", "story.txt"]).unwrap();
    repo.git(&["commit", "-m", "Initial draft"]).unwrap();
    repo.sync_daemon();

    let mut file = repo.filename("story.txt");
    file.assert_committed_lines(crate::lines!["old draft".unattributed_human()]);

    let session_id = "a5f5793e-0cae-4cfd-9414-b35bce1c127f";
    let patch =
        "*** Begin Patch\n*** Add File: story.txt\n+new draft\n+second line\n*** End Patch\n";

    let pre_hook_input = json!({
        "hook_event_name": "PreToolUse",
        "session_id": session_id,
        "timestamp": "2026-06-22T20:10:33.166Z",
        "cwd": repo.path().to_str().unwrap(),
        "tool_name": "Write",
        "tool_input": patch,
    });
    repo.checkpoint_with_hook_input("github-copilot", &pre_hook_input.to_string())
        .unwrap();

    std::fs::write(&file_path, "new draft\nsecond line\n").unwrap();

    let post_hook_input = json!({
        "hook_event_name": "PostToolUse",
        "session_id": session_id,
        "timestamp": "2026-06-22T20:10:33.212Z",
        "cwd": repo.path().to_str().unwrap(),
        "tool_name": "Write",
        "tool_input": patch,
        "tool_result": {
            "result_type": "success",
            "text_result_for_llm": "Wrote 1 file(s): story.txt"
        }
    });
    repo.checkpoint_with_hook_input("github-copilot", &post_hook_input.to_string())
        .unwrap();

    repo.sync_daemon();
    repo.git(&["add", "story.txt"]).unwrap();
    repo.git(&["commit", "-m", "Overwrite file via Claude-format Write"])
        .unwrap();
    repo.sync_daemon();

    let mut file = repo.filename("story.txt");
    file.assert_lines_and_blame(crate::lines!["new draft".ai(), "second line".ai()]);
}
