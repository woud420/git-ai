use super::{TestRepo, commit_as_agent, commit_as_human, extract_json};

// =============================================================================
// JSON output format: verify prompts are included for agent commits
// =============================================================================

#[test]
fn test_agent_blame_json_output() {
    let repo = TestRepo::new();

    let commit_sha = commit_as_agent(
        &repo,
        "json_test.rs",
        "fn hello() {}\nfn world() {}\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "cursor commit",
    );

    let output = repo.git_ai(&["blame", "--json", "json_test.rs"]).unwrap();

    // Should be valid JSON (strip any trailing migration log lines)
    let json_str = extract_json(&output);
    let json: serde_json::Value =
        serde_json::from_str(json_str).expect("Output should be valid JSON");

    // Lines should be present
    assert!(json["lines"].is_object(), "Should have lines object");

    // Prompts should contain at least one entry for the simulated agent prompt
    assert!(json["prompts"].is_object(), "Should have prompts object");

    // The prompts should not be empty (simulated agent prompt should be present)
    let prompts = json["prompts"].as_object().unwrap();
    assert!(
        !prompts.is_empty(),
        "Prompts should contain simulated agent prompt data, got empty. Full JSON: {}",
        output
    );

    // Verify the prompt record contains the correct tool
    let prompt_entry = prompts.values().next().unwrap();
    assert_eq!(
        prompt_entry["agent_id"]["tool"].as_str().unwrap(),
        "cursor-agent",
        "Prompt should have tool=cursor-agent"
    );
    assert_eq!(
        prompt_entry["agent_id"]["model"].as_str().unwrap(),
        "unknown",
        "Prompt should have model=unknown"
    );

    // Verify the agent_id.id is the commit SHA
    assert_eq!(
        prompt_entry["agent_id"]["id"].as_str().unwrap(),
        commit_sha,
        "Prompt agent_id.id should be the commit SHA"
    );
}

#[test]
fn test_agent_blame_json_mixed_human_agent() {
    let repo = TestRepo::new();

    // Human commit
    commit_as_human(&repo, "json_mixed.rs", "human line\n", "human commit");

    // Agent commit
    commit_as_agent(
        &repo,
        "json_mixed.rs",
        "human line\nagent line\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "cursor commit",
    );

    let output = repo.git_ai(&["blame", "--json", "json_mixed.rs"]).unwrap();
    let json: serde_json::Value = serde_json::from_str(extract_json(&output)).expect("Valid JSON");

    let lines = json["lines"].as_object().unwrap();
    let prompts = json["prompts"].as_object().unwrap();

    // In JSON mode, line_authors uses prompt hashes as names.
    // AI-authored lines map to a prompt hash that exists in prompts;
    // human-authored lines map to the human author name (not in prompts).

    // Find line 2's value: it may be keyed as "2", "2:2", or part of a range like "1-2"
    let mut line2_prompt_hash: Option<String> = None;
    for (range_key, val) in lines {
        let val_str = val.as_str().unwrap_or("");
        // Check if this range covers line 2
        if range_key == "2" || range_key == "2:2" {
            line2_prompt_hash = Some(val_str.to_string());
        } else if range_key.contains('-') {
            let parts: Vec<&str> = range_key.split('-').collect();
            if parts.len() == 2
                && let (Ok(start), Ok(end)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                && start <= 2
                && end >= 2
            {
                line2_prompt_hash = Some(val_str.to_string());
            }
        }
    }

    let line2_hash = line2_prompt_hash.expect("Line 2 should be present in lines map");

    // The hash should be a key in prompts (indicating AI authorship)
    assert!(
        prompts.contains_key(&line2_hash),
        "Line 2's value '{}' should be a prompt hash in prompts, prompts keys: {:?}",
        line2_hash,
        prompts.keys().collect::<Vec<_>>()
    );

    // The prompt should have tool=cursor-agent
    let prompt = &prompts[&line2_hash];
    assert_eq!(
        prompt["agent_id"]["tool"].as_str().unwrap(),
        "cursor-agent",
        "Prompt should have tool=cursor-agent"
    );
}

// =============================================================================
// Porcelain output format
// =============================================================================

#[test]
fn test_agent_blame_porcelain_output() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "porcelain_test.rs",
        "agent line 1\nagent line 2\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "cursor commit",
    );

    let output = repo
        .git_ai(&["blame", "--porcelain", "porcelain_test.rs"])
        .unwrap();

    // Porcelain output should contain author fields with the tool name
    assert!(
        output.contains("author cursor"),
        "Porcelain should show 'author cursor', got:\n{}",
        output
    );
}

#[test]
fn test_agent_blame_line_porcelain_output() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "line_porcelain_test.rs",
        "line 1\nline 2\n",
        "Claude",
        "noreply@anthropic.com",
        "claude commit",
    );

    let output = repo
        .git_ai(&["blame", "--line-porcelain", "line_porcelain_test.rs"])
        .unwrap();

    // Each line should have an author entry
    let author_count = output.matches("author claude").count();
    assert!(
        author_count >= 2,
        "Line porcelain should have 'author claude' for each line, found {} occurrences",
        author_count
    );
}

// =============================================================================
// Verify stats in JSON output for simulated agent authorship
// =============================================================================

#[test]
fn test_agent_blame_json_stats() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "stats_test.rs",
        "line 1\nline 2\nline 3\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "cursor commit",
    );

    let output = repo.git_ai(&["blame", "--json", "stats_test.rs"]).unwrap();
    let json: serde_json::Value = serde_json::from_str(extract_json(&output)).expect("Valid JSON");

    let prompts = json["prompts"].as_object().unwrap();
    assert!(!prompts.is_empty(), "Should have simulated prompt");

    let prompt = prompts.values().next().unwrap();

    // Verify simulated stats
    assert_eq!(
        prompt["accepted_lines"].as_u64().unwrap(),
        3,
        "accepted_lines should equal total lines in commit for this file"
    );
    assert_eq!(
        prompt["total_additions"].as_u64().unwrap(),
        3,
        "total_additions should equal total lines"
    );
    assert_eq!(
        prompt["overriden_lines"].as_u64().unwrap(),
        0,
        "overriden_lines should be 0 (simulated)"
    );
    assert_eq!(
        prompt["total_deletions"].as_u64().unwrap(),
        0,
        "total_deletions should be 0 (simulated)"
    );
}

// =============================================================================
// Incremental output format
// =============================================================================

#[test]
fn test_agent_blame_incremental_output() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "incremental_test.rs",
        "line 1\nline 2\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "cursor commit",
    );

    let output = repo
        .git_ai(&["blame", "--incremental", "incremental_test.rs"])
        .unwrap();

    // Incremental format should contain author with the tool name
    assert!(
        output.contains("author cursor"),
        "Incremental output should contain 'author cursor', got:\n{}",
        output
    );
}
