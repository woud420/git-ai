use super::{AuthorshipLog, ExpectedLineExt, TestRepo, fs, write_note};

// Test 9: Amend a commit that has an old-format note, with new-format checkpoints in the working log.
// This simulates: user had git-ai old version, made a commit (old prompts note), then upgraded git-ai,
// makes new edits (which produce session-format checkpoints), and amends the commit.
// The post-amend note must have BOTH old prompts AND new sessions.
#[test]
fn test_amend_old_prompts_commit_with_new_session_checkpoints() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.txt");

    // Step 1: Create initial commit with known-human context and AI content
    fs::write(&file_path, "Human line 1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "example.txt"])
        .unwrap();
    let initial = "Human line 1\nAI old line\n";
    fs::write(&file_path, initial).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Step 2: Replace the note with an old-format note (simulating pre-upgrade git-ai)
    let old_hash = "deadbeef12345678"; // 16-char bare hex (old format)
    let human_hash = "h_amendoldhuman";
    let old_note = format!(
        r#"example.txt
  {} 1-1
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "cursor", "id": "old_session_abc", "model": "gpt-4"}},
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

    // Step 3: Make new edits and checkpoint with new-format (mock_ai produces trace_id)
    let edited = "Human line 1\nAI old line\nAI new line\n";
    fs::write(&file_path, edited).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();

    // Step 4: Amend the commit (this triggers the amend rewrite pipeline)
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Amended commit"])
        .unwrap();

    // Step 5: Read the post-amend note and verify BOTH formats are present
    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note = repo
        .read_authorship_note(&amended_sha)
        .expect("amended commit should have note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse amended note");

    // The old-format prompt from the original note should still be there
    // (it was referenced by an attestation for line 2 which still exists)
    assert!(
        !log.metadata.prompts.is_empty(),
        "amended note should preserve old prompts from original note"
    );
    assert!(
        log.metadata.prompts.contains_key(old_hash),
        "old prompt hash should be preserved in amended note"
    );

    // The new checkpoint (with trace_id) should have produced a session
    assert!(
        !log.metadata.sessions.is_empty(),
        "amended note should have sessions from new checkpoint"
    );

    // Verify attestations include both formats
    let mut has_old_format_att = false;
    let mut has_new_format_att = false;
    for file_att in &log.attestations {
        for entry in &file_att.entries {
            if entry.hash == old_hash {
                has_old_format_att = true;
            }
            if entry.hash.starts_with("s_") && entry.hash.contains("::t_") {
                has_new_format_att = true;
            }
        }
    }
    assert!(
        has_old_format_att,
        "amended note should have old-format attestation hash"
    );
    assert!(
        has_new_format_att,
        "amended note should have new-format (s_::t_) attestation hash"
    );

    // Verify blame works correctly
    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(crate::lines![
        "Human line 1".human(),
        "AI old line".ai(),
        "AI new line".ai(),
    ]);
}

