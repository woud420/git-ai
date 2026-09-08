use super::{AgentId, ExpectedLineExt, HashMap, PromptRecord, TestRepo, write_note};

#[test]
fn test_rebase_preserves_human_only_commit_note_metadata() {
    let repo = TestRepo::new();

    // Common base commit.
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();
    let default_branch = repo.current_branch();

    // Branch we will rebase onto.
    repo.git(&["checkout", "-b", "dev"]).unwrap();
    let mut dev_file = repo.filename("dev.txt");
    dev_file.set_contents(crate::lines!["dev content"]);
    repo.stage_all_and_commit("Dev commit").unwrap();

    // Create the source branch from the old base and make a human-only commit.
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["checkout", "-b", "prod"]).unwrap();
    let mut prod_file = repo.filename("prod.txt");
    prod_file.set_contents(crate::lines!["human change only"]);
    let prod_commit = repo.stage_all_and_commit("Prod human commit").unwrap();

    // Sanity check: original commit has a note and it's metadata-only.
    let old_log = repo.require_authorship_log(&prod_commit.commit_sha);
    assert!(
        old_log.metadata.prompts.is_empty(),
        "precondition: human-only commit should have no prompts"
    );
    assert!(
        old_log.metadata.sessions.is_empty(),
        "precondition: human-only commit should have no sessions"
    );

    // Rebase prod onto dev.
    repo.git(&["rebase", "dev"]).unwrap();
    let rebased_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Regression check: rebased commit should still carry the metadata-only note.
    let rebased_log = repo.require_authorship_log(&rebased_sha);
    assert!(
        rebased_log.metadata.prompts.is_empty(),
        "rebased human-only commit should still have no prompts"
    );
    assert!(
        rebased_log.metadata.sessions.is_empty(),
        "rebased human-only commit should still have no sessions"
    );
    assert_eq!(rebased_log.metadata.base_commit_sha, rebased_sha);
}

#[test]
fn test_rebase_preserves_prompt_only_commit_note_metadata() {
    let repo = TestRepo::new();

    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "dev"]).unwrap();
    let mut dev_file = repo.filename("dev.txt");
    dev_file.set_contents(crate::lines!["dev content"]);
    repo.stage_all_and_commit("Dev commit").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["checkout", "-b", "prod"]).unwrap();
    let mut prod_file = repo.filename("prod.txt");
    prod_file.set_contents(crate::lines!["human change only"]);
    let prod_commit = repo
        .stage_all_and_commit("Prod human commit")
        .expect("create prod commit");

    let mut original_log = repo.require_authorship_log(&prod_commit.commit_sha);
    assert!(
        original_log.metadata.prompts.is_empty(),
        "precondition: source commit should not have prompts before test mutation"
    );
    assert!(
        original_log.metadata.sessions.is_empty(),
        "precondition: source commit should not have sessions before test mutation"
    );

    let mut test_attrs = HashMap::new();
    test_attrs.insert("employee_id".to_string(), "E123".to_string());
    test_attrs.insert("team".to_string(), "platform".to_string());

    original_log.metadata.prompts.insert(
        "prompt-only-session".to_string(),
        PromptRecord {
            agent_id: AgentId {
                tool: "mock_ai".to_string(),
                id: "session-1".to_string(),
                model: "test-model".to_string(),
            },
            human_author: Some("Test User <test@example.com>".to_string()),
            total_additions: 17,
            total_deletions: 3,
            accepted_lines: 0,
            overriden_lines: 0,
            custom_attributes: Some(test_attrs.clone()),
            messages_url: None,
        },
    );

    let mutated_source_note = original_log
        .serialize_to_string()
        .expect("serialize mutated source note");
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &prod_commit.commit_sha, &mutated_source_note)
        .expect("overwrite source note with prompt-only metadata");

    repo.git(&["rebase", "dev"]).unwrap();
    let rebased_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let rebased_log = repo.require_authorship_log(&rebased_sha);
    assert_eq!(rebased_log.metadata.prompts.len(), 1);
    assert_eq!(rebased_log.metadata.base_commit_sha, rebased_sha);

    let prompt = rebased_log
        .metadata
        .prompts
        .get("prompt-only-session")
        .expect("prompt metadata should be preserved");
    assert_eq!(prompt.agent_id.tool, "mock_ai");
    assert_eq!(prompt.agent_id.id, "session-1");
    assert_eq!(prompt.agent_id.model, "test-model");
    assert_eq!(prompt.total_additions, 17);
    assert_eq!(prompt.total_deletions, 3);
    assert_eq!(
        prompt.custom_attributes,
        Some(test_attrs),
        "custom_attributes should be preserved through rebase"
    );
}

