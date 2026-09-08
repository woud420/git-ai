use super::{
    AuthorshipLog, CommitAuthorship, GitAiError, commit_unattributed_file,
    commits_with_authorship_notes, deepest_note_path, fs, get_commits_with_notes_from_list,
    get_reference_as_working_log, git_stdin_stdout, grep_ai_notes, head_sha, install_note_at_paths,
    note_blob_oids_for_commits, notes_api, read_authorship_v3, repo_with_handle, write_note,
};

#[test]
fn test_eng_214_read_notes_batch_finds_deeply_fanned_out_note() {
    let (repo, gitai_repo) = repo_with_handle();
    let commit_sha = commit_unattributed_file(&repo, "deep-note.txt", "deep\n", "Deep note target");
    install_note_at_paths(
        &gitai_repo,
        &[deepest_note_path(&commit_sha)],
        "deeply fanned out",
    );

    let notes = notes_api::read_notes_batch(&gitai_repo, std::slice::from_ref(&commit_sha))
        .expect("batch-read deeply fanned-out note");
    assert_eq!(
        notes.get(&commit_sha).map(String::as_str),
        Some("deeply fanned out")
    );
}

#[test]
fn test_grep_ai_notes_single_match() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.stage_all_and_commit("Commit").expect("commit");
    let commit_sha = head_sha(&repo);

    let note = "{\"tool\":\"cursor\",\"model\":\"claude-3-sonnet\"}";
    write_note(&gitai_repo, &commit_sha, note).expect("add note");

    // Search for "cursor" should find the commit
    let results = grep_ai_notes(&gitai_repo, "cursor").expect("grep");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], commit_sha);
}

#[test]
fn test_grep_ai_notes_multiple_matches() {
    let (repo, gitai_repo) = repo_with_handle();

    // Create three commits with notes
    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.stage_all_and_commit("Commit A").expect("commit A");
    let commit_a = head_sha(&repo);

    fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    repo.stage_all_and_commit("Commit B").expect("commit B");
    let commit_b = head_sha(&repo);

    fs::write(repo.path().join("c.txt"), "c\n").unwrap();
    repo.stage_all_and_commit("Commit C").expect("commit C");
    let commit_c = head_sha(&repo);

    // Add notes with "cursor" to all three
    write_note(&gitai_repo, &commit_a, "{\"tool\":\"cursor\"}").expect("add note A");
    write_note(&gitai_repo, &commit_b, "{\"tool\":\"cursor\"}").expect("add note B");
    write_note(&gitai_repo, &commit_c, "{\"tool\":\"cursor\"}").expect("add note C");

    // Search should find all three, sorted by commit date (newest first)
    let results = grep_ai_notes(&gitai_repo, "cursor").expect("grep");

    // Should find at least 3 commits (may find more from auto-created notes)
    assert!(
        results.len() >= 3,
        "Expected at least 3 results, got {}",
        results.len()
    );

    // Verify our three commits are in the results
    assert!(
        results.contains(&commit_a),
        "Results should contain commit A"
    );
    assert!(
        results.contains(&commit_b),
        "Results should contain commit B"
    );
    assert!(
        results.contains(&commit_c),
        "Results should contain commit C"
    );
}

#[test]
fn test_grep_ai_notes_no_match() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.stage_all_and_commit("Commit").expect("commit");
    let commit_sha = head_sha(&repo);

    let note = "{\"tool\":\"cursor\"}";
    write_note(&gitai_repo, &commit_sha, note).expect("add note");

    // Search for non-existent pattern
    let results = grep_ai_notes(&gitai_repo, "vscode");
    // grep may return empty or error if no matches, both are acceptable
    if let Ok(refs) = results {
        assert_eq!(refs.len(), 0);
    }
    // Err is also acceptable - git grep returns non-zero when no matches
}

#[test]
fn test_grep_ai_notes_no_notes() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    // Use git_og to create a commit without triggering checkpoint/notes
    repo.git_og(&["add", "."]).expect("add");
    repo.git_og(&["commit", "-m", "Commit"]).expect("commit");

    // No notes exist, search should return empty or error
    let results = grep_ai_notes(&gitai_repo, "cursor");
    // grep may return empty or error if refs/notes/ai doesn't exist
    if let Ok(refs) = results {
        assert_eq!(refs.len(), 0);
    }
    // Err is also acceptable - refs/notes/ai may not exist yet
}

#[test]
fn test_get_commits_with_notes_from_list() {
    let (repo, gitai_repo) = repo_with_handle();

    // Create commits - stage_all_and_commit auto-creates authorship notes,
    // so all commits will have notes. This is expected behavior.
    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.stage_all_and_commit("Commit A").expect("commit A");
    let commit_a = head_sha(&repo);

    fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    repo.stage_all_and_commit("Commit B").expect("commit B");
    let commit_b = head_sha(&repo);

    fs::write(repo.path().join("c.txt"), "c\n").unwrap();
    repo.stage_all_and_commit("Commit C").expect("commit C");
    let commit_c = head_sha(&repo);

    // Get authorship for all commits
    let commit_list = vec![commit_a.clone(), commit_b.clone(), commit_c.clone()];
    let result = get_commits_with_notes_from_list(&gitai_repo, &commit_list).expect("get commits");

    assert_eq!(result.len(), 3);

    // All commits should have logs since stage_all_and_commit creates them
    for (idx, commit_authorship) in result.iter().enumerate() {
        match commit_authorship {
            CommitAuthorship::Log {
                sha,
                git_author: _,
                authorship_log: _,
            } => {
                // This is expected - verify SHA matches
                let expected_sha = &commit_list[idx];
                assert_eq!(sha, expected_sha);
            }
            CommitAuthorship::NoLog { .. } => {
                // Also acceptable if checkpoint system didn't run
            }
        }
    }
}

