use super::*;

// Test 1: Old format note can be read and deserializes correctly
#[test]
fn test_old_format_note_can_be_attached_and_read() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Human line", "AI line".ai()]);
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Replace with an old-format note (using "cursor" as tool name)
    let old_hash = "5a1b2c3d4e5f6789"; // 16-char bare hex
    let base_sha = &commit.commit_sha;
    let old_note = format!(
        r#"test.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.3.3",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "old_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }}
}}"#,
        old_hash, base_sha, old_hash
    );

    // Attach old-format note
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, base_sha, &old_note).expect("add old-format note");

    // Verify old format note is present and reads correctly
    let read_note = repo
        .read_authorship_note(base_sha)
        .expect("should have note");
    let log =
        AuthorshipLog::deserialize_from_string(&read_note).expect("should deserialize old note");

    // Verify structure
    assert_eq!(log.metadata.prompts.len(), 1, "should have 1 prompt");
    assert_eq!(
        log.metadata.sessions.len(),
        0,
        "should have no sessions (old format)"
    );

    // Verify old prompt metadata
    let prompt = log
        .metadata
        .prompts
        .get(old_hash)
        .expect("old hash should be in prompts");
    assert_eq!(prompt.agent_id.tool, "cursor");
    assert_eq!(prompt.total_additions, 1);
    assert_eq!(prompt.accepted_lines, 1);

    // Verify attestation uses old format
    assert_eq!(log.attestations.len(), 1);
    assert_eq!(log.attestations[0].entries.len(), 1);
    assert_eq!(log.attestations[0].entries[0].hash, old_hash);

    // Verify blame works with old format note
    file.assert_committed_lines(crate::lines!["Human line".human(), "AI line".ai(),]);
}

// Test 2: Note with both old and new format attestations deserializes and blame works
#[test]
fn test_mixed_format_note_with_both_prompts_and_sessions() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Line 1", "Line 2".ai(), "Line 3".ai()]);
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Replace with a mixed-format note that has BOTH prompts and sessions
    let old_hash = "abcd1234ef567890"; // 16-char hex for old format
    // The new hash will be extracted from the original note
    let original_note = repo
        .read_authorship_note(&commit.commit_sha)
        .expect("should have original note");
    let original_log =
        AuthorshipLog::deserialize_from_string(&original_note).expect("parse original note");

    // Get the new-format session ID from the original note
    let new_hash = if !original_log.metadata.sessions.is_empty() {
        original_log
            .metadata
            .sessions
            .keys()
            .next()
            .unwrap()
            .clone()
    } else {
        "s_1234567890abcd".to_string() // fallback
    };

    let mixed_note = format!(
        r#"test.txt
  {} 2-2
  {}::t_fedcba0987654321 3-3
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.3.3",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "old_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
      "overriden_lines": 0
    }}
  }},
  "sessions": {{
    "{}": {{
      "agent_id": {{"tool": "mock_ai", "id": "new_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": []
    }}
  }}
}}"#,
        old_hash, new_hash, commit.commit_sha, old_hash, new_hash
    );

    // Attach mixed-format note
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &commit.commit_sha, &mixed_note).expect("add mixed-format note");

    // Read and verify the note
    let read_note = repo
        .read_authorship_note(&commit.commit_sha)
        .expect("should have note");
    let log = AuthorshipLog::deserialize_from_string(&read_note).expect("should parse note");

    // Verify both prompts and sessions are present
    assert_eq!(log.metadata.prompts.len(), 1, "should have 1 prompt");
    assert_eq!(log.metadata.sessions.len(), 1, "should have 1 session");

    // Verify attestations have both formats
    assert_eq!(log.attestations.len(), 1);
    assert_eq!(
        log.attestations[0].entries.len(),
        2,
        "should have 2 attestation entries"
    );

    let mut has_old_format = false;
    let mut has_new_format = false;
    for entry in &log.attestations[0].entries {
        if entry.hash.len() == 16 && !entry.hash.contains("::") {
            has_old_format = true;
        }
        if entry.hash.contains("::t_") {
            has_new_format = true;
        }
    }
    assert!(has_old_format, "should have old-format attestation");
    assert!(has_new_format, "should have new-format attestation");

    // Verify blame works for both formats
    file.assert_committed_lines(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".ai(),
    ]);
}

