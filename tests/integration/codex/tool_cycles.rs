use super::{CodexHookInput, ExpectedLineExt, checkpoint_codex, fixture_path, fs, json};

#[test]
fn test_codex_e2e_bash_pre_and_post_tool_use_full_cycle() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("app.py");
    fs::write(&file_path, "print('hello')\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-bash-full-cycle.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-bash-full-cycle",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-full-1",
        "tool_input": {
            "command": "sed -i '' 's/hello/world/' app.py"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("bash pre-hook should succeed");

    fs::write(&file_path, "print('world')\nprint('from codex')\n").unwrap();

    let post_hook_input = json!({
        "session_id": "codex-bash-full-cycle",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-full-1",
        "tool_input": {
            "command": "sed -i '' 's/hello/world/' app.py"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("bash post-hook should succeed");

    let commit = repo
        .stage_all_and_commit("Codex bash edit")
        .expect("commit should succeed");

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-bash-full-cycle");

    let mut tracked_file = repo.filename("app.py");
    tracked_file.assert_lines_and_blame(crate::lines![
        "print('world')".ai(),
        "print('from codex')".ai(),
    ]);
}

#[test]
fn test_codex_e2e_model_falls_back_to_config() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("lib.rs");
    fs::write(&file_path, "fn old() {}\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Transcript lives under the codex home and carries no model (session_meta
    // has none, turn_context's is null), so resolution falls through to
    // config.toml.
    let codex_home = repo.test_home_path().join(".codex");
    let rollout_dir = codex_home.join("sessions/2026/07/30");
    fs::create_dir_all(&rollout_dir).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        "model = \"config-derived-model\"\n",
    )
    .unwrap();

    let transcript_path = rollout_dir.join("rollout-config-fallback.jsonl");
    let transcript = fs::read_to_string(fixture_path("codex-session-simple.jsonl"))
        .unwrap()
        .replace("\"model\":\"gpt-5-codex\"", "\"model\":null");
    fs::write(&transcript_path, transcript).unwrap();

    let patch = format!(
        "*** Update File: {}\n@@ fn old() {{}}\n+fn configured_model() {{}}\n",
        file_path.to_string_lossy()
    );
    checkpoint_codex(
        &repo,
        CodexHookInput::pre_file_edit(
            "codex-config-model-session",
            &repo_root,
            "patch-config-model-1",
            &file_path,
        )
        .with_patch(patch.clone())
        .with_transcript_path(&transcript_path),
    );

    fs::write(&file_path, "fn configured_model() {}\n").unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::post_file_edit(
            "codex-config-model-session",
            &repo_root,
            "patch-config-model-1",
            &file_path,
        )
        .with_patch(patch)
        .with_transcript_path(&transcript_path),
    );

    let commit = repo
        .stage_all_and_commit("Codex config model edit")
        .expect("commit should succeed");

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-config-model-session");
    assert_eq!(session.agent_id.model, "config-derived-model");

    let mut tracked_file = repo.filename("lib.rs");
    tracked_file.assert_lines_and_blame(crate::lines!["fn configured_model() {}".ai(),]);
}

#[test]
fn test_codex_e2e_bash_then_apply_patch_in_same_session() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("main.py");
    fs::write(&file_path, "# starter\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-mixed.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::pre_bash(
            "codex-mixed-session",
            &repo_root,
            "bash-mixed-1",
            "echo 'setup step'",
        )
        .with_transcript_path(&transcript_path),
    );

    checkpoint_codex(
        &repo,
        CodexHookInput::post_bash(
            "codex-mixed-session",
            &repo_root,
            "bash-mixed-1",
            "echo 'setup step'",
        )
        .with_tool_response("setup step\n")
        .with_transcript_path(&transcript_path),
    );

    checkpoint_codex(
        &repo,
        CodexHookInput::pre_file_edit(
            "codex-mixed-session",
            &repo_root,
            "patch-mixed-1",
            &file_path,
        )
        .with_patch(format!(
            "*** Update File: {}\n@@ # starter\n+# updated by codex\n",
            file_path.to_string_lossy()
        ))
        .with_transcript_path(&transcript_path),
    );

    fs::write(&file_path, "# updated by codex\ndef main(): pass\n").unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::post_file_edit(
            "codex-mixed-session",
            &repo_root,
            "patch-mixed-1",
            &file_path,
        )
        .with_patch(format!(
            "*** Update File: {}\n@@ # starter\n+# updated by codex\n+def main(): pass\n",
            file_path.to_string_lossy()
        ))
        .with_transcript_path(&transcript_path),
    );

    let commit = repo
        .stage_all_and_commit("Mixed codex edit")
        .expect("commit should succeed");

    assert_eq!(
        commit.authorship_log.metadata.sessions.len(),
        1,
        "Both tool uses share the same session"
    );

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-mixed-session");

    let mut tracked_file = repo.filename("main.py");
    tracked_file.assert_lines_and_blame(crate::lines![
        "# updated by codex".ai(),
        "def main(): pass".ai(),
    ]);
}

