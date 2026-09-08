use super::{
    BTreeMap, BTreeSet, ExpectedLineExt, TestRepo, Value, assert_stats_exact, checkpoint_agent_v1,
    checkpoint_human, checkpoint_known_human, commit_after_staging_all,
    commit_with_git_og_as_author, diff_json, write_lines,
};

#[test]
fn test_diff_json_omits_commit_stats_without_include_stats_flag() {
    let repo = TestRepo::new();

    let mut file = repo.filename("stats_omitted.txt");
    file.set_contents(crate::lines!["base".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    file.set_contents(crate::lines!["base".human(), "ai line".ai()]);
    let commit = repo.stage_all_and_commit("Add AI line").unwrap();

    let output = repo
        .git_ai(&["diff", &commit.commit_sha, "--json"])
        .expect("git-ai diff --json should succeed");
    let json: Value = serde_json::from_str(&output).expect("diff JSON should parse");

    assert!(
        json.get("commit_stats").is_none(),
        "commit_stats should be omitted unless --include-stats is provided"
    );
}

#[test]
fn test_diff_json_include_stats_exact_human_landed_with_ai_generated() {
    let repo = TestRepo::new();

    write_lines(&repo, "human_landed_stats.txt", &["base"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    // AI generates two lines, human rewrites both before commit.
    write_lines(
        &repo,
        "human_landed_stats.txt",
        &["base", "ai-temp-1", "ai-temp-2"],
    );
    checkpoint_agent_v1(
        &repo,
        "human_landed_stats.txt",
        "cursor",
        "gpt-4o",
        "human-landed-conv",
        "temporary ai lines",
    );

    write_lines(
        &repo,
        "human_landed_stats.txt",
        &["base", "human-final-1", "human-final-2"],
    );
    checkpoint_known_human(&repo, "human_landed_stats.txt");

    let commit = commit_after_staging_all(&repo, "human landed target");
    let diff = diff_json(
        &repo,
        &["diff", &commit.commit_sha, "--json", "--include-stats"],
    );
    let commit_stats = diff
        .get("commit_stats")
        .expect("commit_stats should be present with --include-stats");

    // Session format: deletions_generated is always 0
    // Sessions are cleared if ALL their lines are overridden (none land)
    let expected_top_level = serde_json::json!({
        "ai_lines_added": 0,
        "human_lines_added": 2,
        "unknown_lines_added": 0,
        "git_lines_added": 2,
        "git_lines_deleted": 0
    });
    // Session is cleared when all lines are overridden
    let expected_breakdown = BTreeMap::new();
    assert_stats_exact(commit_stats, &expected_top_level, &expected_breakdown);
}

#[test]
fn test_diff_json_include_stats_blame_deletions_devin_added_prompts_only() {
    let repo = TestRepo::new();

    write_lines(
        &repo,
        "blame_deletion_stats.txt",
        &["base-1", "base-2", "base-3"],
    );
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    // Commit A: codex prompt that will appear only in deleted hunks of commit B.
    write_lines(
        &repo,
        "blame_deletion_stats.txt",
        &["base-1", "ai-temp-2", "ai-temp-3"],
    );
    checkpoint_agent_v1(
        &repo,
        "blame_deletion_stats.txt",
        "codex",
        "o3",
        "blame-deletion-source",
        "ai replacement",
    );
    let _source_commit = commit_after_staging_all(&repo, "codex source");

    // Commit B: background/synthetic agent commit authored as Devin bot (no authorship note).
    // It deletes codex lines and adds new Devin lines.
    let devin_commit_sha = commit_with_git_og_as_author(
        &repo,
        "blame_deletion_stats.txt",
        &["base-1", "devin-final-4", "devin-final-5"],
        "devin-ai-integration[bot]",
        "158243242+devin-ai-integration[bot]@users.noreply.github.com",
        "devin cleanup",
    );

    let diff = diff_json(
        &repo,
        &[
            "diff",
            &devin_commit_sha,
            "--json",
            "--include-stats",
            "--blame-deletions",
        ],
    );

    // Devin (simulated from agent email) goes to prompts; codex (session-format) goes to sessions
    let prompts = diff["prompts"]
        .as_object()
        .expect("prompts should be object");
    let prompt_tools: BTreeSet<String> = prompts
        .values()
        .map(|prompt| {
            prompt["agent_id"]["tool"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert!(
        prompt_tools.contains("devin"),
        "prompts should include simulated Devin prompt record"
    );

    let sessions = diff["sessions"]
        .as_object()
        .expect("sessions should be object");
    let session_tools: BTreeSet<String> = sessions
        .values()
        .map(|session| {
            session["agent_id"]["tool"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert!(
        session_tools.contains("codex"),
        "sessions should include deleted-origin codex session record"
    );

    let commit_stats = diff
        .get("commit_stats")
        .expect("commit_stats should be present with --include-stats");
    let breakdown = commit_stats["tool_model_breakdown"]
        .as_object()
        .expect("tool_model_breakdown should be an object");
    assert!(
        breakdown.keys().any(|key| key.starts_with("devin::")),
        "expected Devin in tool_model_breakdown, got: {:?}",
        breakdown.keys().collect::<Vec<_>>()
    );
    assert!(
        !breakdown.keys().any(|key| key.starts_with("codex::")),
        "deleted-origin codex prompt should not contribute to commit_stats breakdown"
    );

    assert_eq!(commit_stats["ai_lines_added"], serde_json::json!(2));
    assert_eq!(commit_stats["git_lines_added"], serde_json::json!(2));
    assert_eq!(commit_stats["git_lines_deleted"], serde_json::json!(2));
    assert_eq!(commit_stats["human_lines_added"], serde_json::json!(0));
    assert_eq!(commit_stats["unknown_lines_added"], serde_json::json!(0));
}

crate::reuse_tests_in_worktree!(
    test_diff_json_omits_commit_stats_without_include_stats_flag,
    test_diff_json_include_stats_exact_human_landed_with_ai_generated,
    test_diff_json_include_stats_blame_deletions_devin_added_prompts_only,
);
