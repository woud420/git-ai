use super::{AuthorshipLog, ExpectedLineExt, TestRepo, fs, write_note};

// Test 11: Reset --soft of a commit with old-format note, then make new AI edits (sessions),
// then re-commit. The working log after reset has INITIAL from old note (bare hex prompts).
// New checkpoints produce sessions. Re-commit must have BOTH prompts and sessions.
#[test]
fn test_reset_soft_old_note_then_new_session_checkpoints() {
    let repo = TestRepo::new_with_daemon_scope(crate::repos::test_repo::DaemonTestScope::Dedicated);
    let file_path = repo.path().join("reset_test.txt");

    // Step 1: Create initial commit (needed as parent)
    let base = "Base line\n";
    repo.human_edit("reset_test.txt", base);
    repo.stage_all_and_commit("Base commit").unwrap();

    // Step 2: Create second commit with AI content
    let second = "Base line\nOld AI line\n";
    fs::write(&file_path, second).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "reset_test.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Step 3: Replace the note with old-format (simulating pre-upgrade git-ai)
    let old_hash = "f1e2d3c4b5a69788";
    let old_note = format!(
        r#"reset_test.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "old_reset_session", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
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

    // Step 4: Reset --soft HEAD~1 (uncommit, triggers working log reconstruction with old prompts)
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();

    // Step 5: Make new AI edits (produces session-format checkpoints)
    let third = "Base line\nOld AI line\nNew session AI line\n";
    fs::write(&file_path, third).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "reset_test.txt"])
        .unwrap();

    // Step 6: Re-commit
    repo.git(&["add", "."]).unwrap();
    repo.commit("Re-committed with new edits").unwrap();

    // Step 7: Verify the resulting note has BOTH formats
    let new_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&new_sha)
        .expect("re-committed commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse note");

    // Old-format prompt from the reset commit's note should be preserved
    assert!(
        !log.metadata.prompts.is_empty(),
        "re-committed note should have prompts from old-format note (via reset reconstruction)"
    );

    // New-format session from the fresh checkpoint should be present
    assert!(
        !log.metadata.sessions.is_empty(),
        "re-committed note should have sessions from new checkpoint"
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
    assert!(has_old_att, "should have old-format attestation hash");
    assert!(
        has_new_att,
        "should have new-format (s_::t_) attestation hash"
    );

    // Verify blame
    let mut file = repo.filename("reset_test.txt");
    file.assert_committed_lines(crate::lines![
        "Base line".human(),
        "Old AI line".ai(),
        "New session AI line".ai(),
    ]);
}

// Test 23: Working log INITIAL from old-format note (via reset) contains old-format
// author_ids in the file entries. When the user adds both a known_human edit and a
// session-format AI edit, the resulting commit should properly route:
// - INITIAL's old-format author_ids → prompts
// - New known_human edits → humans
// - New AI edits → sessions
#[test]
fn test_initial_from_old_note_plus_human_and_session_edits() {
    let repo = TestRepo::new_with_daemon_scope(crate::repos::test_repo::DaemonTestScope::Dedicated);
    let file_path = repo.path().join("triple.txt");

    // Step 1: Base commit
    let base = "Line 1\n";
    repo.human_edit("triple.txt", base);
    repo.stage_all_and_commit("Base").unwrap();

    // Step 2: Commit with AI content
    let ai_edit = "Line 1\nOld AI line\n";
    fs::write(&file_path, ai_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "triple.txt"])
        .unwrap();
    let ai_commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Step 3: Replace with old-format note
    let old_hash = "tripletest567890";
    let old_note = format!(
        r#"triple.txt
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "triple_agent", "model": "gpt-4"}},
      "human_author": null,
      "messages": [],
      "total_additions": 1,
      "total_deletions": 0,
      "accepted_lines": 1,
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

    // Step 4: Reset --soft to bring content back to working tree
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();

    // Step 5: Add a known-human line
    let human_edit = "Line 1\nOld AI line\nHuman typed this\n";
    repo.human_edit("triple.txt", human_edit);

    // Step 6: Add a new AI line (session-format)
    let session_edit = "Line 1\nOld AI line\nHuman typed this\nNew AI session line\n";
    fs::write(&file_path, session_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "triple.txt"])
        .unwrap();

    // Step 7: Commit
    repo.git(&["add", "."]).unwrap();
    repo.commit("Mixed triple commit").unwrap();

    // Step 8: Verify
    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&sha)
        .expect("triple commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse triple note");

    // Old-format INITIAL attributions should produce prompts
    assert!(
        !log.metadata.prompts.is_empty(),
        "old-format INITIAL author_ids should route to prompts"
    );

    // Known-human edit should produce humans
    assert!(
        !log.metadata.humans.is_empty(),
        "known_human checkpoint should produce humans entry"
    );

    // New AI edit should produce sessions
    assert!(
        !log.metadata.sessions.is_empty(),
        "new AI checkpoint should produce sessions entry"
    );

    // Verify blame: all three types of attribution should work correctly
    let mut file = repo.filename("triple.txt");
    file.assert_committed_lines(crate::lines![
        "Line 1".human(),
        "Old AI line".ai(),
        "Human typed this".human(),
        "New AI session line".ai(),
    ]);
}
