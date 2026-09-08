use super::{AuthorshipLog, ExpectedLineExt, TestRepo, fs, write_note};

// Test 20: Amend a commit with old-format prompts where the user DELETES the AI line.
// The pruning logic should remove the now-unreferenced prompt from metadata.
// Then the user adds a NEW AI line (session-format). The result should have only sessions.
#[test]
fn test_amend_old_prompts_delete_ai_line_then_add_new_session_line() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("prune.txt");

    // Step 1: Create initial commit with known-human context and AI content
    fs::write(&file_path, "Human line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "prune.txt"])
        .unwrap();
    let initial = "Human line\nOld AI line\n";
    fs::write(&file_path, initial).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "prune.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Step 2: Replace note with old-format
    let old_hash = "prunetest1234567";
    let human_hash = "h_pruneoldhuman";
    let old_note = format!(
        r#"prune.txt
  {} 1-1
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "prune_agent", "model": "gpt-4"}},
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
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // Step 3: Delete the old AI line and add a new one with new-format checkpoint
    let edited = "Human line\nNew session AI line\n";
    fs::write(&file_path, edited).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "prune.txt"])
        .unwrap();

    // Step 4: Amend
    repo.git(&["add", "."]).unwrap();
    repo.git(&[
        "commit",
        "--amend",
        "-m",
        "Amended: deleted old AI, added new",
    ])
    .unwrap();

    // Step 5: Verify
    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&amended_sha)
        .expect("amended commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse amended note");

    // The old prompt should be PRUNED because its referenced line (line 2) was deleted
    assert!(
        !log.metadata.prompts.contains_key(old_hash),
        "old prompt should be pruned when its AI line is deleted during amend"
    );

    // The new checkpoint should produce a session
    assert!(
        !log.metadata.sessions.is_empty(),
        "new AI line should produce a session entry"
    );

    // Verify attestations only have new-format
    let mut has_old_att = false;
    let mut has_new_att = false;
    for file_att in &log.attestations {
        for entry in &file_att.entries {
            if entry.hash == old_hash {
                has_old_att = true;
            }
            if entry.hash.starts_with("s_") && entry.hash.contains("::t_") {
                has_new_att = true;
            }
        }
    }
    assert!(
        !has_old_att,
        "old attestation hash should be removed when line is deleted"
    );
    assert!(has_new_att, "new session attestation should be present");

    // Verify blame
    let mut file = repo.filename("prune.txt");
    file.assert_committed_lines(crate::lines![
        "Human line".human(),
        "New session AI line".ai(),
    ]);
}

// Test 21: Amend a commit with old-format prompts, KEEPING the old AI line
// and adding a new AI line in the SAME file. Both old prompt AND new session
// must be present in the final note, with correct per-line attribution.
#[test]
fn test_amend_old_prompts_keep_old_line_add_new_session_same_file() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("keepold.txt");

    // Step 1: Create initial commit with known-human context and AI content
    fs::write(&file_path, "Human line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "keepold.txt"])
        .unwrap();
    let initial = "Human line\nOld AI line\n";
    fs::write(&file_path, initial).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "keepold.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Step 2: Replace note with old-format
    let old_hash = "keepoldtest12345";
    let human_hash = "h_keepoldhuman";
    let old_note = format!(
        r#"keepold.txt
  {} 1-1
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "windsurf", "id": "keep_agent", "model": "claude-3.5"}},
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
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // Step 3: Add a new line at the end (keep existing content) with new-format checkpoint
    let edited = "Human line\nOld AI line\nNew session AI line\n";
    fs::write(&file_path, edited).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "keepold.txt"])
        .unwrap();

    // Step 4: Amend
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Amended: kept old, added new"])
        .unwrap();

    // Step 5: Verify
    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&amended_sha)
        .expect("amended commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse amended note");

    // Old prompt should be preserved (its line is still there)
    assert!(
        log.metadata.prompts.contains_key(old_hash),
        "old prompt should be preserved when its AI line still exists"
    );

    // New session should also be present
    assert!(
        !log.metadata.sessions.is_empty(),
        "new AI line should produce a session entry"
    );

    // Verify attestations have BOTH formats for different lines
    let mut old_att_lines: Vec<String> = Vec::new();
    let mut new_att_lines: Vec<String> = Vec::new();
    for file_att in &log.attestations {
        for entry in &file_att.entries {
            if entry.hash == old_hash {
                old_att_lines.push(format!("{:?}", entry.line_ranges));
            }
            if entry.hash.starts_with("s_") && entry.hash.contains("::t_") {
                new_att_lines.push(format!("{:?}", entry.line_ranges));
            }
        }
    }
    assert!(
        !old_att_lines.is_empty(),
        "old-format attestation should still reference old AI line"
    );
    assert!(
        !new_att_lines.is_empty(),
        "new-format attestation should reference new AI line"
    );

    // Verify blame shows both lines as AI
    let mut file = repo.filename("keepold.txt");
    file.assert_committed_lines(crate::lines![
        "Human line".human(),
        "Old AI line".ai(),
        "New session AI line".ai(),
    ]);
}

