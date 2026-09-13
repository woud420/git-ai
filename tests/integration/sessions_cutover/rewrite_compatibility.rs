use super::*;

// Test 3: Rebase chain with old and new format notes
#[test]
fn test_rebase_chain_with_old_and_new_format_notes() {
    let repo = TestRepo::new();

    // Create base commit on main
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["Base line"]);
    repo.stage_all_and_commit("Base commit").unwrap();
    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Commit A with AI content on feature
    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["Human line A", "AI line A".ai()]);
    let commit_a = repo.stage_all_and_commit("Commit A").unwrap();

    // Replace commit A's note with old-format note (using "claude" as tool name)
    let old_hash_a = "1111222233334444";
    let old_note_a = format!(
        r#"file_a.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.3.3",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "claude", "id": "old_agent", "model": "claude-3.5"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash_a, commit_a.commit_sha, old_hash_a
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit_a.commit_sha, &old_note_a).expect("add old-format note A");

    // Commit B with AI content (will use new format naturally)
    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["Human line B", "AI line B".ai()]);
    repo.stage_all_and_commit("Commit B").unwrap();

    // Go back to main, add unrelated commit
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other = repo.filename("other.txt");
    other.set_contents(crate::lines!["Other line"]);
    repo.stage_all_and_commit("Other commit").unwrap();

    // Rebase feature onto main
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Find the two rebased commits (A' and B')
    let log_output = repo
        .git(&["log", "--oneline", "--no-decorate", "-2"])
        .unwrap();
    let lines: Vec<&str> = log_output.trim().lines().collect();
    assert_eq!(lines.len(), 2, "should have 2 commits");

    // Get commit SHAs (most recent first)
    let commit_b_prime_sha = lines[0].split_whitespace().next().unwrap();
    let commit_a_prime_sha = lines[1].split_whitespace().next().unwrap();

    // Verify commit A' still has prompts (old format preserved)
    let note_a_prime = repo
        .read_authorship_note(commit_a_prime_sha)
        .expect("commit A' should have note");
    let log_a_prime = AuthorshipLog::deserialize_from_string(&note_a_prime).expect("parse note A'");
    assert!(
        !log_a_prime.metadata.prompts.is_empty(),
        "commit A' should have prompts"
    );
    assert_eq!(
        log_a_prime.metadata.sessions.len(),
        0,
        "commit A' should not have sessions (old format)"
    );

    // Verify old prompt data preserved
    assert!(
        log_a_prime.metadata.prompts.contains_key(old_hash_a),
        "old hash should be preserved"
    );

    // Verify commit B' still has sessions (new format preserved)
    let note_b_prime = repo
        .read_authorship_note(commit_b_prime_sha)
        .expect("commit B' should have note");
    let log_b_prime = AuthorshipLog::deserialize_from_string(&note_b_prime).expect("parse note B'");
    assert!(
        !log_b_prime.metadata.sessions.is_empty(),
        "commit B' should have sessions (new format)"
    );

    // Verify blame works correctly on both commits
    repo.git(&["checkout", commit_a_prime_sha]).unwrap();
    file_a.assert_committed_lines(crate::lines!["Human line A".human(), "AI line A".ai(),]);

    repo.git(&["checkout", commit_b_prime_sha]).unwrap();
    file_b.assert_committed_lines(crate::lines!["Human line B".human(), "AI line B".ai(),]);
}