// Test 5: Verify that sessions-format is the default for all new operations
// This test documents that the current system produces sessions, not prompts
#[test]
fn test_current_system_produces_sessions_not_prompts() {
    let repo = TestRepo::new();

    // Create commit with AI content using standard helpers
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Line 1", "AI line".ai()]);
    repo.stage_all_and_commit("AI commit").unwrap();

    // Read note
    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo.read_authorship_note(&sha).expect("should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse note");

    // Should have sessions, NOT prompts (this is the new default)
    assert!(
        log.metadata.prompts.is_empty(),
        "new system should not produce prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "new system should produce sessions"
    );

    // Verify attestations use session format (s_::t_)
    let mut has_session_format = false;
    for file_att in &log.attestations {
        for entry in &file_att.entries {
            if entry.hash.starts_with("s_") && entry.hash.contains("::t_") {
                has_session_format = true;
                break;
            }
        }
    }
    assert!(
        has_session_format,
        "attestations should use session format (s_::t_)"
    );

    // Verify blame works
    file.assert_committed_lines(crate::lines!["Line 1".human(), "AI line".ai(),]);
}

// Test 6: Old format note roundtrips through operations without corruption
#[test]
fn test_old_format_note_roundtrips_without_corruption() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Line 1"]);
    let _initial_commit = repo.stage_all_and_commit("Initial").unwrap();

    // Create commit with AI content
    file.set_contents(crate::lines!["Line 1", "AI line".ai()]);
    let ai_commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Replace with genuine old-format note with stats
    let old_hash = "0123456789abcdef";
    let old_note = format!(
        r#"test.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.3.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "roundtrip_tool", "id": "roundtrip_agent", "model": "roundtrip_model"}},
      "human_author": null,
      "messages": [],
      "total_additions": 42,
      "total_deletions": 7,
      "accepted_lines": 35,
      "overriden_lines": 3
    }}
  }},
  "humans": {{
    "h_fedcba9876543210": {{
      "author": "Test User <test@example.com>"
    }}
  }}
}}"#,
        old_hash, ai_commit.commit_sha, old_hash
    );
    let git_ai_repo =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find repository");
    write_note(&git_ai_repo, &ai_commit.commit_sha, &old_note).expect("add old-format note");

    // Read it back
    let note_v1 = repo
        .read_authorship_note(&ai_commit.commit_sha)
        .expect("should have note");
    let log_v1 = AuthorshipLog::deserialize_from_string(&note_v1).expect("parse note v1");

    // Verify structure
    assert_eq!(log_v1.metadata.prompts.len(), 1);
    assert_eq!(log_v1.metadata.sessions.len(), 0);
    assert_eq!(log_v1.metadata.humans.len(), 1);

    // Verify stats preserved
    let prompt_v1 = log_v1
        .metadata
        .prompts
        .get(old_hash)
        .expect("should have old hash");
    assert_eq!(prompt_v1.total_additions, 42);
    assert_eq!(prompt_v1.total_deletions, 7);
    assert_eq!(prompt_v1.accepted_lines, 35);
    assert_eq!(prompt_v1.overriden_lines, 3);

    // Serialize and deserialize again (roundtrip)
    let serialized = log_v1.serialize_to_string().expect("serialize");
    let log_v2 = AuthorshipLog::deserialize_from_string(&serialized).expect("parse note v2");

    // Verify structure unchanged
    assert_eq!(log_v2.metadata.prompts.len(), 1);
    assert_eq!(log_v2.metadata.sessions.len(), 0);
    assert_eq!(log_v2.metadata.humans.len(), 1);

    // Verify stats still preserved
    let prompt_v2 = log_v2
        .metadata
        .prompts
        .get(old_hash)
        .expect("should still have old hash");
    assert_eq!(prompt_v2.total_additions, 42);
    assert_eq!(prompt_v2.total_deletions, 7);
    assert_eq!(prompt_v2.accepted_lines, 35);
    assert_eq!(prompt_v2.overriden_lines, 3);

    // Verify serialized output doesn't contain "sessions" key
    assert!(
        !serialized.contains("\"sessions\""),
        "should not add sessions key to old-format note"
    );
}

// Test 8: Verify that new checkpoints always produce sessions, never prompts
#[test]
fn test_new_checkpoints_always_produce_sessions() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Use the standard helper which calls mock_ai checkpoint
    file.set_contents(crate::lines!["Line 1", "AI line".ai()]);
    repo.stage_all_and_commit("AI commit").unwrap();

    // Read note
    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo.read_authorship_note(&sha).expect("should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse note");

    // Should have sessions, NOT prompts
    assert!(
        log.metadata.prompts.is_empty(),
        "new checkpoints should not produce prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "new checkpoints should produce sessions"
    );

    // Verify session format in attestations (s_::t_)
    let mut has_session_format = false;
    for file_att in &log.attestations {
        for entry in &file_att.entries {
            if entry.hash.starts_with("s_") && entry.hash.contains("::t_") {
                has_session_format = true;
                break;
            }
        }
    }
    assert!(
        has_session_format,
        "attestations should use session format (s_::t_)"
    );
}

