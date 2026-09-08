use super::{
    BTreeMap, BTreeSet, TestRepo, assert_stats_exact, checkpoint_agent_v1, checkpoint_human,
    checkpoint_known_human, commit_after_staging_all, diff_json, tool_model_stats, write_lines,
};

#[test]
fn test_diff_json_all_prompts_includes_non_landing_prompts() {
    let repo = TestRepo::new();

    write_lines(&repo, "all_prompts.txt", &["base"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    // Landed AI prompt: cursor::gpt-4o
    write_lines(&repo, "all_prompts.txt", &["base", "cursor landed"]);
    checkpoint_agent_v1(
        &repo,
        "all_prompts.txt",
        "cursor",
        "gpt-4o",
        "cursor-conv",
        "cursor landed edit",
    );

    // Landed AI prompt: codex::o3
    write_lines(
        &repo,
        "all_prompts.txt",
        &["base", "cursor landed", "codex landed"],
    );
    checkpoint_agent_v1(
        &repo,
        "all_prompts.txt",
        "codex",
        "o3",
        "codex-conv",
        "codex landed edit",
    );

    // Non-landing AI prompt: claude::sonnet (added then removed before commit)
    write_lines(
        &repo,
        "all_prompts.txt",
        &["base", "cursor landed", "codex landed", "claude temp"],
    );
    checkpoint_agent_v1(
        &repo,
        "all_prompts.txt",
        "claude",
        "sonnet",
        "claude-conv",
        "temporary claude edit",
    );
    write_lines(
        &repo,
        "all_prompts.txt",
        &["base", "cursor landed", "codex landed"],
    );
    checkpoint_human(&repo);

    let commit = commit_after_staging_all(&repo, "all-prompts target");

    let all_session_ids: BTreeSet<String> = commit
        .authorship_log
        .metadata
        .sessions
        .keys()
        .cloned()
        .collect();
    // Unscoped checkpoint_human() clears non-landing session metadata
    assert_eq!(
        all_session_ids.len(),
        2,
        "expected two landing sessions (unscoped checkpoint_human clears non-landing sessions)"
    );

    // Verify claude session was cleared by unscoped checkpoint
    let claude_session = commit
        .authorship_log
        .metadata
        .sessions
        .iter()
        .find(|(_, session)| {
            session.agent_id.tool == "claude" && session.agent_id.model == "sonnet"
        });
    assert!(
        claude_session.is_none(),
        "unscoped checkpoint_human should clear non-landing claude session"
    );

    // Sessions appear in the dedicated "sessions" key in diff JSON output
    let without_all_prompts = diff_json(&repo, &["diff", &commit.commit_sha, "--json"]);
    let without_ids: BTreeSet<String> = without_all_prompts["sessions"]
        .as_object()
        .expect("sessions should be an object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        without_ids.len(),
        2,
        "without --all-prompts, return only landing sessions"
    );

    let with_all_prompts = diff_json(
        &repo,
        &["diff", &commit.commit_sha, "--json", "--all-prompts"],
    );
    let with_ids: BTreeSet<String> = with_all_prompts["sessions"]
        .as_object()
        .expect("sessions should be an object")
        .keys()
        .cloned()
        .collect();
    // Diff output includes trace IDs (s_xxx::t_yyy), authorship note only has session IDs (s_xxx)
    // Strip trace IDs for comparison
    let with_ids_base: BTreeSet<String> = with_ids
        .iter()
        .map(|id| id.split("::").next().unwrap_or(id).to_string())
        .collect();
    let without_ids_base: BTreeSet<String> = without_ids
        .iter()
        .map(|id| id.split("::").next().unwrap_or(id).to_string())
        .collect();
    assert_eq!(
        with_ids_base, all_session_ids,
        "--all-prompts returns all sessions from authorship note (2)"
    );
    assert_eq!(
        with_ids_base, without_ids_base,
        "both flags return same 2 sessions"
    );
}

#[test]
fn test_diff_json_include_stats_exact_single_model_counts() {
    let repo = TestRepo::new();

    write_lines(
        &repo,
        "single_model_stats.txt",
        &["base-1", "base-2", "base-3", "base-4"],
    );
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    // Math:
    // - Landed diff: +2 AI lines, -2 lines
    // - Session format: deletions_generated is 0 (no total_deletions in sessions)
    write_lines(
        &repo,
        "single_model_stats.txt",
        &["base-1", "cursor-ai-1", "cursor-ai-2", "base-4"],
    );
    checkpoint_agent_v1(
        &repo,
        "single_model_stats.txt",
        "cursor",
        "gpt-4o",
        "single-model-conv",
        "replace two lines",
    );

    let commit = commit_after_staging_all(&repo, "single model stats target");
    let diff = diff_json(
        &repo,
        &["diff", &commit.commit_sha, "--json", "--include-stats"],
    );
    let commit_stats = diff
        .get("commit_stats")
        .expect("commit_stats should be present with --include-stats");

    let expected_top_level = serde_json::json!({
        "ai_lines_added": 2,
        "human_lines_added": 0,
        "unknown_lines_added": 0,
        "git_lines_added": 2,
        "git_lines_deleted": 2
    });
    let expected_breakdown = BTreeMap::from([("cursor::gpt-4o".to_string(), tool_model_stats(2))]);
    assert_stats_exact(commit_stats, &expected_top_level, &expected_breakdown);
}

#[test]
fn test_diff_json_include_stats_exact_multi_model_with_non_landing_prompt() {
    let repo = TestRepo::new();

    write_lines(&repo, "multi_model_stats.txt", &["base"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    // cursor::gpt-4o adds 3 lines
    write_lines(
        &repo,
        "multi_model_stats.txt",
        &["base", "cursor-1", "cursor-2", "cursor-3"],
    );
    checkpoint_agent_v1(
        &repo,
        "multi_model_stats.txt",
        "cursor",
        "gpt-4o",
        "cursor-main-conv",
        "cursor adds three lines",
    );

    // codex::o3 adds 2 lines
    write_lines(
        &repo,
        "multi_model_stats.txt",
        &[
            "base", "cursor-1", "cursor-2", "cursor-3", "codex-a", "codex-b",
        ],
    );
    checkpoint_agent_v1(
        &repo,
        "multi_model_stats.txt",
        "codex",
        "o3",
        "codex-main-conv",
        "codex adds two lines",
    );

    // Same codex prompt does delete 1 + add 1 (net 0)
    write_lines(
        &repo,
        "multi_model_stats.txt",
        &[
            "base", "cursor-1", "cursor-2", "cursor-3", "codex-a", "codex-b2",
        ],
    );
    checkpoint_agent_v1(
        &repo,
        "multi_model_stats.txt",
        "codex",
        "o3",
        "codex-main-conv",
        "codex replaces one line",
    );

    // Non-landing claude::sonnet prompt (+1 generated, 0 landed)
    write_lines(
        &repo,
        "multi_model_stats.txt",
        &[
            "base",
            "cursor-1",
            "cursor-2",
            "cursor-3",
            "codex-a",
            "codex-b2",
            "claude-temp",
        ],
    );
    checkpoint_agent_v1(
        &repo,
        "multi_model_stats.txt",
        "claude",
        "sonnet",
        "claude-temp-conv",
        "temporary claude line",
    );

    // Human override of one cursor line and remove claude temp line
    write_lines(
        &repo,
        "multi_model_stats.txt",
        &[
            "base",
            "cursor-1",
            "human-override",
            "cursor-3",
            "codex-a",
            "codex-b2",
        ],
    );
    checkpoint_known_human(&repo, "multi_model_stats.txt");

    let commit = commit_after_staging_all(&repo, "multi model stats target");
    let diff = diff_json(
        &repo,
        &["diff", &commit.commit_sha, "--json", "--include-stats"],
    );
    let commit_stats = diff
        .get("commit_stats")
        .expect("commit_stats should be present with --include-stats");

    // Math ledger:
    // - Landed additions in final diff: 5 total
    //   - AI landed: 4 (cursor-1, cursor-3, codex-a, codex-b2)
    //   - Human landed: 1 (human-override)
    // - Landed deletions in final diff: 0
    // - Session format: deletions_generated is 0 (no total_deletions in sessions)
    // - Sessions format: only counts lines that land, not overridden/removed:
    //   - cursor::gpt-4o => landed 2 (cursor-1, cursor-3); cursor-2 overridden not counted
    //   - codex::o3 => landed 2 (codex-a, codex-b2); replacements within session not double-counted
    //   - claude::sonnet => landed 0, session cleared
    // => Only 2 sessions remain (cursor, codex)
    // => totals: ai_lines_added=4 (only landed AI lines)
    let expected_top_level = serde_json::json!({
        "ai_lines_added": 4,
        "human_lines_added": 1,
        "unknown_lines_added": 0,
        "git_lines_added": 5,
        "git_lines_deleted": 0
    });
    // Only sessions with landed lines remain
    let expected_breakdown = BTreeMap::from([
        ("codex::o3".to_string(), tool_model_stats(2)),
        ("cursor::gpt-4o".to_string(), tool_model_stats(2)),
    ]);
    assert_stats_exact(commit_stats, &expected_top_level, &expected_breakdown);

    // Sessions appear in the dedicated "sessions" key in diff JSON output
    let sessions_without_all = diff["sessions"]
        .as_object()
        .expect("sessions should be object");
    // Only sessions with landed lines are included (without --all-prompts)
    // codex is called twice with same conversation_id, so it has one session ID
    // claude has no landed lines, so it's not included
    assert_eq!(
        sessions_without_all.len(),
        2,
        "cursor and codex sessions (codex deduplicated by session ID, claude has no landed lines)"
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_json_all_prompts_includes_non_landing_prompts,
    test_diff_json_include_stats_exact_single_model_counts,
    test_diff_json_include_stats_exact_multi_model_with_non_landing_prompt,
);