// Test 4: Cherry-pick old format note with AI lines preserved
#[test]
fn test_cherry_pick_old_format_note_with_ai_lines_preserved() {
    let repo = TestRepo::new();

    // Create initial commit on main
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["Base line"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    // Create source branch
    repo.git(&["checkout", "-b", "source"]).unwrap();

    // Add AI content and commit
    let mut file = repo.filename("source.txt");
    file.set_contents(crate::lines!["Human line", "AI line".ai()]);
    let source_commit = repo.stage_all_and_commit("Source commit").unwrap();

    // Replace with old-format note INCLUDING attestation (using "copilot" as tool name)
    let old_hash = "9876543210fedcba";
    let old_note = format!(
        r#"source.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.3.3",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "copilot", "id": "cherry_agent", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, source_commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &source_commit.commit_sha, &old_note).expect("add old-format note");

    // Go back to main and cherry-pick
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["cherry-pick", &source_commit.commit_sha])
        .unwrap();

    // Get cherry-picked commit SHA
    let picked_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Verify cherry-picked commit has prompts (not sessions)
    let picked_note = repo
        .read_authorship_note(&picked_sha)
        .expect("cherry-picked commit should have note");
    let picked_log =
        AuthorshipLog::deserialize_from_string(&picked_note).expect("parse cherry-picked note");

    assert!(
        !picked_log.metadata.prompts.is_empty(),
        "cherry-picked commit should have prompts"
    );
    // Note: cherry-pick may add sessions if there are new changes; we primarily care that prompts are preserved
    assert!(
        picked_log.metadata.prompts.contains_key(old_hash),
        "old hash should be preserved in cherry-pick"
    );

    // Verify AI lines correctly attributed
    file.assert_committed_lines(crate::lines!["Human line".human(), "AI line".ai(),]);
}

// Test 7: Reset with old format notes
#[test]
fn test_reset_preserves_old_format_notes_in_working_log() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    // Create initial commit
    fs::write(&file_path, "Line 1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.stage_all_and_commit("Initial").unwrap();

    // Create commit with AI content
    fs::write(&file_path, "Line 1\nAI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Replace with old-format note (using "windsurf" as tool name)
    let old_hash = "aabbccddeeff1122";
    let human_hash = "h_resetoldhuman";
    let old_note = format!(
        r#"test.txt
  {} 1-1
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.3.3",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "windsurf", "id": "reset_agent", "model": "claude-3.5"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }},
  "humans": {{
    "{}": {{
      "author": "Test User <test@example.com>"
    }}
  }}
}}"#,
        human_hash, old_hash, commit.commit_sha, old_hash, human_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("add old-format note");

    // Reset --soft to un-commit but keep changes staged
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();

    // Re-commit
    repo.commit("Recommit").unwrap();

    // Verify note is preserved with prompts
    let new_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let new_note = repo
        .read_authorship_note(&new_sha)
        .expect("should have note after reset");
    let new_log = AuthorshipLog::deserialize_from_string(&new_note).expect("parse note");

    // Should have prompts from the old format note
    assert!(
        !new_log.metadata.prompts.is_empty(),
        "should preserve prompts after reset"
    );

    // Verify AI attribution still works
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines!["Line 1".human(), "AI line".ai(),]);
}

// Test 12: Squash merge a feature branch where some commits have old-format notes
// and others have new-format notes. The squashed commit must contain BOTH prompts and sessions.
#[test]
fn test_squash_merge_mixed_format_commits() {
    let repo = TestRepo::new();

    // Step 1: Create base commit on main
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["Base line"]);
    repo.stage_all_and_commit("Base commit").unwrap();
    let default_branch = repo.current_branch();

    // Step 2: Create feature branch
    repo.git(&["checkout", "-b", "feature-mixed"]).unwrap();

    // Step 3: Commit C1 with AI content, then replace with old-format note
    let mut file_a = repo.filename("feature_a.txt");
    file_a.set_contents(crate::lines!["Human A", "AI A".ai()]);
    let commit_a = repo.stage_all_and_commit("Feature commit A").unwrap();

    let old_hash = "aaaa1111bbbb2222";
    let old_note = format!(
        r#"feature_a.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "windsurf", "id": "old_squash_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, commit_a.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit_a.commit_sha, &old_note).expect("attach old-format note");

    // Step 4: Commit C2 with AI content using standard helpers (produces new-format/sessions)
    let mut file_b = repo.filename("feature_b.txt");
    file_b.set_contents(crate::lines!["Human B", "AI B".ai()]);
    repo.stage_all_and_commit("Feature commit B").unwrap();

    // Step 5: Switch to main, squash merge
    repo.git(&["checkout", &default_branch]).unwrap();
    repo.git(&["merge", "--squash", "feature-mixed"]).unwrap();
    repo.commit("Squash merge mixed formats").unwrap();

    // Step 6: Verify squashed commit note has BOTH prompts and sessions
    let squash_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&squash_sha)
        .expect("squash commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse squash note");

    // C1's old-format prompts should be preserved
    assert!(
        !log.metadata.prompts.is_empty(),
        "squash note should have prompts from old-format commit C1"
    );

    // C2's new-format sessions should be present
    assert!(
        !log.metadata.sessions.is_empty(),
        "squash note should have sessions from new-format commit C2"
    );

    // Verify both file attestations work for blame
    file_a.assert_committed_lines(crate::lines!["Human A".human(), "AI A".ai(),]);
    file_b.assert_committed_lines(crate::lines!["Human B".human(), "AI B".ai(),]);
}

