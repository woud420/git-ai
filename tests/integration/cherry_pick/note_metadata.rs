use super::{AgentId, ExpectedLineExt, HashMap, PromptRecord, TestRepo, write_note};

#[test]
fn test_cherry_pick_preserves_human_only_commit_note_metadata() {
    let repo = TestRepo::new();

    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["human-only change"]);
    let source_commit = repo
        .stage_all_and_commit("human-only commit")
        .expect("create source commit");

    let source_log = repo.require_authorship_log(&source_commit.commit_sha);
    assert!(source_log.metadata.prompts.is_empty());
    assert!(source_log.metadata.sessions.is_empty());

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &source_commit.commit_sha])
        .unwrap();
    let new_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let new_log = repo.require_authorship_log(&new_commit);
    assert!(new_log.metadata.prompts.is_empty());
    assert!(new_log.metadata.sessions.is_empty());
    assert_eq!(new_log.metadata.base_commit_sha, new_commit);
}

#[test]
fn test_cherry_pick_preserves_prompt_only_commit_note_metadata() {
    let repo = TestRepo::new();

    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["human-only change"]);
    let source_commit = repo
        .stage_all_and_commit("human-only commit")
        .expect("create source commit");

    let mut source_log = repo.require_authorship_log(&source_commit.commit_sha);
    assert!(
        source_log.metadata.prompts.is_empty(),
        "precondition: source commit should not have AI prompts before test mutation"
    );

    let mut test_attrs = HashMap::new();
    test_attrs.insert("employee_id".to_string(), "E456".to_string());
    test_attrs.insert("team".to_string(), "backend".to_string());
    test_attrs.insert("device_id".to_string(), "MAC-002".to_string());

    source_log.metadata.prompts.insert(
        "prompt-only-session".to_string(),
        PromptRecord {
            agent_id: AgentId {
                tool: "mock_ai".to_string(),
                id: "session-1".to_string(),
                model: "test-model".to_string(),
            },
            human_author: Some("Test User <test@example.com>".to_string()),
            total_additions: 11,
            total_deletions: 2,
            accepted_lines: 0,
            overriden_lines: 0,
            custom_attributes: Some(test_attrs.clone()),
            messages_url: None,
        },
    );

    let mutated_source_note = source_log
        .serialize_to_string()
        .expect("serialize mutated source note");
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(
        &git_ai_repo,
        &source_commit.commit_sha,
        &mutated_source_note,
    )
    .expect("overwrite source note with prompt-only metadata");

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &source_commit.commit_sha])
        .unwrap();
    let new_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let new_log = repo.require_authorship_log(&new_commit);
    assert_eq!(new_log.metadata.prompts.len(), 1);
    assert_eq!(new_log.metadata.base_commit_sha, new_commit);

    let prompt = new_log
        .metadata
        .prompts
        .get("prompt-only-session")
        .expect("prompt metadata should be preserved");
    assert_eq!(prompt.agent_id.tool, "mock_ai");
    assert_eq!(prompt.agent_id.id, "session-1");
    assert_eq!(prompt.agent_id.model, "test-model");
    assert_eq!(prompt.total_additions, 11);
    assert_eq!(prompt.total_deletions, 2);
    assert_eq!(
        prompt.custom_attributes,
        Some(test_attrs),
        "custom_attributes should be preserved through cherry-pick"
    );
}

/// Test cherry-pick preserving multiple AI sessions from different commits
#[test]
fn test_cherry_pick_multiple_ai_sessions() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("main.rs");
    file.set_contents(crate::lines!["fn main() {}"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI session adds logging
    file.replace_at(0, "fn main() {".human());
    file.insert_at(1, crate::lines!["    println!(\"Starting\");".ai()]);
    file.insert_at(2, crate::lines!["}".human()]);
    repo.stage_all_and_commit("Add logging").unwrap();
    let commit1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Second AI session adds error handling
    file.insert_at(2, crate::lines!["    // TODO: Add error handling".ai()]);
    repo.stage_all_and_commit("Add error handling").unwrap();
    let commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Cherry-pick both to main
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &commit1, &commit2]).unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines![
        "fn main() {".ai(),
        "    println!(\"Starting\");".ai(),
        "    // TODO: Add error handling".ai(),
        "}".human(),
    ]);

    // Verify stats for the last cherry-picked commit
    let stats = repo.stats().unwrap();
    assert_eq!(stats.git_diff_added_lines, 1, "Last commit adds 1 line");
    assert_eq!(stats.ai_additions, 1, "1 AI line in last commit");
    assert_eq!(stats.ai_accepted, 1, "1 AI lines accepted");

    // Verify session records exist
    let head_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = git_ai::operations::git::notes_api::read_authorship_v3(
        &git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap(),
        &head_commit,
    )
    .unwrap();

    assert!(
        log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "Should have at least one session record"
    );
    for (session_id, session_record) in &log.metadata.sessions {
        assert!(
            !session_record.agent_id.tool.is_empty(),
            "Session {} should have a non-empty tool",
            session_id
        );
        assert!(
            !session_record.agent_id.model.is_empty(),
            "Session {} should have a non-empty model",
            session_id
        );
    }
}

/// Test that custom attributes set via config are preserved through a cherry-pick
/// when the real post-commit pipeline injects them.
#[test]
fn test_cherry_pick_preserves_custom_attributes_from_config() {
    let mut repo =
        TestRepo::new_with_daemon_scope(crate::repos::test_repo::DaemonTestScope::Dedicated);

    // Configure custom attributes via config patch
    let mut attrs = HashMap::new();
    attrs.insert("employee_id".to_string(), "E101".to_string());
    attrs.insert("team".to_string(), "frontend".to_string());
    attrs.insert("device_id".to_string(), "LNX-007".to_string());
    repo.patch_git_ai_config(|patch| {
        patch.custom_attributes = Some(attrs.clone());
    });

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Initial content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // Create feature branch with AI-authored changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI feature line".ai()]);
    repo.stage_all_and_commit("Add AI feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Verify custom attributes were set on the original commit
    let original_log = repo.require_authorship_log(&feature_commit);
    assert!(
        original_log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !original_log.metadata.sessions.is_empty(),
        "precondition: original commit should have session records"
    );
    for session in original_log.metadata.sessions.values() {
        assert_eq!(
            session.custom_attributes.as_ref(),
            Some(&attrs),
            "precondition: original commit should have custom_attributes from config"
        );
    }

    // Switch back to main and cherry-pick the feature commit
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify custom attributes survived the cherry-pick
    let new_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let new_log = repo.require_authorship_log(&new_commit);
    assert!(
        new_log.metadata.prompts.is_empty(),
        "cherry-picked commit should not have prompts"
    );
    assert!(
        !new_log.metadata.sessions.is_empty(),
        "cherry-picked commit should have session records"
    );
    for session in new_log.metadata.sessions.values() {
        assert_eq!(
            session.custom_attributes.as_ref(),
            Some(&attrs),
            "custom_attributes should be preserved through cherry-pick"
        );
    }

    // Also verify the AI attribution itself survived
    file.assert_lines_and_blame(crate::lines![
        "Initial content".ai(),
        "AI feature line".ai()
    ]);
}

crate::reuse_tests_in_worktree!(
    test_cherry_pick_preserves_human_only_commit_note_metadata,
    test_cherry_pick_preserves_prompt_only_commit_note_metadata,
    test_cherry_pick_multiple_ai_sessions,
);
