use super::{
    AgentId, AuthorshipLog, BTreeMap, PromptRecord, TestRepo, find_repository_in_path,
    generate_short_hash,
};
use git_ai::operations::authorship::stats::get_git_diff_stats;
use git_ai::operations::authorship::stats::stats_command;
use git_ai::operations::authorship::stats::stats_for_commit_stats;
use git_ai::operations::authorship::stats::stats_from_authorship_log;

#[test]
fn test_stats_for_simple_ai_commit() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "Line1\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds 2 lines
    std::fs::write(repo.path().join("test.txt"), "Line1\nLine 2\nLine 3\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    repo.stage_all_and_commit("AI adds lines").unwrap();

    // Get the commit SHA for the AI commit
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Test our stats function
    let stats = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();

    // Verify the stats
    assert_eq!(
        stats.human_additions, 0,
        "No human additions in AI-only commit"
    );
    assert_eq!(stats.ai_additions, 2, "AI added 2 lines");
    assert_eq!(stats.ai_accepted, 2, "AI lines were accepted");
    assert_eq!(
        stats.git_diff_added_lines, 2,
        "Git diff shows 2 added lines"
    );
    assert_eq!(
        stats.git_diff_deleted_lines, 0,
        "Git diff shows 0 deleted lines"
    );
}

#[test]
fn test_stats_for_mixed_commit() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "Base line\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds lines
    std::fs::write(
        repo.path().join("test.txt"),
        "Base line\nAI line 1\nAI line 2\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Human adds lines
    std::fs::write(
        repo.path().join("test.txt"),
        "Base line\nAI line 1\nAI line 2\nHuman line 1\nHuman line 2\n",
    )
    .unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    repo.stage_all_and_commit("Mixed commit").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let stats = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();

    // Verify the stats
    // trigger_checkpoint_with_author produces KnownHuman checkpoints (post Task 9),
    // so human-written lines have h_-prefixed attestation entries → human_additions.
    assert_eq!(stats.human_additions, 2, "Human added 2 lines");
    assert_eq!(stats.ai_additions, 2, "AI added 2 lines");
    assert_eq!(stats.ai_accepted, 2, "AI lines were accepted");
    assert_eq!(
        stats.git_diff_added_lines, 4,
        "Git diff shows 4 added lines total"
    );
    assert_eq!(
        stats.git_diff_deleted_lines, 0,
        "Git diff shows 0 deleted lines"
    );
}

#[test]
fn test_stats_for_initial_commit() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "Line1\nLine2\nLine3\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();

    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    repo.stage_all_and_commit("Initial commit").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let stats = stats_for_commit_stats(&gitai_repo, &head_sha, &[]).unwrap();

    // KnownHuman checkpoints record h_<hash> attributions for all human-edited lines,
    // so they appear as human_additions (not unknown) even on pure-human commits.
    assert_eq!(
        stats.human_additions, 3,
        "All 3 lines should be KnownHuman-attested human_additions"
    );
    assert_eq!(
        stats.unknown_additions, 0,
        "No unattested lines in a KnownHuman-checkpointed commit"
    );
    assert_eq!(stats.ai_additions, 0, "No AI additions in initial commit");
    assert_eq!(stats.ai_accepted, 0, "No AI lines to accept");
    assert_eq!(
        stats.git_diff_added_lines, 3,
        "Git diff shows 3 added lines (initial commit)"
    );
    assert_eq!(
        stats.git_diff_deleted_lines, 0,
        "Git diff shows 0 deleted lines"
    );
}

#[test]
fn test_stats_for_merge_commit_skips_ai_acceptance() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "base\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    std::fs::write(repo.path().join("test.txt"), "base\nfeature line\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.stage_all_and_commit("Feature change").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    std::fs::write(repo.path().join("main.txt"), "main line\n").unwrap();
    repo.git(&["add", "main.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.stage_all_and_commit("Main change").unwrap();

    repo.git(&["merge", "feature", "-m", "Merge feature"])
        .unwrap();

    let merge_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let stats = stats_for_commit_stats(&gitai_repo, &merge_sha, &[]).unwrap();

    assert_eq!(stats.ai_accepted, 0);
    assert_eq!(stats.ai_additions, 0);
}

#[test]
fn test_stats_command_nonexistent_commit() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.stage_all_and_commit("Commit").unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Non-existent SHA should error
    let result = stats_command(
        &gitai_repo,
        Some("0000000000000000000000000000000000000000"),
        false,
        &[],
    );
    assert!(result.is_err());
}

#[test]
fn test_stats_command_with_json_output() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Commit").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Should succeed with json output
    let result = stats_command(&gitai_repo, Some(&head_sha), true, &[]);
    assert!(result.is_ok());
}

#[test]
fn test_stats_command_default_to_head() {
    let repo = TestRepo::new();

    std::fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Commit").unwrap();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // No SHA provided should default to HEAD
    let result = stats_command(&gitai_repo, None, false, &[]);
    assert!(result.is_ok());
}

#[test]
fn test_get_git_diff_stats_binary_files() {
    let repo = TestRepo::new();

    // Create initial commit
    std::fs::write(repo.path().join("text.txt"), "text\n").unwrap();
    repo.git(&["add", "text.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "text.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial").unwrap();

    // Add binary file (git will detect it as binary if it contains null bytes)
    let binary_content = vec![0u8, 1u8, 2u8, 3u8, 255u8];
    std::fs::write(repo.path().join("binary.bin"), &binary_content).unwrap();
    repo.git(&["add", "binary.bin"]).unwrap();

    repo.stage_all_and_commit("Add binary").unwrap();

    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();

    // Binary files should be handled (shown as "-" in numstat)
    let result = get_git_diff_stats(&gitai_repo, &head_sha, &[]);
    assert!(result.is_ok());
}

#[test]
fn test_stats_from_authorship_log_no_log() {
    let stats = stats_from_authorship_log(None, 10, 5, 3, 0, &BTreeMap::new());

    assert_eq!(stats.git_diff_added_lines, 10);
    assert_eq!(stats.git_diff_deleted_lines, 5);
    assert_eq!(stats.ai_accepted, 3);
    assert_eq!(stats.ai_additions, 3); // ai_accepted when no mixed
    assert_eq!(stats.human_additions, 0); // no known-human attestations passed
    assert_eq!(stats.unknown_additions, 7); // 10 - 3 (unattested lines)
}

#[test]
fn test_stats_from_authorship_log_mixed_cap() {
    // Test that mixed_additions is capped to remaining added lines
    let mut log = AuthorshipLog::new();
    let agent_id = AgentId {
        tool: "cursor".to_string(),
        id: "session".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    let hash = generate_short_hash(&agent_id.id, &agent_id.tool);

    // Prompt with 100 overridden lines (way more than the diff)
    log.metadata.prompts.insert(
        hash,
        PromptRecord {
            agent_id,
            human_author: None,
            total_additions: 50,
            total_deletions: 0,
            accepted_lines: 0,
            overriden_lines: 100, // Unrealistically high
            custom_attributes: None,
            messages_url: None,
        },
    );

    // Only 10 lines added, 5 accepted by AI
    let stats = stats_from_authorship_log(Some(&log), 10, 0, 5, 0, &BTreeMap::new());

    assert_eq!(stats.ai_additions, 5); // ai_accepted
    assert_eq!(stats.human_additions, 0); // no known-human attestations passed
}
