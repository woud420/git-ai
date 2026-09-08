use super::{CodexHookInput, ExpectedLineExt, checkpoint_codex, fixture_path, fs, json};

#[test]
fn test_codex_commit_inside_bash_inflight_is_attributed_to_codex() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let src_dir = repo_root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let file_path = src_dir.join("main.rs");
    fs::write(&file_path, "fn main() {}\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-bash-rollout.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::pre_bash(
            "codex-bash-session",
            &repo_root,
            "bash-use-commit",
            "python - <<'PY'\nprint('commit from codex bash')\nPY",
        )
        .with_transcript_path(&transcript_path),
    );

    fs::write(
        &file_path,
        "fn greet() { println!(\"hello\"); }\nfn main() { greet(); }\n",
    )
    .unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::post_bash(
            "codex-bash-session",
            &repo_root,
            "bash-use-commit",
            "python - <<'PY'\nprint('commit from codex bash')\nPY",
        )
        .with_transcript_path(&transcript_path),
    );

    let commit = repo
        .stage_all_and_commit("Apply codex bash refactor")
        .expect("commit should succeed");

    assert_eq!(
        commit.authorship_log.metadata.sessions.len(),
        1,
        "Expected one session record from the Codex bash context"
    );

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("Session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-bash-session");

    let mut tracked_file = repo.filename("src/main.rs");
    tracked_file.assert_lines_and_blame(crate::lines![
        "fn greet() { println!(\"hello\"); }".ai(),
        "fn main() { greet(); }".ai(),
    ]);
}

#[test]
fn test_codex_commit_inside_bash_inflight_repeated_append_keeps_file_ai() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    repo.human_edit("README.md", "Project README\n");
    let mut readme = repo.filename("README.md");
    repo.stage_all_and_commit("Initial README")
        .expect("initial README commit should succeed");

    let repo_root = repo.canonical_path();
    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-bash-append-rollout.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-bash-append-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("pre-hook checkpoint should succeed");

    readme.set_contents(crate::lines!["Project README", "Updated by Codex".ai()]);

    let post_hook_input = json!({
        "session_id": "codex-bash-append-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("post-hook checkpoint should succeed");

    repo.stage_all_and_commit("Codex append proof")
        .expect("Codex append commit should succeed");

    readme.assert_lines_and_blame(crate::lines![
        "Project README".human(),
        "Updated by Codex".ai(),
    ]);

    let second_pre_hook_input = json!({
        "session_id": "codex-bash-append-session-2",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit-2",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof 2'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &second_pre_hook_input)
        .expect("second pre-hook checkpoint should succeed");

    readme.set_contents(crate::lines![
        "Project README",
        "Updated by Codex".ai(),
        "Updated again by Codex".ai(),
    ]);

    let second_post_hook_input = json!({
        "session_id": "codex-bash-append-session-2",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit-2",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof 2'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &second_post_hook_input)
        .expect("second post-hook checkpoint should succeed");

    repo.stage_all_and_commit("Codex append proof 2")
        .expect("second Codex append commit should succeed");

    readme.assert_lines_and_blame(crate::lines![
        "Project README".human(),
        "Updated by Codex".ai(),
        "Updated again by Codex".ai(),
    ]);
}

#[test]
fn test_codex_commit_inside_bash_inflight_repeated_append_keeps_file_ai_standard_human() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["Project README".unattributed_human()]);
    repo.stage_all_and_commit("Initial README")
        .expect("initial README commit should succeed");

    let repo_root = repo.canonical_path();
    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-bash-append-rollout-standard-human.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-bash-append-session-sh",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit-sh",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("pre-hook checkpoint should succeed");

    let readme_path = repo_root.join("README.md");
    fs::write(&readme_path, "Project README\nUpdated by Codex").unwrap();

    let post_hook_input = json!({
        "session_id": "codex-bash-append-session-sh",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit-sh",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("post-hook checkpoint should succeed");

    repo.stage_all_and_commit("Codex append proof")
        .expect("Codex append commit should succeed");

    readme.assert_lines_and_blame(crate::lines![
        "Project README".ai(),
        "Updated by Codex".ai(),
    ]);

    let second_pre_hook_input = json!({
        "session_id": "codex-bash-append-session-2-sh",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit-2-sh",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof 2'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &second_pre_hook_input)
        .expect("second pre-hook checkpoint should succeed");

    fs::write(
        &readme_path,
        "Project README\nUpdated by Codex\nUpdated again by Codex",
    )
    .unwrap();

    let second_post_hook_input = json!({
        "session_id": "codex-bash-append-session-2-sh",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-use-append-commit-2-sh",
        "tool_input": {
            "command": "git add README.md && git commit -m 'Codex append proof 2'"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &second_post_hook_input)
        .expect("second post-hook checkpoint should succeed");

    repo.stage_all_and_commit("Codex append proof 2")
        .expect("second Codex append commit should succeed");

    readme.assert_lines_and_blame(crate::lines![
        "Project README".ai(),
        "Updated by Codex".ai(),
        "Updated again by Codex".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_codex_commit_inside_bash_inflight_is_attributed_to_codex,
    test_codex_commit_inside_bash_inflight_repeated_append_keeps_file_ai,
    test_codex_commit_inside_bash_inflight_repeated_append_keeps_file_ai_standard_human,
);
