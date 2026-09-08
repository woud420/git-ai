use super::{ExpectedLineExt, TestRepo, Value, fs, write_note};

// Test 15: show-prompt with an old-format prompt ID finds it in metadata.prompts
#[test]
fn test_show_prompt_finds_old_format_prompt_by_id() {
    let repo = TestRepo::new();

    // Create commit with AI content
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Human line", "AI line".ai()]);
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Replace with old-format note
    let old_hash = "abcd1234efgh5678";
    let old_note = format!(
        r#"test.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "show_prompt_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 5,
      "total_deletions": 2,
      "accepted_lines": 3,
      "overriden_lines": 1
    }}
  }}
}}"#,
        old_hash, commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // show-prompt with --commit should find the old-format prompt
    let output = repo
        .git_ai(&["show-prompt", old_hash, "--commit", "HEAD"])
        .expect("show-prompt should find old-format prompt");

    let json: Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(json["prompt_id"].as_str(), Some(old_hash));
    assert_eq!(json["prompt"]["agent_id"]["tool"].as_str(), Some("cursor"));
    assert_eq!(json["prompt"]["agent_id"]["model"].as_str(), Some("gpt-4"));
}

// Test 16: show-prompt searches history for old-format prompt
#[test]
fn test_show_prompt_finds_old_format_prompt_in_history() {
    let repo = TestRepo::new();

    // Create commit with AI content
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Human line", "AI line".ai()]);
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Replace with old-format note
    let old_hash = "1122334455667788";
    let old_note = format!(
        r#"test.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "windsurf", "id": "history_session", "model": "claude-3.5"}},
      "human_author": null,
      "messages": [],
      "total_additions": 10,
      "total_deletions": 3,
      "accepted_lines": 7,
      "overriden_lines": 2
    }}
  }}
}}"#,
        old_hash, commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // show-prompt without --commit should search history and find it
    let output = repo
        .git_ai(&["show-prompt", old_hash])
        .expect("show-prompt should find old-format prompt in history");

    let json: Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(json["prompt_id"].as_str(), Some(old_hash));
    assert_eq!(
        json["prompt"]["agent_id"]["tool"].as_str(),
        Some("windsurf")
    );
}

// Test 17: git-ai stats --json works correctly with old-format notes.
// After the stats simplification (PR #1154), prompt-era fields like total_additions,
// total_deletions, and overriden_lines are no longer surfaced. Stats are now purely
// diff-based. This test verifies that old-format notes don't break stats and that
// diff-based ai_accepted still works correctly.
#[test]
fn test_stats_json_works_with_old_format_notes() {
    let repo = TestRepo::new();

    // Create commit with AI content
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Human line", "AI line".ai()]);
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Replace with old-format note that has specific stats
    let old_hash = "aabb11223344ccdd";
    let old_note = format!(
        r#"test.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "stats_session", "model": "gpt-4o"}},
      "human_author": null,
      "messages": [],
      "total_additions": 15,
      "total_deletions": 5,
      "accepted_lines": 8,
      "overriden_lines": 3
    }}
  }}
}}"#,
        old_hash, commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // Run git-ai stats --json — should not crash on old-format notes
    let output = repo
        .git_ai(&["stats", "--json"])
        .expect("stats should work with old-format notes");
    let json: Value = serde_json::from_str(output.trim()).unwrap();

    // Diff-based ai_accepted should still correctly count AI lines from the attestation
    let ai_accepted = json["ai_accepted"].as_u64().unwrap_or(0);
    assert_eq!(
        ai_accepted, 1,
        "ai_accepted should count AI lines from old-format attestation (1 line at line 2)"
    );

    // ai_additions should equal ai_accepted (post-PR-1154: no mixed component)
    let ai_additions = json["ai_additions"].as_u64().unwrap_or(0);
    assert_eq!(
        ai_additions, ai_accepted,
        "ai_additions should equal ai_accepted"
    );

    // tool_model_breakdown should still include the old-format prompt's tool::model
    let breakdown = &json["tool_model_breakdown"];
    assert!(
        breakdown.get("cursor::gpt-4o").is_some(),
        "tool_model_breakdown should include old-format prompt's tool::model"
    );
}

// Test 25: git ai status correctly counts AI lines from old-format INITIAL entries.
// When a reset brings old-format prompts into the INITIAL working log, `git ai status`
// should recognize those author_ids as AI (via prompts map lookup) and report them.
#[test]
fn test_status_counts_ai_lines_from_old_format_initial() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("status_test.txt");

    // Step 1: Base commit
    let base = "Base line\n";
    repo.human_edit("status_test.txt", base);
    repo.stage_all_and_commit("Base").unwrap();

    // Step 2: Commit with AI content
    let ai_edit = "Base line\nAI status line\nAnother AI line\n";
    fs::write(&file_path, ai_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "status_test.txt"])
        .unwrap();
    let ai_commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Step 3: Replace with old-format note
    let old_hash = "statustest123456";
    let old_note = format!(
        r#"status_test.txt
  {} 2-3
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "status_agent", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 2,
      "total_deletions": 0,
      "accepted_lines": 2,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, ai_commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &ai_commit.commit_sha, &old_note).expect("attach old-format note");

    // Step 4: Reset --soft to bring content into working log with old-format INITIAL
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();
    repo.sync_daemon_force();

    // Step 5: Run git ai status --json and check it counts AI lines from old-format INITIAL
    let status_output = repo.git_ai(&["status", "--json"]);
    assert!(
        status_output.is_ok(),
        "git ai status should work with old-format INITIAL"
    );
    let output = status_output.unwrap();
    let json: Value = serde_json::from_str(output.trim()).unwrap();

    // The status should report AI lines from the old-format INITIAL
    let ai_accepted = json["stats"]["ai_accepted"].as_u64().unwrap_or(0);
    assert!(
        ai_accepted >= 2,
        "status should count AI lines from old-format INITIAL (got {})",
        ai_accepted
    );
}
