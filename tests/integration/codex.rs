use crate::repos::test_file::ExpectedLineExt;
use crate::test_utils::{CodexHookInput, checkpoint_codex, fixture_path, read_jsonl_fixture};
use git_ai::error::GitAiError;
use git_ai::model::stream_watermark::ByteOffsetWatermark;
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use git_ai::operations::streams::agent::Agent;
use git_ai::operations::streams::agents::CodexAgent;
use serde_json::json;
use std::fs;

fn parse_codex(hook_input: &str) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    resolve_preset("codex")?.parse(hook_input, "t_test")
}

mod inflight_commit;

mod tool_cycles;

#[test]
fn test_codex_e2e_apply_patch_file_edit_full_cycle() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("lib.rs");
    fs::write(&file_path, "fn old() {}\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-apply-patch.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::pre_file_edit(
            "codex-apply-patch-session",
            &repo_root,
            "patch-1",
            &file_path,
        )
        .with_patch(format!(
            "*** Update File: {}\n@@ fn old() {{}}\n+fn new_func() {{}}\n",
            file_path.to_string_lossy()
        ))
        .with_transcript_path(&transcript_path),
    );

    fs::write(&file_path, "fn new_func() {}\nfn helper() {}\n").unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::post_file_edit(
            "codex-apply-patch-session",
            &repo_root,
            "patch-1",
            &file_path,
        )
        .with_patch(format!(
            "*** Update File: {}\n@@ fn old() {{}}\n+fn new_func() {{}}\n+fn helper() {{}}\n",
            file_path.to_string_lossy()
        ))
        .with_transcript_path(&transcript_path),
    );

    let commit = repo
        .stage_all_and_commit("Codex apply_patch edit")
        .expect("commit should succeed");

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-apply-patch-session");

    let mut tracked_file = repo.filename("lib.rs");
    tracked_file.assert_lines_and_blame(crate::lines![
        "fn new_func() {}".ai(),
        "fn helper() {}".ai(),
    ]);
}

