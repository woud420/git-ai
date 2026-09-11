use super::{ExpectedLineExt, TestRepo, Value, fs, write_note};

// Test 18: git-ai diff --json --all-prompts with new-format (sessions-only) commit
// includes sessions in the dedicated "sessions" output key
#[test]
fn test_diff_json_all_prompts_includes_sessions() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create second commit with AI content (produces sessions, not prompts)
    file.set_contents(crate::lines!["Line 1", "AI line".ai()]);
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Run git-ai diff --json --all-prompts
    let output = repo
        .git_ai(&["diff", "--json", "--all-prompts", &commit.commit_sha])
        .expect("diff --json --all-prompts should work");
    let json: Value = serde_json::from_str(output.trim()).unwrap();

    // Sessions appear in the dedicated "sessions" key, not in "prompts"
    let sessions = json["sessions"].as_object();
    assert!(
        sessions.is_some() && !sessions.unwrap().is_empty(),
        "diff --json --all-prompts should include sessions in 'sessions' key, got: {:?}",
        json["sessions"]
    );

    // prompts should be empty for new-format-only commits
    let prompts = json["prompts"].as_object();
    assert!(
        prompts.is_none() || prompts.unwrap().is_empty(),
        "new-format commit should not have entries in 'prompts'"
    );

    // Session keys should use s_xxx format (session ID only, not combined with trace ID)
    let first_key = sessions.unwrap().keys().next().unwrap();
    assert!(
        first_key.starts_with("s_") && !first_key.contains("::"),
        "session key should be session ID only (s_xxx), not combined ID (s_xxx::t_yyy), got: {}",
        first_key
    );

    // The session should have mock_ai as the tool
    let first_session = sessions.unwrap().values().next().unwrap();
    assert_eq!(
        first_session["agent_id"]["tool"].as_str(),
        Some("mock_ai"),
        "session should have correct agent_id"
    );
}