/// Test that custom attributes set via config are preserved through a rebase
/// when the real post-commit pipeline injects them.
#[test]
fn test_rebase_preserves_custom_attributes_from_config() {
    let mut repo =
        TestRepo::new_with_daemon_scope(crate::repos::test_repo::DaemonTestScope::Dedicated);

    // Configure custom attributes via config patch
    let mut attrs = HashMap::new();
    attrs.insert("employee_id".to_string(), "E789".to_string());
    attrs.insert("team".to_string(), "infra".to_string());
    repo.patch_git_ai_config(|patch| {
        patch.custom_attributes = Some(attrs.clone());
    });

    // Create initial commit on default branch
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    // Create feature branch with AI commit
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature code".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Verify custom attributes were set on the original commit
    let original_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let original_log = repo.require_authorship_log(&original_sha);
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

    // Advance default branch (non-conflicting)
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other content"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify custom attributes survived the rebase
    let rebased_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let rebased_log = repo.require_authorship_log(&rebased_sha);
    assert!(
        rebased_log.metadata.prompts.is_empty(),
        "rebased commit should not have prompts"
    );
    assert!(
        !rebased_log.metadata.sessions.is_empty(),
        "rebased commit should have session records"
    );
    for session in rebased_log.metadata.sessions.values() {
        assert_eq!(
            session.custom_attributes.as_ref(),
            Some(&attrs),
            "custom_attributes should be preserved through rebase"
        );
    }

    // Also verify the AI attribution itself survived
    feature_file.assert_lines_and_blame(crate::lines!["// AI feature code".ai()]);
}

/// Regression test: prompt metrics (accepted_lines) must update per commit, not be frozen
/// from the initial state. When commit 1 has 2 AI lines and commit 2 adds 2 more
/// (total 4), the rebased notes should reflect different accepted_lines.
#[test]
fn test_rebase_prompt_metrics_update_per_commit() {
    let repo = TestRepo::new();
    let default_branch = repo.current_branch();

    // Initial setup
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Commit 1: add 2 AI lines
    let mut ai_file = repo.filename("feature.txt");
    ai_file.set_contents(crate::lines!["line1".ai(), "line2".ai()]);
    let commit1 = repo.stage_all_and_commit("AI commit 1 - 2 lines").unwrap();

    // Commit 2: add 2 more AI lines (total 4)
    ai_file.set_contents(crate::lines![
        "line1".ai(),
        "line2".ai(),
        "line3".ai(),
        "line4".ai()
    ]);
    let commit2 = repo.stage_all_and_commit("AI commit 2 - 4 lines").unwrap();

    // Verify pre-rebase: commit 1 has 2 accepted, commit 2 has 4
    let log1 = repo.require_authorship_log(&commit1.commit_sha);
    let log2 = repo.require_authorship_log(&commit2.commit_sha);

    // Session format: verify pre-rebase sessions exist and attestation line counts differ
    assert!(
        log1.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        log2.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !log1.metadata.sessions.is_empty(),
        "precondition: commit 1 should have session records"
    );
    assert!(
        !log2.metadata.sessions.is_empty(),
        "precondition: commit 2 should have session records"
    );
    let pre_lines_1: u32 = log1
        .attestations
        .iter()
        .flat_map(|a| &a.entries)
        .flat_map(|e| &e.line_ranges)
        .map(|r| match r {
            git_ai::model::authorship_log::LineRange::Single(_) => 1,
            git_ai::model::authorship_log::LineRange::Range(s, e) => e - s + 1,
        })
        .sum();
    let pre_lines_2: u32 = log2
        .attestations
        .iter()
        .flat_map(|a| &a.entries)
        .flat_map(|e| &e.line_ranges)
        .map(|r| match r {
            git_ai::model::authorship_log::LineRange::Single(_) => 1,
            git_ai::model::authorship_log::LineRange::Range(s, e) => e - s + 1,
        })
        .sum();
    assert!(
        pre_lines_1 < pre_lines_2,
        "precondition: commit 2 ({}) should have more attested lines than commit 1 ({})",
        pre_lines_2,
        pre_lines_1
    );

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Get rebased commit SHAs
    let rebased_tip = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let rebased_parent = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    // Verify post-rebase: metrics should differ between the two commits
    let rebased_log1 = repo.require_authorship_log(&rebased_parent);
    let rebased_log2 = repo.require_authorship_log(&rebased_tip);

    // Session format: verify sessions survive rebase and attestation line counts differ
    assert!(
        rebased_log1.metadata.prompts.is_empty(),
        "rebased commit 1 should not have prompts"
    );
    assert!(
        rebased_log2.metadata.prompts.is_empty(),
        "rebased commit 2 should not have prompts"
    );
    assert!(
        !rebased_log1.metadata.sessions.is_empty(),
        "regression: rebased commit 1 should have session records"
    );
    assert!(
        !rebased_log2.metadata.sessions.is_empty(),
        "regression: rebased commit 2 should have session records"
    );
    let post_lines_1: u32 = rebased_log1
        .attestations
        .iter()
        .flat_map(|a| &a.entries)
        .flat_map(|e| &e.line_ranges)
        .map(|r| match r {
            git_ai::model::authorship_log::LineRange::Single(_) => 1,
            git_ai::model::authorship_log::LineRange::Range(s, e) => e - s + 1,
        })
        .sum();
    let post_lines_2: u32 = rebased_log2
        .attestations
        .iter()
        .flat_map(|a| &a.entries)
        .flat_map(|e| &e.line_ranges)
        .map(|r| match r {
            git_ai::model::authorship_log::LineRange::Single(_) => 1,
            git_ai::model::authorship_log::LineRange::Range(s, e) => e - s + 1,
        })
        .sum();
    assert!(
        post_lines_1 < post_lines_2,
        "regression: rebased commit 2 ({}) should have more attested lines than commit 1 ({}). \
         If equal, the fast path is freezing metrics across commits.",
        post_lines_2,
        post_lines_1
    );
}

crate::reuse_tests_in_worktree!(
    test_rebase_preserves_human_only_commit_note_metadata,
    test_rebase_preserves_prompt_only_commit_note_metadata,
    test_rebase_prompt_metrics_update_per_commit,
);