// Test 13: Stash and pop a mixed-format working log.
// The working log has old-format checkpoints (downgraded, no trace_id) + new-format checkpoints.
// After stash push + pop + commit, the note should have BOTH prompts and sessions.
#[test]
fn test_stash_pop_mixed_format_working_log() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("stash_test.txt");

    // Step 1: Create base commit
    let base = "Base line\n";
    repo.human_edit("stash_test.txt", base);
    repo.stage_all_and_commit("Base commit").unwrap();

    // Step 2: Make an AI edit (new format checkpoint)
    let edit1 = "Base line\nAI old line\n";
    fs::write(&file_path, edit1).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "stash_test.txt"])
        .unwrap();

    // Step 3: Downgrade that checkpoint to old format
    let working_log = repo.current_working_logs();
    let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
    assert!(checkpoints_file.exists(), "checkpoints.jsonl should exist");

    rewrite_checkpoint_journal_as_legacy(&working_log);
    let content = fs::read_to_string(&checkpoints_file).expect("read checkpoints");
    let mut modified_lines = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut checkpoint: Value = serde_json::from_str(line).expect("parse checkpoint");
        let kind = checkpoint
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("");
        if kind == "AiAgent"
            && checkpoint
                .get("trace_id")
                .and_then(|t| t.as_str())
                .is_some()
        {
            let agent_tool = checkpoint
                .get("agent_id")
                .and_then(|a| a.get("tool"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let agent_id_str = checkpoint
                .get("agent_id")
                .and_then(|a| a.get("id"))
                .and_then(|i| i.as_str())
                .unwrap_or("");
            let old_author_id = git_ai::model::authorship_log_serialization::generate_short_hash(
                agent_id_str,
                agent_tool,
            );
            checkpoint["trace_id"] = Value::Null;
            if let Some(entries) = checkpoint.get_mut("entries").and_then(|e| e.as_array_mut()) {
                for entry in entries {
                    if let Some(attributions) =
                        entry.get_mut("attributions").and_then(|a| a.as_array_mut())
                    {
                        for attr in attributions {
                            if let Some(author_id) =
                                attr.get("author_id").and_then(|id| id.as_str())
                                && author_id.starts_with("s_")
                            {
                                attr["author_id"] = Value::String(old_author_id.clone());
                            }
                        }
                    }
                    if let Some(line_attrs) = entry
                        .get_mut("line_attributions")
                        .and_then(|a| a.as_array_mut())
                    {
                        for line_attr in line_attrs {
                            if let Some(author_id) =
                                line_attr.get("author_id").and_then(|id| id.as_str())
                                && author_id.starts_with("s_")
                            {
                                line_attr["author_id"] = Value::String(old_author_id.clone());
                            }
                        }
                    }
                }
            }
        }
        checkpoint
            .as_object_mut()
            .expect("checkpoint should be an object")
            .remove("_git_ai_record_version");
        checkpoint
            .as_object_mut()
            .expect("checkpoint should be an object")
            .remove("_git_ai_record_checksum");
        modified_lines.push(serde_json::to_string(&checkpoint).expect("serialize"));
    }
    fs::write(&checkpoints_file, modified_lines.join("\n") + "\n").expect("write");

    // Step 4: Make another AI edit (new format)
    let edit2 = "Base line\nAI old line\nAI new line\n";
    fs::write(&file_path, edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "stash_test.txt"])
        .unwrap();

    // Step 5: Stash
    repo.git(&["stash", "push", "-u"]).unwrap();

    // Step 6: Pop
    repo.git(&["stash", "pop"]).unwrap();

    // Step 7: Commit
    repo.git(&["add", "."]).unwrap();
    repo.commit("After stash pop").unwrap();

    // Step 8: Verify the note
    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&sha)
        .expect("post-stash commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse note");

    // Old-format checkpoint should produce prompt
    assert!(
        !log.metadata.prompts.is_empty(),
        "post-stash note should have prompts from old-format checkpoint"
    );

    // New-format checkpoint should produce session
    assert!(
        !log.metadata.sessions.is_empty(),
        "post-stash note should have sessions from new-format checkpoint"
    );

    // Verify blame
    let mut file = repo.filename("stash_test.txt");
    file.assert_committed_lines(crate::lines![
        "Base line".human(),
        "AI old line".ai(),
        "AI new line".ai(),
    ]);
}