#[test]
fn test_note_blob_oids_for_commits_empty() {
    let (_repo, gitai_repo) = repo_with_handle();

    // Empty list should return empty map
    let result = note_blob_oids_for_commits(&gitai_repo, &[]).expect("empty list");
    assert!(result.is_empty());
}

#[test]
#[ignore] // Checkpoint system auto-creates notes, making this assertion invalid
fn test_note_blob_oids_for_commits_no_notes() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.stage_all_and_commit("Commit").expect("commit");
    let commit_sha = head_sha(&repo);

    // Commit exists but has no note
    let result = note_blob_oids_for_commits(&gitai_repo, &[commit_sha]).expect("no notes");
    assert!(result.is_empty());
}

#[test]
fn test_read_notes_batch_errors_on_dangling_note_blob() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("dangling.txt"), "dangling\n").unwrap();
    repo.git_og(&["add", "."]).expect("add dangling");
    repo.git_og(&["commit", "-m", "Dangling note target"])
        .expect("commit dangling target");
    let commit_sha = head_sha(&repo);
    let missing_blob = "2222222222222222222222222222222222222222";
    let prefix = &commit_sha[..2];
    let suffix = &commit_sha[2..];

    let leaf_tree = git_stdin_stdout(
        &gitai_repo,
        &["mktree", "--missing"],
        format!("100644 blob {missing_blob}\t{suffix}\n").as_bytes(),
    );

    let root_tree = git_stdin_stdout(
        &gitai_repo,
        &["mktree"],
        format!("040000 tree {leaf_tree}\t{prefix}\n").as_bytes(),
    );
    repo.git_og(&["update-ref", "refs/notes/ai", &root_tree])
        .expect("install dangling notes tree");

    let result = notes_api::read_notes_batch(&gitai_repo, &[commit_sha]);
    assert!(
        result.is_err(),
        "a notes tree entry pointing at a missing blob must be reported as corruption, not as no note"
    );
}

#[test]
fn test_read_notes_batch_prefers_flat_note_in_mixed_fanout_tree() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("mixed.txt"), "mixed\n").unwrap();
    repo.git_og(&["add", "."]).expect("add mixed");
    repo.git_og(&["commit", "-m", "Mixed note target"])
        .expect("commit mixed target");
    let commit_sha = head_sha(&repo);
    let prefix = &commit_sha[..2];
    let suffix = &commit_sha[2..];

    let flat_blob = git_stdin_stdout(&gitai_repo, &["hash-object", "-w", "--stdin"], b"flat");
    let fanout_blob = git_stdin_stdout(&gitai_repo, &["hash-object", "-w", "--stdin"], b"fanout");
    let leaf_tree = git_stdin_stdout(
        &gitai_repo,
        &["mktree"],
        format!("100644 blob {fanout_blob}\t{suffix}\n").as_bytes(),
    );
    let root_tree = git_stdin_stdout(
        &gitai_repo,
        &["mktree"],
        format!("100644 blob {flat_blob}\t{commit_sha}\n040000 tree {leaf_tree}\t{prefix}\n")
            .as_bytes(),
    );
    repo.git_og(&["update-ref", "refs/notes/ai", &root_tree])
        .expect("install mixed notes tree");

    let notes =
        notes_api::read_notes_batch(&gitai_repo, std::slice::from_ref(&commit_sha)).unwrap();
    assert_eq!(
        notes.get(&commit_sha).map(String::as_str),
        Some("flat"),
        "mixed flat/fanout notes must preserve the historical flat-path preference"
    );
}

#[test]
fn test_commits_with_authorship_notes() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.stage_all_and_commit("Commit A").expect("commit A");
    let commit_a = head_sha(&repo);

    fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    repo.stage_all_and_commit("Commit B").expect("commit B");
    let commit_b = head_sha(&repo);

    // Both commits may already have notes from stage_all_and_commit
    // Add a custom note to A to ensure it has one
    write_note(&gitai_repo, &commit_a, "{\"test\":\"note\"}").expect("add note");

    let commits = vec![commit_a.clone(), commit_b.clone()];
    let result = commits_with_authorship_notes(&gitai_repo, &commits).expect("check notes");

    // Commit A should definitely be in results
    assert!(result.contains(&commit_a), "Commit A should have a note");

    // Commit B may or may not have a note depending on checkpoint system
    // Just verify we got at least 1 result (commit A)
    assert!(
        !result.is_empty(),
        "Should have at least 1 commit with notes"
    );
}

#[test]
fn test_get_reference_as_working_log() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.stage_all_and_commit("Commit").expect("commit");
    let commit_sha = head_sha(&repo);

    // Add a working log format note
    let working_log_json = "[]";
    write_note(&gitai_repo, &commit_sha, working_log_json).expect("add note");

    let result = get_reference_as_working_log(&gitai_repo, &commit_sha).expect("get working log");
    assert_eq!(result.len(), 0); // Empty array
}

#[test]
fn test_get_reference_as_authorship_log_v3_version_mismatch() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.stage_all_and_commit("Commit").expect("commit");
    let commit_sha = head_sha(&repo);

    // Create log with wrong version
    let mut log = AuthorshipLog::new();
    log.metadata.schema_version = "999".to_string();
    log.metadata.base_commit_sha = commit_sha.clone();

    let note_content = log.serialize_to_string().expect("serialize");
    write_note(&gitai_repo, &commit_sha, &note_content).expect("add note");

    // Should fail with version mismatch error
    let result = read_authorship_v3(&gitai_repo, &commit_sha);
    assert!(result.is_err());

    if let Err(GitAiError::Generic(msg)) = result {
        assert!(msg.contains("Unsupported authorship log version"));
    } else {
        panic!("Expected version mismatch error");
    }
}