// Test 19: git-ai diff --json --all-prompts with old-format (prompts-only) commit
// includes old prompts in the output
#[test]
fn test_diff_json_all_prompts_includes_old_format_prompts() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create second commit with AI content
    file.set_contents(crate::lines!["Line 1", "AI line".ai()]);
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Replace with old-format note
    let old_hash = "5566778899aabbcc";
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
      "agent_id": {{"tool": "copilot", "id": "diff_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 5,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // Run git-ai diff --json --all-prompts
    let output = repo
        .git_ai(&["diff", "--json", "--all-prompts", &commit.commit_sha])
        .expect("diff --json --all-prompts should work");
    let json: Value = serde_json::from_str(output.trim()).unwrap();

    // The prompts should include the old-format prompt
    let prompts = json["prompts"].as_object();
    assert!(
        prompts.is_some() && !prompts.unwrap().is_empty(),
        "diff --json --all-prompts should include old-format prompts"
    );
    assert!(
        prompts.unwrap().contains_key(old_hash),
        "old prompt hash should be present in diff --all-prompts output"
    );
}

// Test 26: git-ai diff --json on a commit that has BOTH old prompts and new sessions
// (e.g. after amending an old-format commit with new-format checkpoints).
// Verifies that prompts go to "prompts", sessions go to "sessions", and stats
// correctly aggregate both.
#[test]
fn test_diff_json_mixed_format_commit_separates_prompts_and_sessions() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("mixed_diff.txt");

    // Step 1: Create base commit
    fs::write(&file_path, "base line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "mixed_diff.txt"])
        .unwrap();
    repo.stage_all_and_commit("Base commit").unwrap();

    // Step 2: Add AI content and commit
    fs::write(&file_path, "base line\nold ai line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "mixed_diff.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Step 3: Replace note with old-format (simulating pre-upgrade git-ai)
    let old_hash = "oldfmt1234567890";
    let old_note = format!(
        r#"mixed_diff.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "old_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 5,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // Step 4: Amend with new-format AI content
    fs::write(&file_path, "base line\nold ai line\nnew ai line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "mixed_diff.txt"])
        .unwrap();
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "--amend", "--no-edit"]).unwrap();

    // Step 5: Run diff --json --include-stats on the amended commit
    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let output = repo
        .git_ai(&["diff", &sha, "--json", "--include-stats"])
        .expect("diff --json --include-stats should succeed");
    let json: Value = serde_json::from_str(output.trim()).expect("diff JSON should parse");

    // Verify prompts contains old-format entry
    let prompts = json["prompts"]
        .as_object()
        .expect("prompts should be object");
    assert!(
        prompts.contains_key(old_hash),
        "prompts should contain old-format hash '{}', got keys: {:?}",
        old_hash,
        prompts.keys().collect::<Vec<_>>()
    );
    let old_prompt = &prompts[old_hash];
    assert_eq!(old_prompt["agent_id"]["tool"].as_str(), Some("cursor"));
    assert_eq!(old_prompt["agent_id"]["model"].as_str(), Some("gpt-4"));

    // Verify sessions contains new-format entry with s_::t_ key
    let sessions = json["sessions"]
        .as_object()
        .expect("sessions should be object");
    assert!(
        !sessions.is_empty(),
        "sessions should contain new-format entries"
    );
    let session_key = sessions.keys().next().unwrap();
    assert!(
        session_key.starts_with("s_") && !session_key.contains("::"),
        "session key should be session ID only (s_xxx), not combined ID (s_xxx::t_yyy), got: {}",
        session_key
    );
    let session = &sessions[session_key];
    assert_eq!(session["agent_id"]["tool"].as_str(), Some("mock_ai"));

    // Verify stats aggregate both formats
    let stats = json["commit_stats"]
        .as_object()
        .expect("commit_stats should exist");
    let ai_lines_added = stats["ai_lines_added"].as_u64().unwrap();
    assert_eq!(
        ai_lines_added, 2,
        "should count AI lines from both old prompt (1) and new session (1)"
    );

    // Verify tool_model_breakdown has entries from both formats
    let breakdown = stats["tool_model_breakdown"].as_object().unwrap();
    assert!(
        breakdown.contains_key("cursor::gpt-4"),
        "breakdown should have old-format tool::model, got: {:?}",
        breakdown.keys().collect::<Vec<_>>()
    );
    assert!(
        breakdown.contains_key("mock_ai::unknown"),
        "breakdown should have new-format tool::model, got: {:?}",
        breakdown.keys().collect::<Vec<_>>()
    );
}

// Test 27: git-ai diff --json across a history that has both old-format and new-format commits.
// Uses --all-prompts on each individual commit to verify correct routing, then also checks
// a plain range diff includes hunks from both.
#[test]
fn test_diff_json_history_with_mixed_old_and_new_format_commits() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("range_mixed.txt");

    // Step 1: Base commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "range_mixed.txt"])
        .unwrap();
    let base = repo.stage_all_and_commit("Base").unwrap();

    // Step 2: Old-format AI commit
    fs::write(&file_path, "base\nold ai line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "range_mixed.txt"])
        .unwrap();
    let old_commit = repo.stage_all_and_commit("Old AI commit").unwrap();

    // Replace note with old-format
    let old_hash = "rangemix123456ab";
    let old_note = format!(
        r#"range_mixed.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "windsurf", "id": "old_range", "model": "claude-3.5"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, base.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &old_commit.commit_sha, &old_note).expect("attach old-format note");

    // Step 3: New-format AI commit
    fs::write(&file_path, "base\nold ai line\nnew session line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "range_mixed.txt"])
        .unwrap();
    let new_commit = repo.stage_all_and_commit("New AI commit").unwrap();

    // Verify old-format commit individually: prompts populated, sessions empty
    let old_json: Value = {
        let output = repo
            .git_ai(&["diff", &old_commit.commit_sha, "--json", "--all-prompts"])
            .expect("diff old commit should succeed");
        serde_json::from_str(output.trim()).unwrap()
    };
    let old_prompts = old_json["prompts"].as_object().expect("prompts obj");
    assert!(
        old_prompts.contains_key(old_hash),
        "old commit should have prompt hash '{}', got: {:?}",
        old_hash,
        old_prompts.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        old_prompts[old_hash]["agent_id"]["tool"].as_str(),
        Some("windsurf")
    );
    assert!(
        old_json["sessions"]
            .as_object()
            .is_none_or(|s| s.is_empty()),
        "old commit should have no sessions"
    );

    // Verify new-format commit individually: sessions populated, prompts empty
    let new_json: Value = {
        let output = repo
            .git_ai(&["diff", &new_commit.commit_sha, "--json", "--all-prompts"])
            .expect("diff new commit should succeed");
        serde_json::from_str(output.trim()).unwrap()
    };
    let new_sessions = new_json["sessions"].as_object().expect("sessions obj");
    assert!(
        !new_sessions.is_empty(),
        "new commit should have session entries"
    );
    let session_key = new_sessions.keys().next().unwrap();
    assert!(
        session_key.starts_with("s_") && !session_key.contains("::"),
        "session key should be session ID only (s_xxx), not combined ID (s_xxx::t_yyy), got: {}",
        session_key
    );
    assert!(
        new_json["prompts"].as_object().is_none_or(|p| p.is_empty()),
        "new commit should have no prompts"
    );

    // Verify plain range diff (no --all-prompts, no --include-stats) includes hunks from both
    let range = format!("{}..{}", base.commit_sha, new_commit.commit_sha);
    let range_json: Value = {
        let output = repo
            .git_ai(&["diff", &range, "--json"])
            .expect("range diff should succeed");
        serde_json::from_str(output.trim()).unwrap()
    };
    let commits = range_json["commits"].as_object().expect("commits obj");
    assert!(
        commits.len() >= 2,
        "range should span at least 2 commits, got {}",
        commits.len()
    );
    let hunks = range_json["hunks"].as_array().expect("hunks array");
    assert!(!hunks.is_empty(), "range should have hunks");
}

// Test 28: git-ai diff --json --include-stats on single commit with old-format note
// verifies stats still work correctly (tool_model_breakdown uses old prompt's tool::model)
#[test]
fn test_diff_json_stats_with_old_format_note_only() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("old_stats.txt");

    // Base commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "old_stats.txt"])
        .unwrap();
    let base = repo.stage_all_and_commit("Base").unwrap();

    // AI commit
    fs::write(&file_path, "base\nai one\nai two\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "old_stats.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Replace with old-format note (2 AI lines)
    let old_hash = "oldstats12345678";
    let old_note = format!(
        r#"old_stats.txt
  {} 2-3
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "copilot", "id": "stats_test", "model": "gpt-4o"}},
      "human_author": null,
      "messages": [],
      "total_additions": 10,
      "total_deletions": 3,
      "accepted_lines": 2,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, base.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // Run diff --json --include-stats
    let output = repo
        .git_ai(&["diff", &commit.commit_sha, "--json", "--include-stats"])
        .expect("diff --json --include-stats should succeed");
    let json: Value = serde_json::from_str(output.trim()).expect("diff JSON should parse");

    // Stats should reflect attestation-based data (2 AI lines), not checkpoint stats
    let stats = json["commit_stats"]
        .as_object()
        .expect("commit_stats should exist");
    assert_eq!(
        stats["ai_lines_added"].as_u64().unwrap(),
        2,
        "ai_lines_added should be 2 (from attestation line ranges, not total_additions=10)"
    );
    assert_eq!(
        stats["human_lines_added"].as_u64().unwrap(),
        0,
        "no human lines"
    );

    // tool_model_breakdown should use old-format prompt's tool::model
    let breakdown = stats["tool_model_breakdown"].as_object().unwrap();
    assert!(
        breakdown.contains_key("copilot::gpt-4o"),
        "breakdown should have copilot::gpt-4o from old-format prompt, got: {:?}",
        breakdown.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        breakdown["copilot::gpt-4o"]["ai_lines_added"]
            .as_u64()
            .unwrap(),
        2,
        "copilot::gpt-4o should have 2 AI lines"
    );

    // Prompts should be in prompts key, sessions should be empty
    let prompts = json["prompts"]
        .as_object()
        .expect("prompts should be object");
    assert!(
        prompts.contains_key(old_hash),
        "prompts should have old-format hash"
    );
    assert!(
        json["sessions"].as_object().is_none_or(|s| s.is_empty()),
        "sessions should be empty for old-format-only commit"
    );
}
