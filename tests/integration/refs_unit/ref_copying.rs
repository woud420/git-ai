use super::{
    AI_AUTHORSHIP_FORK_TRACKING_REF, copy_missing_notes_for_commits_from_ref, copy_ref, exec_git,
    fs, head_sha, merge_notes_from_ref, note_blob_oids_for_commits_from_ref, read_note, ref_exists,
    repo_with_handle, write_note,
};

#[test]
fn test_copy_missing_notes_for_commits_from_ref_copies_only_requested_commits() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.git_og(&["add", "."]).expect("add A");
    repo.git_og(&["commit", "-m", "Commit A"])
        .expect("commit A");
    let commit_a = head_sha(&repo);

    fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    repo.git_og(&["add", "."]).expect("add B");
    repo.git_og(&["commit", "-m", "Commit B"])
        .expect("commit B");
    let commit_b = head_sha(&repo);

    for (commit, note) in [(&commit_a, "fork-note-a"), (&commit_b, "fork-note-b")] {
        let mut args = gitai_repo.global_args_for_exec();
        args.extend_from_slice(&[
            "notes".to_string(),
            "--ref=ai-remote/fork".to_string(),
            "add".to_string(),
            "-f".to_string(),
            "-m".to_string(),
            note.to_string(),
            commit.clone(),
        ]);
        exec_git(&args).expect("add source note");
    }

    let source_notes = note_blob_oids_for_commits_from_ref(
        &gitai_repo,
        AI_AUTHORSHIP_FORK_TRACKING_REF,
        &[commit_a.clone(), commit_b.clone()],
    )
    .expect("source note oids");
    assert_eq!(source_notes.len(), 2);

    let copied = copy_missing_notes_for_commits_from_ref(
        &gitai_repo,
        AI_AUTHORSHIP_FORK_TRACKING_REF,
        std::slice::from_ref(&commit_a),
    )
    .expect("copy scoped notes");

    assert_eq!(copied, 1);
    assert_eq!(
        read_note(&gitai_repo, &commit_a).as_deref(),
        Some("fork-note-a")
    );
    assert!(
        read_note(&gitai_repo, &commit_b).is_none(),
        "note for unrequested commit must not be copied"
    );
}

#[test]
fn test_copy_missing_notes_for_commits_from_ref_keeps_existing_local_note() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.git_og(&["add", "."]).expect("add A");
    repo.git_og(&["commit", "-m", "Commit A"])
        .expect("commit A");
    let commit_a = head_sha(&repo);

    let mut args = gitai_repo.global_args_for_exec();
    args.extend_from_slice(&[
        "notes".to_string(),
        "--ref=ai-remote/fork".to_string(),
        "add".to_string(),
        "-f".to_string(),
        "-m".to_string(),
        "fork-note".to_string(),
        commit_a.clone(),
    ]);
    exec_git(&args).expect("add source note");

    write_note(&gitai_repo, &commit_a, "local-note").expect("add local note");

    let copied = copy_missing_notes_for_commits_from_ref(
        &gitai_repo,
        AI_AUTHORSHIP_FORK_TRACKING_REF,
        std::slice::from_ref(&commit_a),
    )
    .expect("copy scoped notes");

    assert_eq!(copied, 0);
    assert_eq!(
        read_note(&gitai_repo, &commit_a).as_deref(),
        Some("local-note")
    );
}

#[test]
fn test_ref_exists() {
    let (repo, gitai_repo) = repo_with_handle();

    // Create initial commit
    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.stage_all_and_commit("Initial commit").expect("commit");

    // HEAD should exist
    assert!(ref_exists(&gitai_repo, "HEAD"));

    // refs/heads/main (or master) should exist
    let branch_name = repo.current_branch();
    assert!(ref_exists(
        &gitai_repo,
        &format!("refs/heads/{}", branch_name)
    ));

    // Non-existent ref should not exist
    assert!(!ref_exists(&gitai_repo, "refs/heads/nonexistent-branch"));
    assert!(!ref_exists(&gitai_repo, "refs/notes/ai-test"));
}

#[test]
fn test_merge_notes_from_ref() {
    let (repo, gitai_repo) = repo_with_handle();

    // Create commits - stage_all_and_commit may auto-create notes on refs/notes/ai
    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.stage_all_and_commit("Commit A").expect("commit A");
    let _commit_a = head_sha(&repo);

    fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    repo.stage_all_and_commit("Commit B").expect("commit B");
    let _commit_b = head_sha(&repo);

    // Create a third commit without checkpoint (using git_og to bypass hooks)
    fs::write(repo.path().join("c.txt"), "c\n").unwrap();
    repo.git_og(&["add", "."]).expect("add files");
    repo.git_og(&["commit", "-m", "Commit C"]).expect("commit");
    let commit_c = head_sha(&repo);

    // Add note to commit C on a different ref
    let note_c = "{\"note\":\"c\"}";
    let mut args = gitai_repo.global_args_for_exec();
    args.extend_from_slice(&[
        "notes".to_string(),
        "--ref=test".to_string(),
        "add".to_string(),
        "-f".to_string(),
        "-m".to_string(),
        note_c.to_string(),
        commit_c.clone(),
    ]);
    exec_git(&args).expect("add note C on test ref");

    // Verify initial state - commit C should not have note on refs/notes/ai
    let initial_note_c = read_note(&gitai_repo, &commit_c);

    // Merge notes from refs/notes/test into refs/notes/ai
    merge_notes_from_ref(&gitai_repo, "refs/notes/test").expect("merge notes");

    // After merge, commit C should have a note on refs/notes/ai
    let final_note_c = read_note(&gitai_repo, &commit_c);

    // If initially had no note, should now have one. If it had one, should still have one.
    assert!(final_note_c.is_some() || initial_note_c.is_some());
}

#[test]
fn test_copy_ref() {
    let (repo, gitai_repo) = repo_with_handle();

    // Create commit with note
    fs::write(repo.path().join("test.txt"), "content\n").unwrap();
    repo.stage_all_and_commit("Commit").expect("commit");
    let commit_sha = head_sha(&repo);

    let note_content = "{\"test\":\"note\"}";
    write_note(&gitai_repo, &commit_sha, note_content).expect("add note");

    // refs/notes/ai should exist
    assert!(ref_exists(&gitai_repo, "refs/notes/ai"));

    // refs/notes/ai-backup should not exist
    assert!(!ref_exists(&gitai_repo, "refs/notes/ai-backup"));

    // Copy refs/notes/ai to refs/notes/ai-backup
    copy_ref(&gitai_repo, "refs/notes/ai", "refs/notes/ai-backup").expect("copy ref");

    // Both should now exist and point to the same commit
    assert!(ref_exists(&gitai_repo, "refs/notes/ai"));
    assert!(ref_exists(&gitai_repo, "refs/notes/ai-backup"));

    // Verify content is accessible from both refs
    let note_from_ai = read_note(&gitai_repo, &commit_sha).expect("note from ai");

    // Read from backup ref
    let mut args = gitai_repo.global_args_for_exec();
    args.extend_from_slice(&[
        "notes".to_string(),
        "--ref=ai-backup".to_string(),
        "show".to_string(),
        commit_sha.clone(),
    ]);
    let output = exec_git(&args).expect("show note from backup");
    let note_from_backup = String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_string();

    assert_eq!(note_from_ai, note_from_backup);
}