// Test 22: Multiple sequential amends on the same commit, mixing formats.
// Commit starts with old prompts → first amend adds session lines → second amend adds more.
// All attributions must survive through multiple amends.
#[test]
fn test_multiple_amends_mixed_format_accumulation() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("multi.txt");

    // Step 1: Initial commit with known-human context and AI content
    fs::write(&file_path, "Line 1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "multi.txt"])
        .unwrap();
    let initial = "Line 1\nOld AI line\n";
    fs::write(&file_path, initial).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "multi.txt"])
        .unwrap();
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Step 2: Replace note with old-format
    let old_hash = "multiamend123456";
    let human_hash = "h_multiamendhuman";
    let old_note = format!(
        r#"multi.txt
  {} 1-1
  {} 2-2
---
{{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.2.0",
  "base_commit_sha": "{}",
  "prompts": {{
    "{}": {{
      "agent_id": {{"tool": "copilot", "id": "multi_agent", "model": "gpt-4"}},
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

    // Step 3: First amend - add new AI line
    let edit1 = "Line 1\nOld AI line\nFirst session line\n";
    fs::write(&file_path, edit1).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "multi.txt"])
        .unwrap();
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "--amend", "-m", "First amend"])
        .unwrap();

    // Verify after first amend
    let sha1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note1 = repo
        .read_authorship_note(&sha1)
        .expect("first amend should have note");
    let log1 = AuthorshipLog::deserialize_from_string(&note1).expect("parse first amend note");
    assert!(
        log1.metadata.prompts.contains_key(old_hash),
        "first amend should preserve old prompt"
    );
    assert!(
        !log1.metadata.sessions.is_empty(),
        "first amend should have sessions"
    );

    // Step 4: Second amend - add another AI line
    let edit2 = "Line 1\nOld AI line\nFirst session line\nSecond session line\n";
    fs::write(&file_path, edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "multi.txt"])
        .unwrap();
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Second amend"])
        .unwrap();

    // Verify after second amend
    let sha2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let note2 = repo
        .read_authorship_note(&sha2)
        .expect("second amend should have note");
    let log2 = AuthorshipLog::deserialize_from_string(&note2).expect("parse second amend note");

    // Old prompt should STILL be preserved (its original AI line hasn't been deleted)
    assert!(
        log2.metadata.prompts.contains_key(old_hash),
        "second amend should still preserve old prompt (line is still there)"
    );

    // Sessions should be present (both amend additions)
    assert!(
        !log2.metadata.sessions.is_empty(),
        "second amend should have sessions"
    );

    // Verify blame: all AI lines are correctly attributed
    let mut file = repo.filename("multi.txt");
    file.assert_committed_lines(crate::lines![
        "Line 1".human(),
        "Old AI line".ai(),
        "First session line".ai(),
        "Second session line".ai(),
    ]);
}

// Regression: under the HTTP notes backend (notes live in the local notes-db,
// NOT in refs/notes/ai), amending a commit must preserve the sessions
// metadata on the rebuilt note. The amend pipeline re-reads the original note
// and session history through refs/notes/ai-only helpers
// (`refs::get_reference_as_authorship_log_v3`, `refs::grep_ai_notes`), which
// find nothing under the HTTP backend — so the amended note keeps its s_::t_
// attestation hashes but silently loses `metadata.sessions`, and downstream
// consumers bucket every AI line as tool=unknown.
#[test]
fn test_amend_preserves_sessions_under_http_notes_backend() {
    use git_ai::config::{ConfigPatch, NotesBackendConfig, NotesBackendKind};
    use git_ai::model::repository::notes_db::NotesDatabase;

    // The daemon owns note writes and the amend rebuild, so the DAEMON must run
    // with the HTTP backend. The test-home config.json writer does not cover
    // notes_backend and the daemon caches config at startup, so pass the patch
    // via env at daemon spawn.
    let temp_root = std::env::temp_dir();
    let temp_root = temp_root.canonicalize().unwrap_or(temp_root);
    let daemon_patch = ConfigPatch {
        allowed_repositories: Some(vec![temp_root.to_string_lossy().replace('\\', "/")]),
        exclude_prompts_in_repositories: Some(vec![]),
        prompt_storage: Some("notes".to_string()),
        notes_backend: Some(NotesBackendConfig {
            kind: NotesBackendKind::Http,
            backend_url: None,
        }),
        ..Default::default()
    };
    let daemon_patch_json =
        serde_json::to_string(&daemon_patch).expect("serialize daemon config patch");
    // `dirs::home_dir()` does not honor HOME/USERPROFILE overrides on Windows,
    // so explicitly isolate the daemon's HTTP notes cache at a path this test
    // can read on every platform.
    let notes_db_dir = tempfile::tempdir().expect("create isolated notes-db directory");
    let notes_db_path = notes_db_dir.path().join("notes-db");
    let notes_db_path_string = notes_db_path.to_string_lossy().to_string();
    let mut repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_CONFIG_PATCH", daemon_patch_json.as_str()),
        ("GIT_AI_TEST_NOTES_DB_PATH", notes_db_path_string.as_str()),
    ]);
    // CLI invocations (checkpoint, blame) should use the HTTP backend too.
    repo.patch_git_ai_config(|patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Http,
            backend_url: None,
        });
    });

    // Poll the notes-db for a commit's note: post-commit note writes land in the
    // daemon's notes-db queue (never refs/notes/ai), so the harness's usual
    // "note visible in refs/notes/ai" commit assertion cannot be used here.
    let read_note_from_db = |sha: &str| -> Option<String> {
        for _ in 0..100 {
            if let Ok(db) = NotesDatabase::open_at_path(&notes_db_path)
                && let Ok(Some(content)) = db.get_note(sha)
            {
                return Some(content);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    };

    let file_path = repo.path().join("http_amend.txt");
    repo.human_edit("http_amend.txt", "Human line\n");
    fs::write(&file_path, "Human line\nAI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "http_amend.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", "AI commit"]).unwrap();
    repo.sync_daemon();
    let original_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let original_note =
        read_note_from_db(&original_sha).expect("original commit should have a note in notes-db");
    // Under the HTTP backend the note must be in notes-db, not refs/notes/ai.
    assert!(
        repo.read_authorship_note(&original_sha).is_none(),
        "HTTP backend should not write to refs/notes/ai"
    );
    let original_log =
        AuthorshipLog::deserialize_from_string(&original_note).expect("parse original note");
    assert!(
        !original_log.metadata.sessions.is_empty(),
        "original note should carry sessions metadata"
    );

    // Amend the commit message only — the attributed content is unchanged, so
    // the rebuilt note must still attest the AI line to the same session.
    repo.git(&["commit", "--amend", "-m", "Amended commit"])
        .unwrap();
    repo.sync_daemon();
    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_ne!(amended_sha, original_sha, "amend should rewrite HEAD");

    let amended_note =
        read_note_from_db(&amended_sha).expect("amended commit should have a note in notes-db");
    let amended_log =
        AuthorshipLog::deserialize_from_string(&amended_note).expect("parse amended note");

    // The AI line's attestation must still use the session format...
    let has_session_attestation = amended_log
        .attestations
        .iter()
        .flat_map(|fa| fa.entries.iter())
        .any(|entry| entry.hash.starts_with("s_"));
    assert!(
        has_session_attestation,
        "amended note should still attest AI lines to a session hash:\n{}",
        amended_note
    );

    // ...and the sessions map those hashes resolve through must survive the amend.
    assert!(
        !amended_log.metadata.sessions.is_empty(),
        "amended note lost metadata.sessions — session attestations no longer resolve to a tool:\n{}",
        amended_note
    );

    // The surviving record must be the same session as the original note.
    for session_id in original_log.metadata.sessions.keys() {
        assert!(
            amended_log.metadata.sessions.contains_key(session_id),
            "session {} from the original note is missing after amend",
            session_id
        );
    }
}
