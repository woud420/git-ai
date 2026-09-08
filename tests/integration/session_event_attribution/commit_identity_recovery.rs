use super::{
    CodexHookInput, ExpectedLineExt, TestRepo, assert_session_attests_lines, checkpoint_codex,
    file_mtime_secs, fs, generate_session_id, insert_session_event_for_tool,
    isolated_metrics_db_path, session_ids_for_tool,
};

#[test]
fn test_commit_metadata_recovery_uses_existing_matching_session_after_edge_expansion() {
    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });
    let file_path = repo.path().join("metadata-existing.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial base")
        .expect("initial commit should succeed");
    let mut file = repo.filename("metadata-existing.txt");
    file.assert_committed_lines(lines!["base".unattributed_human()]);

    let external_session_id = "codex-existing-metadata-session";
    checkpoint_codex(
        &repo,
        CodexHookInput::pre_file_edit(
            external_session_id,
            repo.canonical_path(),
            "metadata-existing-tool",
            &file_path,
        )
        .with_model("gpt-5"),
    );

    fs::write(&file_path, "base\ncodex line\n").unwrap();
    checkpoint_codex(
        &repo,
        CodexHookInput::post_file_edit(
            external_session_id,
            repo.canonical_path(),
            "metadata-existing-tool",
            &file_path,
        )
        .with_model("gpt-5"),
    );

    fs::write(
        &file_path,
        "\
base
codex line
unknown 1
unknown 2
unknown 3
unknown 4
unknown 5
unknown 6
",
    )
    .unwrap();

    let commit = repo
        .stage_all_and_commit(
            "Recover from Codex trailer\n\nCo-authored-by: Codex <noreply@openai.com>",
        )
        .expect("commit should succeed");

    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "codex line".ai(),
        "unknown 1".ai(),
        "unknown 2".ai(),
        "unknown 3".ai(),
        "unknown 4".ai(),
        "unknown 5".ai(),
        "unknown 6".ai(),
    ]);
    let expected_session_id = generate_session_id(external_session_id, "codex");
    assert_session_attests_lines(
        &commit.authorship_log,
        "metadata-existing.txt",
        &expected_session_id,
        &[2, 3, 4, 5, 6, 7, 8],
    );
}

#[test]
fn test_commit_metadata_recovery_skips_when_edge_expansion_recovers_all_unknown_lines() {
    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });
    let file_path = repo.path().join("metadata-edge-skip.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial base")
        .expect("initial commit should succeed");
    let mut file = repo.filename("metadata-edge-skip.txt");
    file.assert_committed_lines(lines!["base".unattributed_human()]);

    let external_session_id = "codex-edge-skip-session";
    checkpoint_codex(
        &repo,
        CodexHookInput::pre_file_edit(
            external_session_id,
            repo.canonical_path(),
            "metadata-edge-skip-tool",
            &file_path,
        )
        .with_model("gpt-5"),
    );

    fs::write(&file_path, "base\ncodex line\n").unwrap();
    checkpoint_codex(
        &repo,
        CodexHookInput::post_file_edit(
            external_session_id,
            repo.canonical_path(),
            "metadata-edge-skip-tool",
            &file_path,
        )
        .with_model("gpt-5"),
    );

    fs::write(
        &file_path,
        "\
base
codex line
edge 1
edge 2
edge 3
",
    )
    .unwrap();

    let commit = repo
        .stage_all_and_commit(
            "Edge should win before metadata\n\nCo-authored-by: Claude <noreply@anthropic.com>",
        )
        .expect("commit should succeed");

    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "codex line".ai(),
        "edge 1".ai(),
        "edge 2".ai(),
        "edge 3".ai(),
    ]);
    let expected_session_id = generate_session_id(external_session_id, "codex");
    assert_session_attests_lines(
        &commit.authorship_log,
        "metadata-edge-skip.txt",
        &expected_session_id,
        &[2, 3, 4, 5],
    );
    assert!(
        session_ids_for_tool(&commit.authorship_log, "claude").is_empty(),
        "commit metadata should not synthesize a Claude session after edge recovery removes all unknown lines"
    );
}