#[test]
fn test_codex_e2e_bash_modifies_multiple_files_all_attributed() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_a = repo_root.join("src").join("a.rs");
    let file_b = repo_root.join("src").join("b.rs");
    fs::create_dir_all(repo_root.join("src")).unwrap();
    fs::write(&file_a, "// a\n").unwrap();
    fs::write(&file_b, "// b\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-multi-file.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook = json!({
        "session_id": "codex-multi-file-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-multi-1",
        "tool_input": {
            "command": "find src -name '*.rs' -exec sed -i '' 's/\\/\\//\\/\\/ modified/' {} +"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook)
        .expect("pre-hook should succeed");

    fs::write(&file_a, "// modified a\nfn a() {}\n").unwrap();
    fs::write(&file_b, "// modified b\nfn b() {}\n").unwrap();

    let post_hook = json!({
        "session_id": "codex-multi-file-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "bash-multi-1",
        "tool_input": {
            "command": "find src -name '*.rs' -exec sed -i '' 's/\\/\\//\\/\\/ modified/' {} +"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook)
        .expect("post-hook should succeed");

    repo.stage_all_and_commit("Codex multi-file bash edit")
        .expect("commit should succeed");

    let mut fa = repo.filename("src/a.rs");
    fa.assert_lines_and_blame(crate::lines!["// modified a".ai(), "fn a() {}".ai(),]);

    let mut fb = repo.filename("src/b.rs");
    fb.assert_lines_and_blame(crate::lines!["// modified b".ai(), "fn b() {}".ai(),]);
}

#[test]
fn test_codex_e2e_namespaced_exec_command_bash_full_cycle() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("app.py");
    fs::write(&file_path, "print('hello')\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-exec-command-namespaced.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-exec-command-namespaced-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "functions.exec_command",
        "tool_use_id": "exec-namespaced-1",
        "tool_input": {
            "command": "sed -i '' 's/hello/world/' app.py"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("namespaced exec_command pre-hook should succeed");

    fs::write(&file_path, "print('world')\nprint('from codex')\n").unwrap();

    let post_hook_input = json!({
        "session_id": "codex-exec-command-namespaced-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "functions.exec_command",
        "tool_use_id": "exec-namespaced-1",
        "tool_input": {
            "command": "sed -i '' 's/hello/world/' app.py"
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("namespaced exec_command post-hook should succeed");

    let commit = repo
        .stage_all_and_commit("Codex namespaced exec_command edit")
        .expect("commit should succeed");

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-exec-command-namespaced-session");

    let mut tracked_file = repo.filename("app.py");
    tracked_file.assert_lines_and_blame(crate::lines![
        "print('world')".ai(),
        "print('from codex')".ai(),
    ]);
}

#[test]
fn test_codex_e2e_multi_tool_use_parallel_wrapper() {
    use crate::repos::test_repo::TestRepo;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("main.rs");
    fs::write(&file_path, "fn old() {}\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-parallel-wrapper.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "codex-parallel-wrapper-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "multi_tool_use.parallel",
        "tool_use_id": "parallel-1",
        "tool_input": {
            "tool_uses": [
                {
                    "name": "functions.apply_patch",
                    "arguments": {"command": format!("*** Update File: {}\n@@ fn old() {{}}\n+fn new_func() {{}}\n", file_path.to_string_lossy())}
                },
                {
                    "name": "functions.exec_command",
                    "arguments": {"command": "echo 'parallel'"}
                }
            ]
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("multi_tool_use.parallel pre-hook should succeed");

    fs::write(&file_path, "fn new_func() {}\n").unwrap();

    let post_hook_input = json!({
        "session_id": "codex-parallel-wrapper-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "multi_tool_use.parallel",
        "tool_use_id": "parallel-1",
        "tool_input": {
            "tool_uses": [
                {
                    "name": "functions.apply_patch",
                    "arguments": {"command": format!("*** Update File: {}\n@@ fn old() {{}}\n+fn new_func() {{}}\n", file_path.to_string_lossy())}
                },
                {
                    "name": "functions.exec_command",
                    "arguments": {"command": "echo 'parallel'"}
                }
            ]
        },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();

    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("multi_tool_use.parallel post-hook should succeed");

    let commit = repo
        .stage_all_and_commit("Codex multi_tool_use.parallel edit")
        .expect("commit should succeed");

    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("session record should exist");

    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, "codex-parallel-wrapper-session");

    let mut tracked_file = repo.filename("main.rs");
    tracked_file.assert_lines_and_blame(crate::lines!["fn new_func() {}".ai(),]);
}

crate::reuse_tests_in_worktree!(
    test_codex_e2e_bash_pre_and_post_tool_use_full_cycle,
    test_codex_e2e_model_falls_back_to_config,
    test_codex_e2e_bash_then_apply_patch_in_same_session,
    test_codex_e2e_bash_modifies_multiple_files_all_attributed,
    test_codex_e2e_namespaced_exec_command_bash_full_cycle,
    test_codex_e2e_multi_tool_use_parallel_wrapper,
);