#[test]
fn test_codex_e2e_apply_patch_scoped_to_edited_file_only() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_a = repo_root.join("a.txt");
    let file_b = repo_root.join("b.txt");
    fs::write(&file_a, "original a\n").unwrap();
    fs::write(&file_b, "original b\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-scoped-patch.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-scoped-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-scoped-1",
        "tool_input": {
            "patch": format!("*** Update File: {}\n@@ original a\n+patched a\n", file_a.to_string_lossy())
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("scoped pre-hook should succeed");

    fs::write(&file_a, "patched a\n").unwrap();
    fs::write(&file_b, "modified b outside codex\n").unwrap();

    let post_hook_input = json!({
        "session_id": "codex-scoped-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-scoped-1",
        "tool_input": {
            "patch": format!("*** Update File: {}\n@@ original a\n+patched a\n", file_a.to_string_lossy())
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("scoped post-hook should succeed");

    repo.stage_all_and_commit("Scoped codex edit")
        .expect("commit should succeed");

    let mut fa = repo.filename("a.txt");
    fa.assert_lines_and_blame(crate::lines!["patched a".ai(),]);

    let mut fb = repo.filename("b.txt");
    fb.assert_lines_and_blame(crate::lines![
        "modified b outside codex".unattributed_human(),
    ]);
}

#[test]
fn test_codex_e2e_apply_patch_preserves_human_lines() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("config.toml");

    repo.human_edit("config.toml", "# human config\nkey = \"value\"\n");
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut config = repo.filename("config.toml");
    config.assert_committed_lines(crate::lines![
        "# human config".human(),
        "key = \"value\"".human(),
    ]);

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-preserve-human.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-preserve-human-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-preserve-1",
        "tool_input": {
            "patch": format!("*** Update File: {}\n@@ key = \"value\"\n+new_key = \"ai_value\"\n", file_path.to_string_lossy())
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("pre-hook should succeed");

    fs::write(
        &file_path,
        "# human config\nkey = \"value\"\nnew_key = \"ai_value\"\n",
    )
    .unwrap();

    let post_hook_input = json!({
        "session_id": "codex-preserve-human-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-preserve-1",
        "tool_input": {
            "patch": format!("*** Update File: {}\n@@ key = \"value\"\n+new_key = \"ai_value\"\n", file_path.to_string_lossy())
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("post-hook should succeed");

    repo.stage_all_and_commit("Codex appends to config")
        .expect("commit should succeed");

    config.assert_lines_and_blame(crate::lines![
        "# human config".human(),
        "key = \"value\"".human(),
        "new_key = \"ai_value\"".ai(),
    ]);
}

#[test]
fn test_codex_e2e_namespaced_apply_patch_file_edit_full_cycle() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("lib.rs");
    fs::write(&file_path, "fn old() {}\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-apply-patch-namespaced.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-apply-patch-namespaced-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "functions.apply_patch",
        "tool_use_id": "patch-namespaced-1",
        "tool_input": {
            "command": format!("*** Update File: {}\n@@ fn old() {{}}\n+fn new_func() {{}}\n", file_path.to_string_lossy())
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("namespaced apply_patch pre-hook should succeed");

    fs::write(&file_path, "fn new_func() {}\nfn helper() {}\n").unwrap();

    let post_hook_input = json!({
        "session_id": "codex-apply-patch-namespaced-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "functions.apply_patch",
        "tool_use_id": "patch-namespaced-1",
        "tool_input": {
            "command": format!("*** Update File: {}\n@@ fn old() {{}}\n+fn new_func() {{}}\n+fn helper() {{}}\n", file_path.to_string_lossy())
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("namespaced apply_patch post-hook should succeed");

    let commit = repo
        .stage_all_and_commit("Codex namespaced apply_patch edit")
        .expect("commit should succeed");

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-apply-patch-namespaced-session");

    let mut tracked_file = repo.filename("lib.rs");
    tracked_file.assert_lines_and_blame(crate::lines![
        "fn new_func() {}".ai(),
        "fn helper() {}".ai(),
    ]);
}

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

#[test]
fn test_codex_file_edit_then_bash_pretooluse_does_not_steal_ai_commit_attribution() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["Project README"]);
    repo.stage_all_and_commit("Initial README").unwrap();

    let repo_root = repo.canonical_path();
    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-bash-status-rollout.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-status-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-status",
        "tool_input": {
            "command": "echo 'Updated by live Codex proof' >> README.md"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("pre-hook checkpoint should succeed");

    fs::write(
        repo_root.join("README.md"),
        "Project README\nUpdated by live Codex proof\n",
    )
    .unwrap();

    let post_hook_input = json!({
        "session_id": "codex-status-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-status",
        "tool_input": {
            "command": "echo 'Updated by live Codex proof' >> README.md"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("post-hook checkpoint should succeed");

    repo.stage_all_and_commit("Codex status commit")
        .expect("Codex status commit should succeed");

    readme.assert_lines_and_blame(crate::lines![
        "Project README".ai(),
        "Updated by live Codex proof".ai(),
    ]);
}

#[test]
fn test_codex_file_edit_then_camel_case_bash_pretooluse_does_not_steal_ai_commit_attribution() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["Project README"]);
    repo.stage_all_and_commit("Initial README").unwrap();

    let repo_root = repo.canonical_path();
    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-bash-status-rollout-camel.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-status-session-camel",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hookEventName": "PreToolUse",
        "toolName": "Bash",
        "toolUseId": "bash-use-status-camel",
        "tool_input": {
            "command": "echo 'Updated by live Codex proof camel' >> README.md"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("pre-hook checkpoint should succeed");

    fs::write(
        repo_root.join("README.md"),
        "Project README\nUpdated by live Codex proof camel\n",
    )
    .unwrap();

    let post_hook_input = json!({
        "session_id": "codex-status-session-camel",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hookEventName": "PostToolUse",
        "toolName": "Bash",
        "toolUseId": "bash-use-status-camel",
        "tool_input": {
            "command": "echo 'Updated by live Codex proof camel' >> README.md"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("post-hook checkpoint should succeed");

    repo.stage_all_and_commit("Codex status camel commit")
        .expect("Codex status camel commit should succeed");

    readme.assert_lines_and_blame(crate::lines![
        "Project README".ai(),
        "Updated by live Codex proof camel".ai(),
    ]);
}

#[test]
fn test_codex_read_only_bash_post_tool_use_before_edit_does_not_steal_commit_attribution() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["Project README"]);
    repo.stage_all_and_commit("Initial README").unwrap();

    let repo_root = repo.canonical_path();
    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-live-readonly-rollout.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let which_git_pre = json!({
        "session_id": "codex-live-readonly-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "which-git",
        "tool_input": { "command": "which git" },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();
    repo.checkpoint_with_hook_input("codex", &which_git_pre)
        .expect("read-only pre-hook should succeed");

    let which_git_post = json!({
        "session_id": "codex-live-readonly-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "which-git",
        "tool_input": { "command": "which git" },
        "tool_response": "/usr/bin/git\n",
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();
    repo.checkpoint_with_hook_input("codex", &which_git_post)
        .expect("read-only post-hook should succeed");

    let commit_pre = json!({
        "session_id": "codex-live-readonly-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "commit-bash",
        "tool_input": {
            "command": "git add README.md && git commit -m \"Codex readonly bash commit\""
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();
    repo.checkpoint_with_hook_input("codex", &commit_pre)
        .expect("commit pre-hook should succeed");

    fs::write(
        repo_root.join("README.md"),
        "Project README\nUpdated after read-only bash\n",
    )
    .unwrap();

    let commit_post = json!({
        "session_id": "codex-live-readonly-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "commit-bash",
        "tool_input": {
            "command": "git add README.md && git commit -m \"Codex readonly bash commit\""
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();
    repo.checkpoint_with_hook_input("codex", &commit_post)
        .expect("commit post-hook should succeed");

    repo.stage_all_and_commit("Codex readonly bash commit")
        .expect("commit should succeed");

    readme.assert_lines_and_blame(crate::lines![
        "Project README".ai(),
        "Updated after read-only bash".ai(),
    ]);
}

#[test]
fn test_codex_raw_event_fidelity() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let agent = CodexAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let result = agent
        .read_incremental(fixture.as_path(), watermark, "test")
        .expect("Should parse codex JSONL");

    let expected = read_jsonl_fixture(&fixture).unwrap();

    assert_eq!(result.events.len(), expected.len());
    assert_eq!(result.events, expected);
}

#[test]
fn test_codex_preset_structured_hook_input() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let hook_input = json!({
        "session_id": "session-abc-123",
        "cwd": "/Users/test/projects/git-ai",
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-1",
        "triggered_at": "2026-02-11T05:53:33Z",
        "hook_event": {
            "event_type": "after_agent",
            "thread_id": "thread-xyz-999",
            "turn_id": "turn-2",
            "input_messages": ["Refactor src/main.rs"],
            "last_assistant_message": "Done."
        },
        "transcript_path": fixture.to_str().unwrap()
    })
    .to_string();

    let events = parse_codex(&hook_input).expect("Codex preset should run");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "codex");
            assert_eq!(
                e.context.external_session_id, "session-abc-123",
                "session_id should be preferred when present"
            );
            assert_eq!(
                e.context.cwd.to_string_lossy(),
                "/Users/test/projects/git-ai"
            );
            assert!(e.stream_source.is_some());
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

// Regression coverage for ENG-324 and upstream git-ai-project/git-ai#2204.
#[test]
fn test_codex_subagent_stream_identity_comes_from_rollout_filename() {
    let parent_id = "01a00000-0000-7000-8000-0000000000aa";
    let child_id = "01a00000-0000-7000-8000-0000000000b1";
    let temp = tempfile::tempdir().unwrap();
    let rollout = temp
        .path()
        .join(format!("rollout-2026-08-31T15-00-00-{child_id}.jsonl"));
    fs::write(
        &rollout,
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{child_id}\",\"forked_from_id\":\"{parent_id}\",\"thread_source\":\"subagent\"}}}}\n"
        ),
    )
    .unwrap();

    let hook_input = json!({
        "session_id": parent_id,
        "cwd": "/tmp/test-project",
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-subagent",
        "transcript_path": rollout,
        "tool_input": {
            "patch": "*** Update File: /tmp/test-project/src/lib.rs\n+child edit\n"
        }
    })
    .to_string();

    let events = parse_codex(&hook_input).expect("Codex subagent hook should parse");
    let ParsedHookEvent::PostFileEdit(event) = &events[0] else {
        panic!("expected PostFileEdit");
    };
    let source = event.stream_source.as_ref().expect("stream source");

    assert_eq!(event.context.external_session_id, parent_id);
    assert_eq!(event.context.agent_id.id, parent_id);
    assert_eq!(source.external_session_id, child_id);
    assert_eq!(
        source.session_id,
        git_ai::model::authorship_log_serialization::generate_session_id(child_id, "codex")
    );
    assert_eq!(
        source.external_parent_session_id.as_deref(),
        Some(parent_id)
    );
}

#[test]
fn test_codex_plain_rollout_keeps_existing_session_identity() {
    let session_id = "01a00000-0000-7000-8000-0000000000cc";
    let temp = tempfile::tempdir().unwrap();
    let rollout = temp
        .path()
        .join(format!("rollout-2026-08-31T15-00-00-{session_id}.jsonl"));
    fs::write(
        &rollout,
        format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n"),
    )
    .unwrap();

    let hook_input = json!({
        "session_id": session_id,
        "cwd": "/tmp/test-project",
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-plain",
        "transcript_path": rollout,
        "tool_input": {
            "patch": "*** Update File: /tmp/test-project/src/lib.rs\n+plain edit\n"
        }
    })
    .to_string();

    let events = parse_codex(&hook_input).expect("plain Codex hook should parse");
    let ParsedHookEvent::PostFileEdit(event) = &events[0] else {
        panic!("expected PostFileEdit");
    };
    let source = event.stream_source.as_ref().expect("stream source");

    assert_eq!(event.context.external_session_id, session_id);
    assert_eq!(source.external_session_id, session_id);
    assert_eq!(source.external_parent_session_id, None);
}

#[test]
fn test_codex_rollout_identity_is_stable_across_equivalent_path_spellings() {
    let child_id = "01a00000-0000-7000-8000-0000000000b1";
    let relative_suffix =
        format!("sessions/2026/08/31/rollout-2026-08-31T15-00-00-{child_id}.jsonl");

    for path in [
        std::path::PathBuf::from("/var/folders/example").join(&relative_suffix),
        std::path::PathBuf::from("/private/var/folders/example").join(&relative_suffix),
    ] {
        assert_eq!(
            CodexAgent::external_session_id_from_rollout_path(&path).as_deref(),
            Some(child_id)
        );
    }
    assert_eq!(
        CodexAgent::external_session_id_from_rollout_path(std::path::Path::new("short.jsonl")),
        None
    );
    assert_eq!(
        CodexAgent::external_session_id_from_rollout_path(std::path::Path::new(
            "/var/folders/example/transcript-2026-08-31T15-00-00-01a00000-0000-7000-8000-0000000000b1.jsonl",
        )),
        None,
        "non-rollout transcript names must fall back to the hook identity"
    );
    assert_eq!(
        CodexAgent::external_session_id_from_rollout_path(std::path::Path::new(
            "/var/folders/example/rollout-2026-08-31T15-00-00-01a00000-0000-7000-8000-0000000000bz.jsonl",
        )),
        None,
        "malformed rollout UUIDs must fail closed"
    );
}

#[test]
fn test_find_rollout_path_for_session_in_home() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let temp = tempfile::tempdir().unwrap();

    let session_id = "019c4b43-1451-7af3-be4c-5576369bf1ba";
    let rollout_dir = temp.path().join("sessions/2026/02/11");
    fs::create_dir_all(&rollout_dir).unwrap();
    let rollout_path = rollout_dir.join(format!("rollout-2026-02-11T05-53-33-{session_id}.jsonl"));
    fs::copy(&fixture, &rollout_path).unwrap();

    let resolved = CodexAgent::find_rollout_path_for_session_in_home(session_id, temp.path())
        .expect("search should succeed")
        .expect("rollout should be found");

    assert_eq!(resolved, rollout_path);
}

crate::reuse_tests_in_worktree!(
    test_codex_e2e_apply_patch_file_edit_full_cycle,
    test_codex_e2e_apply_patch_scoped_to_edited_file_only,
    test_codex_e2e_apply_patch_preserves_human_lines,
    test_codex_e2e_namespaced_apply_patch_file_edit_full_cycle,
    test_codex_preset_bash_pre_tool_use_skips_checkpoint_after_capturing_snapshot,
    test_codex_preset_bash_pre_tool_use_supports_camel_case_hook_event_name,
    test_codex_preset_bash_post_tool_use_detects_changed_files,
    test_codex_file_edit_then_bash_pretooluse_does_not_steal_ai_commit_attribution,
    test_codex_file_edit_then_camel_case_bash_pretooluse_does_not_steal_ai_commit_attribution,
    test_codex_read_only_bash_post_tool_use_before_edit_does_not_steal_commit_attribution,
    test_codex_raw_event_fidelity,
    test_codex_preset_structured_hook_input,
    test_find_rollout_path_for_session_in_home,
);
