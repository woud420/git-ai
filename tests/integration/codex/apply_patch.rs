use super::{CodexHookInput, ExpectedLineExt, checkpoint_codex, fixture_path, fs, json};

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

crate::reuse_tests_in_worktree!(
    test_codex_e2e_apply_patch_file_edit_full_cycle,
    test_codex_e2e_apply_patch_scoped_to_edited_file_only,
    test_codex_e2e_apply_patch_preserves_human_lines,
    test_codex_e2e_namespaced_apply_patch_file_edit_full_cycle,
);