// Test 10: Mixed working log where old-format checkpoints (trace_id: null, bare hex author_ids)
// coexist with new-format checkpoints (trace_id: Some, s_::t_ author_ids) in the SAME commit.
// This simulates: user upgrades git-ai mid-session. The working log has checkpoints from before
// the upgrade (no trace_id) and after the upgrade (with trace_id). On commit, old entries should
// go to prompts and new entries should go to sessions.
#[test]
fn test_mixed_working_log_old_and_new_checkpoints_produce_both_prompts_and_sessions() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("mixed.txt");

    // Step 1: Create a base commit (human only)
    let base = "Base line\n";
    repo.human_edit("mixed.txt", base);
    let base_commit = repo.stage_all_and_commit("Base commit").unwrap();

    // Step 2: Make an AI edit using current (new-format) checkpoint
    let edit1 = "Base line\nAI line from old version\n";
    fs::write(&file_path, edit1).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "mixed.txt"])
        .unwrap();

    // Step 3: Manipulate the checkpoints.jsonl to downgrade the FIRST AI checkpoint
    // to old format (remove trace_id, replace s_::t_ author_ids with bare hex)
    let working_log = repo.current_working_logs();
    let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
    assert!(
        checkpoints_file.exists(),
        "checkpoints.jsonl should exist after checkpoint"
    );

    rewrite_checkpoint_journal_as_legacy(&working_log);
    let content = fs::read_to_string(&checkpoints_file).expect("read checkpoints.jsonl");
    let mut modified_lines = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut checkpoint: Value = serde_json::from_str(line).expect("parse checkpoint JSON");

        // Find AI checkpoints and downgrade the first one we find
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
            // Compute the correct old-format author_id from agent_id fields
            // (this is what the old system would have stored)
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

            // Downgrade: remove trace_id, replace s_::t_ author_ids with old-format hash
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

        modified_lines
            .push(serde_json::to_string(&checkpoint).expect("serialize modified checkpoint"));
    }
    let new_content = modified_lines.join("\n") + "\n";
    fs::write(&checkpoints_file, new_content).expect("write modified checkpoints.jsonl");

    // Step 4: Make ANOTHER edit with new-format checkpoint (upgrade happened mid-session)
    let edit2 = "Base line\nAI line from old version\nAI line from new version\n";
    fs::write(&file_path, edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "mixed.txt"])
        .unwrap();

    // Step 5: Commit - this should produce a note with BOTH prompts and sessions
    repo.git(&["add", "."]).unwrap();
    repo.commit("Mixed format commit").unwrap();

    // Step 6: Verify the resulting note
    let commit_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_ne!(commit_sha, base_commit.commit_sha, "should be a new commit");

    let note = repo
        .read_authorship_note(&commit_sha)
        .expect("mixed commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse mixed note");

    // Old-format checkpoint (trace_id: null, bare hex) should produce a prompt
    assert!(
        !log.metadata.prompts.is_empty(),
        "old-format checkpoint (no trace_id) should produce a prompt entry, got: prompts={:?}",
        log.metadata.prompts
    );

    // New-format checkpoint (trace_id: Some, s_::t_) should produce a session
    assert!(
        !log.metadata.sessions.is_empty(),
        "new-format checkpoint (with trace_id) should produce a session entry, got: sessions={:?}",
        log.metadata.sessions
    );

    // Verify attestations have both formats
    let mut has_old_att = false;
    let mut has_new_att = false;
    for file_att in &log.attestations {
        for entry in &file_att.entries {
            if !entry.hash.starts_with("s_")
                && !entry.hash.starts_with("h_")
                && entry.hash.len() == 16
            {
                has_old_att = true;
            }
            if entry.hash.starts_with("s_") && entry.hash.contains("::t_") {
                has_new_att = true;
            }
        }
    }
    assert!(
        has_old_att,
        "attestations should include old-format (bare hex) hash, got: {:?}",
        log.attestations
    );
    assert!(
        has_new_att,
        "attestations should include new-format (s_::t_) hash, got: {:?}",
        log.attestations
    );

    // The old-format prompt key should match an attestation hash (both are generate_short_hash output)
    let prompt_key = log.metadata.prompts.keys().next().unwrap();
    assert_eq!(
        prompt_key.len(),
        16,
        "prompt key should be 16 chars (old format)"
    );
    assert!(
        !prompt_key.starts_with("s_"),
        "prompt key should not have session prefix"
    );

    // Verify blame works correctly for all lines
    let mut file = repo.filename("mixed.txt");
    file.assert_committed_lines(crate::lines![
        "Base line".human(),
        "AI line from old version".ai(),
        "AI line from new version".ai(),
    ]);
}