// Test 14: Rebase a commit with old-format note that conflicts, AI resolves the conflict
// (producing new session-format checkpoints). The resulting note should have sessions from
// the conflict resolution. This documents that build_note_from_conflict_wl uses the
// working log's format, not the original commit's note format.
#[test]
fn test_rebase_conflict_old_note_ai_resolves_with_sessions() {
    let repo = TestRepo::new_with_daemon_scope(crate::repos::test_repo::DaemonTestScope::Dedicated);

    // Step 1: Create base commit
    let mut file = repo.filename("conflict.txt");
    file.set_contents(crate::lines!["Original line"]);
    repo.stage_all_and_commit("Base commit").unwrap();
    let default_branch = repo.current_branch();

    // Step 2: Create feature branch with AI content
    repo.git(&["checkout", "-b", "feature-conflict"]).unwrap();
    file.set_contents(crate::lines!["Original line", "AI feature line".ai()]);
    let feature_commit = repo.stage_all_and_commit("Feature commit").unwrap();

    // Replace with old-format note
    let old_hash = "cccc3333dddd4444";
    let old_note = format!(
        r#"conflict.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "claude", "id": "old_rebase_session", "model": "claude-3.5"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, feature_commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &feature_commit.commit_sha, &old_note)
        .expect("attach old-format note");

    // Step 3: Go back to main, make conflicting change
    repo.git(&["checkout", &default_branch]).unwrap();
    file.set_contents(crate::lines!["Modified base line"]);
    repo.stage_all_and_commit("Conflicting commit on main")
        .unwrap();

    // Step 4: Rebase feature onto main (will conflict)
    repo.git(&["checkout", "feature-conflict"]).unwrap();
    let rebase_result = repo.git(&["rebase", &default_branch]);
    assert!(rebase_result.is_err(), "rebase should conflict");

    // Step 5: AI resolves the conflict by writing the merged file and checkpointing
    // (using set_contents which calls checkpoint mock_ai + stages the file)
    file.set_contents(crate::lines![
        "Modified base line".human(),
        "AI resolved line".ai()
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    // Step 6: Verify the rebased commit's note
    repo.sync_daemon_force();
    let rebased_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&rebased_sha)
        .expect("rebased commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse rebased note");

    // The conflict was re-resolved from scratch, so build_note_from_conflict_wl
    // creates the note from the conflict working log. The AI checkpoint used new-format
    // (with trace_id), so the result should have sessions.
    assert!(
        !log.metadata.sessions.is_empty(),
        "rebased note should have sessions from AI conflict resolution checkpoint"
    );

    // Verify AI attribution on the resolved line
    file.assert_committed_lines(crate::lines![
        "Modified base line".human(),
        "AI resolved line".ai(),
    ]);
}
