// Critical regression tests for old-format/new-format coexistence during cutover scenarios
//
// These tests verify that git-ai correctly handles:
// 1. Old-format authorship notes (bare 16-char hex hashes, prompts-only metadata)
// 2. New-format authorship notes (s_::t_ hashes, sessions metadata)
// 3. Mixed scenarios where both formats coexist in the same note or across operations
//
// Format detection: checkpoint.trace_id.is_some() determines which format is used.

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::operations::git::notes_api::write_note;
use git_ai::operations::git::repo_storage::PersistedWorkingLog;
use serde_json::Value;
use std::fs;

fn rewrite_checkpoint_journal_as_legacy(working_log: &PersistedWorkingLog) {
    let content = working_log
        .read_all_checkpoints()
        .unwrap()
        .into_iter()
        .map(|checkpoint| serde_json::to_string(&checkpoint).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(working_log.checkpoints_file(), content).unwrap();
}

mod diff_formats;
mod legacy_notes;
mod prompt_lookup;
mod rewrite_compatibility;
mod working_log_formats;

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