#[test]
fn test_commit_metadata_recovery_uses_nearest_matching_tool_session_event() {
    let (_metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    repo.git(&["commit", "--allow-empty", "-m", "initial"])
        .expect("initial empty commit should succeed");

    let file_path = repo.path().join("metadata-nearest.txt");
    fs::write(&file_path, "cursor recovered from trailer\n").unwrap();
    let file_ts = file_mtime_secs(&file_path);
    let far_session_id = insert_session_event_for_tool(
        &metrics_db_path,
        file_ts.saturating_sub(2),
        "cursor",
        "cursor-far-session",
        "cursor-far-tool",
        None,
    );
    let near_session_id = insert_session_event_for_tool(
        &metrics_db_path,
        file_ts,
        "cursor",
        "cursor-near-session",
        "cursor-near-tool",
        None,
    );

    let commit = repo
        .stage_all_and_commit(
            "Recover from Cursor trailer\n\nCo-authored-by: Cursor <cursoragent@cursor.com>",
        )
        .expect("commit should succeed");

    let mut file = repo.filename("metadata-nearest.txt");
    file.assert_committed_lines(lines!["cursor recovered from trailer".ai()]);
    assert!(
        !commit
            .authorship_log
            .metadata
            .sessions
            .contains_key(&far_session_id),
        "nearest timestamp recovery should not select the farther Cursor session"
    );
    assert_session_attests_lines(
        &commit.authorship_log,
        "metadata-nearest.txt",
        &near_session_id,
        &[1],
    );
}

#[test]
fn test_commit_metadata_recovery_falls_back_to_most_recent_matching_tool_session() {
    let (_metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    repo.git(&["commit", "--allow-empty", "-m", "initial"])
        .expect("initial empty commit should succeed");

    let file_path = repo.path().join("metadata-latest.txt");
    fs::write(&file_path, "claude recovered from trailer\n").unwrap();
    let file_ts = file_mtime_secs(&file_path);
    let older_session_id = insert_session_event_for_tool(
        &metrics_db_path,
        file_ts.saturating_sub(200),
        "claude",
        "claude-older-session",
        "claude-older-tool",
        None,
    );
    let latest_session_id = insert_session_event_for_tool(
        &metrics_db_path,
        file_ts.saturating_sub(100),
        "claude",
        "claude-latest-session",
        "claude-latest-tool",
        None,
    );

    let commit = repo
        .stage_all_and_commit(
            "Recover from Claude trailer\n\nCo-authored-by: Claude Opus 4.8 <noreply@anthropic.com>",
        )
        .expect("commit should succeed");

    let mut file = repo.filename("metadata-latest.txt");
    file.assert_committed_lines(lines!["claude recovered from trailer".ai()]);
    assert!(
        !commit
            .authorship_log
            .metadata
            .sessions
            .contains_key(&older_session_id),
        "latest fallback should not select an older Claude session"
    );
    assert_session_attests_lines(
        &commit.authorship_log,
        "metadata-latest.txt",
        &latest_session_id,
        &[1],
    );
}

#[test]
fn test_commit_metadata_recovery_latest_matching_tool_prefers_current_repo_url() {
    let (_metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/acme/metadata-repo-url.git",
    ])
    .expect("remote add should succeed");
    repo.git(&["commit", "--allow-empty", "-m", "initial"])
        .expect("initial empty commit should succeed");

    let file_path = repo.path().join("metadata-latest-repo-url.txt");
    fs::write(&file_path, "cursor recovered with repo url\n").unwrap();
    let file_ts = file_mtime_secs(&file_path);
    let other_repo_session_id = insert_session_event_for_tool(
        &metrics_db_path,
        file_ts.saturating_sub(100),
        "cursor",
        "cursor-other-repo-session",
        "cursor-other-repo-tool",
        Some("https://github.com/acme/other-repo"),
    );
    let same_repo_session_id = insert_session_event_for_tool(
        &metrics_db_path,
        file_ts.saturating_sub(200),
        "cursor",
        "cursor-same-repo-session",
        "cursor-same-repo-tool",
        Some("https://github.com/acme/metadata-repo-url"),
    );

    let commit = repo
        .stage_all_and_commit(
            "Recover from Cursor trailer\n\nCo-authored-by: Cursor <cursoragent@cursor.com>",
        )
        .expect("commit should succeed");

    let mut file = repo.filename("metadata-latest-repo-url.txt");
    file.assert_committed_lines(lines!["cursor recovered with repo url".ai()]);
    assert!(
        !commit
            .authorship_log
            .metadata
            .sessions
            .contains_key(&other_repo_session_id),
        "latest fallback should reject an explicit session event from another repo"
    );
    assert_session_attests_lines(
        &commit.authorship_log,
        "metadata-latest-repo-url.txt",
        &same_repo_session_id,
        &[1],
    );
}

#[test]
fn test_commit_metadata_recovery_synthesizes_session_from_agent_author_email() {
    let (_metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    repo.git(&["commit", "--allow-empty", "-m", "initial"])
        .expect("initial empty commit should succeed");

    let file_path = repo.path().join("metadata-author-email.txt");
    fs::write(&file_path, "codex author email fallback\n").unwrap();

    let commit = repo
        .stage_all_and_commit_with_env(
            "Recover from author email",
            &[
                ("GIT_AUTHOR_NAME", "Codex"),
                ("GIT_AUTHOR_EMAIL", "codex@openai.com"),
            ],
        )
        .expect("commit should succeed");

    let mut file = repo.filename("metadata-author-email.txt");
    file.assert_committed_lines(lines!["codex author email fallback".ai()]);

    let codex_sessions = session_ids_for_tool(&commit.authorship_log, "codex");
    assert_eq!(
        codex_sessions.len(),
        1,
        "author-email fallback should synthesize one Codex session"
    );
    assert!(
        codex_sessions[0].starts_with("s_"),
        "synthesized session ids should use the session id namespace"
    );
    assert_session_attests_lines(
        &commit.authorship_log,
        "metadata-author-email.txt",
        &codex_sessions[0],
        &[1],
    );
}

#[test]
fn test_commit_metadata_recovery_ignores_freeform_message_agent_mentions() {
    let repo = TestRepo::new();

    let file_path = repo.path().join("metadata-freeform-agent-mention.txt");
    fs::write(&file_path, "freeform codex mention should stay unknown\n").unwrap();

    let commit = repo
        .stage_all_and_commit_with_env(
            "codex did things",
            &[
                ("GIT_AUTHOR_NAME", "Sasha Varlamov"),
                ("GIT_AUTHOR_EMAIL", "sasha@sashavarlamov.com"),
            ],
        )
        .expect("freeform mention commit should succeed");

    let mut file = repo.filename("metadata-freeform-agent-mention.txt");
    file.assert_committed_lines(lines![
        "freeform codex mention should stay unknown".unattributed_human()
    ]);
    assert!(
        commit.authorship_log.metadata.sessions.is_empty(),
        "freeform message text mentioning Codex should not synthesize an AI session"
    );
}

#[test]
fn test_commit_metadata_recovery_ignores_ambiguous_identity_markers() {
    let repo = TestRepo::new();

    let amp_file_path = repo.path().join("metadata-ambiguous-amp.txt");
    fs::write(&amp_file_path, "ambiguous amp trailer\n").unwrap();
    let amp_commit = repo
        .stage_all_and_commit("Ambiguous AMP trailer\n\nCo-authored-by: AMP Team <team@amp.dev>")
        .expect("amp trailer commit should succeed");

    let mut amp_file = repo.filename("metadata-ambiguous-amp.txt");
    amp_file.assert_committed_lines(lines!["ambiguous amp trailer".unattributed_human()]);
    assert!(
        amp_commit.authorship_log.metadata.sessions.is_empty(),
        "ambiguous AMP identity should not synthesize an AI session"
    );

    let continue_file_path = repo.path().join("metadata-ambiguous-continue.txt");
    fs::write(&continue_file_path, "ambiguous continue trailer\n").unwrap();
    let continue_commit = repo
        .stage_all_and_commit(
            "Ambiguous Continue trailer\n\nCo-authored-by: Continue Project <dev@continue.dev>",
        )
        .expect("continue trailer commit should succeed");

    let mut continue_file = repo.filename("metadata-ambiguous-continue.txt");
    continue_file.assert_committed_lines(lines!["ambiguous continue trailer".unattributed_human()]);
    assert!(
        continue_commit.authorship_log.metadata.sessions.is_empty(),
        "ambiguous Continue identity should not synthesize an AI session"
    );

    let openai_file_path = repo.path().join("metadata-ambiguous-openai.txt");
    fs::write(&openai_file_path, "generic openai noreply trailer\n").unwrap();
    let openai_commit = repo
        .stage_all_and_commit(
            "Ambiguous OpenAI trailer\n\nCo-authored-by: OpenAI Release Bot <noreply@openai.com>",
        )
        .expect("generic OpenAI noreply trailer commit should succeed");

    let mut openai_file = repo.filename("metadata-ambiguous-openai.txt");
    openai_file.assert_committed_lines(lines![
        "generic openai noreply trailer".unattributed_human()
    ]);
    assert!(
        openai_commit.authorship_log.metadata.sessions.is_empty(),
        "generic OpenAI noreply identity should not synthesize a Codex session"
    );

    for (file_name, line, identity, message) in [
        (
            "metadata-ambiguous-claude.txt",
            "human named claude trailer",
            "Claude Smith <claude.smith@example.com>",
            "ambiguous Claude human identity should not synthesize an AI session",
        ),
        (
            "metadata-ambiguous-devin.txt",
            "human named devin trailer",
            "Devin Patel <devin.patel@example.com>",
            "ambiguous Devin human identity should not synthesize an AI session",
        ),
        (
            "metadata-ambiguous-gemini.txt",
            "human named gemini trailer",
            "Gemini Jones <gemini.jones@example.com>",
            "ambiguous Gemini human identity should not synthesize an AI session",
        ),
    ] {
        let path = repo.path().join(file_name);
        fs::write(&path, format!("{line}\n")).unwrap();
        let commit = repo
            .stage_all_and_commit(&format!(
                "Ambiguous human trailer\n\nCo-authored-by: {identity}"
            ))
            .expect("ambiguous human identity commit should succeed");

        let mut file = repo.filename(file_name);
        file.assert_committed_lines(lines![line.unattributed_human()]);
        assert!(
            commit.authorship_log.metadata.sessions.is_empty(),
            "{message}"
        );
    }
}