// Test 24: Amend with old-format prompts where a DIFFERENT file gets new session edits.
// Tests cross-file mixed format: file A has old prompt attestation, file B has new session attestation.
#[test]
fn test_amend_old_prompts_different_file_gets_session_edits() {
    let repo = TestRepo::new();
    let file_a = repo.path().join("file_a.txt");
    let file_b = repo.path().join("file_b.txt");

    // Step 1: Initial commit with known-human context and AI content in file_a
    fs::write(&file_a, "Human line A\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "file_a.txt"])
        .unwrap();
    let initial_a = "Human line A\nOld AI line A\n";
    fs::write(&file_a, initial_a).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_a.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Step 2: Replace with old-format note for file_a
    let old_hash = "crossfile1234567";
    let human_hash = "h_crossfilehuman";
    let old_note = format!(
        r#"file_a.txt
  {} 1-1
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "copilot", "id": "cross_agent", "model": "gpt-4"}},
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
    write_note(&git_ai_repo, &commit.commit_sha, &old_note).expect("attach old-format note");

    // Step 3: Create file_b with new session AI content (different file, not in original commit)
    let content_b = "New session AI line B\n";
    fs::write(&file_b, content_b).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_b.txt"])
        .unwrap();

    // Step 4: Amend to include file_b
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Amended: added file_b"])
        .unwrap();

    // Step 5: Verify
    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&amended_sha)
        .expect("amended commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse amended note");

    // Old prompt from file_a should be preserved
    assert!(
        log.metadata.prompts.contains_key(old_hash),
        "old prompt for file_a should be preserved in cross-file amend"
    );

    // New session from file_b should be present
    assert!(
        !log.metadata.sessions.is_empty(),
        "new session for file_b should be present"
    );

    // Verify attestations reference both files correctly
    let mut file_a_has_old = false;
    let mut file_b_has_new = false;
    for file_att in &log.attestations {
        let path = &file_att.file_path;
        for entry in &file_att.entries {
            if path == "file_a.txt" && entry.hash == old_hash {
                file_a_has_old = true;
            }
            if path == "file_b.txt" && entry.hash.starts_with("s_") && entry.hash.contains("::t_") {
                file_b_has_new = true;
            }
        }
    }
    assert!(file_a_has_old, "file_a should have old-format attestation");
    assert!(file_b_has_new, "file_b should have new-format attestation");

    // Verify blame on both files
    let mut fa = repo.filename("file_a.txt");
    fa.assert_committed_lines(crate::lines!["Human line A".human(), "Old AI line A".ai(),]);
    let mut fb = repo.filename("file_b.txt");
    fb.assert_committed_lines(crate::lines!["New session AI line B".ai(),]);
}
