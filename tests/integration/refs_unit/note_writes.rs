use super::{
    AuthorshipLog, commit_unattributed_file, deepest_note_path, fs, git_stdin_stdout, head_sha,
    install_note_at_paths, note_blob_oids_for_commits, note_tree_paths, notes_add_batch,
    notes_add_blob_batch, read_authorship_v3, read_note, repo_with_handle, write_note,
};

#[test]
fn test_notes_add_and_show_authorship_note() {
    let (repo, gitai_repo) = repo_with_handle();

    // Create a commit first
    fs::write(repo.path().join("initial.txt"), "initial\n").unwrap();
    repo.stage_all_and_commit("Initial commit")
        .expect("Failed to create initial commit");

    let commit_sha = head_sha(&repo);

    // Test data - simple string content
    let note_content = "This is a test authorship note with some random content!";

    // Add the authorship note (force overwrite since stage_all_and_commit may create one)
    write_note(&gitai_repo, &commit_sha, note_content).expect("Failed to add authorship note");

    // Read the note back
    let retrieved_content =
        read_note(&gitai_repo, &commit_sha).expect("Failed to retrieve authorship note");

    // Assert the content matches exactly
    assert_eq!(retrieved_content, note_content);

    // Test that non-existent commit returns None
    let non_existent_content = read_note(&gitai_repo, "0000000000000000000000000000000000000000");
    assert!(non_existent_content.is_none());
}

#[test]
fn test_notes_add_batch_writes_multiple_notes() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.stage_all_and_commit("Commit A").expect("commit A");
    let commit_a = head_sha(&repo);

    fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    repo.stage_all_and_commit("Commit B").expect("commit B");
    let commit_b = head_sha(&repo);

    let entries = vec![
        (commit_a.clone(), "{\"note\":\"a\",\"value\":1}".to_string()),
        (commit_b.clone(), "{\"note\":\"b\",\"value\":2}".to_string()),
    ];

    notes_add_batch(&gitai_repo, &entries).expect("batch notes add");

    let note_a = read_note(&gitai_repo, &commit_a).expect("note A");
    let note_b = read_note(&gitai_repo, &commit_b).expect("note B");
    assert!(note_a.contains("\"note\":\"a\""));
    assert!(note_b.contains("\"note\":\"b\""));
}

#[test]
fn test_notes_add_batch_keeps_last_duplicate_entry() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("duplicate.txt"), "duplicate\n").unwrap();
    repo.stage_all_and_commit("Duplicate note target")
        .expect("commit duplicate note target");
    let commit_sha = head_sha(&repo);

    let entries = vec![
        (commit_sha.clone(), "first".to_string()),
        (commit_sha.clone(), "last".to_string()),
    ];
    notes_add_batch(&gitai_repo, &entries).expect("write duplicate batch");

    assert_eq!(read_note(&gitai_repo, &commit_sha).as_deref(), Some("last"));
}

#[test]
fn test_notes_add_batch_streams_large_note_contents() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("large-a.txt"), "a\n").unwrap();
    repo.stage_all_and_commit("Large note target A")
        .expect("commit A");
    let commit_a = head_sha(&repo);

    fs::write(repo.path().join("large-b.txt"), "b\n").unwrap();
    repo.stage_all_and_commit("Large note target B")
        .expect("commit B");
    let commit_b = head_sha(&repo);

    // Contents larger than any internal write buffer, so a single batch
    // exercises chunked stdin writes end-to-end through one fast-import.
    let large_a = format!("{{\"padding\":\"{}\"}}", "a".repeat(512 * 1024));
    let large_b = format!("{{\"padding\":\"{}\"}}", "b".repeat(512 * 1024));
    let entries = vec![
        (commit_a.clone(), large_a.clone()),
        (commit_b.clone(), large_b.clone()),
    ];
    notes_add_batch(&gitai_repo, &entries).expect("write large batch");

    assert_eq!(
        read_note(&gitai_repo, &commit_a).as_deref(),
        Some(&*large_a)
    );
    assert_eq!(
        read_note(&gitai_repo, &commit_b).as_deref(),
        Some(&*large_b)
    );
}

#[test]
fn test_eng_214_notes_add_batch_replaces_notes_at_every_legacy_fanout_depth() {
    let (repo, gitai_repo) = repo_with_handle();
    let commit_sha =
        commit_unattributed_file(&repo, "legacy.txt", "legacy\n", "Legacy note target");
    let canonical_path = format!("{}/{}", &commit_sha[..2], &commit_sha[2..]);
    let legacy_paths = vec![
        commit_sha.clone(),
        canonical_path.clone(),
        format!(
            "{}/{}/{}",
            &commit_sha[..2],
            &commit_sha[2..4],
            &commit_sha[4..]
        ),
        deepest_note_path(&commit_sha),
    ];
    install_note_at_paths(&gitai_repo, &legacy_paths, "{}");
    assert_eq!(note_tree_paths(&repo).len(), legacy_paths.len());

    let mut replacement = AuthorshipLog::new();
    replacement.metadata.base_commit_sha = commit_sha.clone();
    let replacement = replacement
        .serialize_to_string()
        .expect("serialize replacement note");
    notes_add_batch(&gitai_repo, &[(commit_sha.clone(), replacement)])
        .expect("replace mixed-fanout note");

    assert_eq!(note_tree_paths(&repo), vec![canonical_path]);
    let parsed = read_authorship_v3(&gitai_repo, &commit_sha).expect("parse replacement note");
    assert_eq!(parsed.metadata.base_commit_sha, commit_sha);
}

#[test]
fn test_notes_add_blob_batch_reuses_existing_note_blob() {
    let (repo, gitai_repo) = repo_with_handle();

    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.stage_all_and_commit("Commit A").expect("commit A");
    let commit_a = head_sha(&repo);

    fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    repo.stage_all_and_commit("Commit B").expect("commit B");
    let commit_b = head_sha(&repo);

    let mut log = AuthorshipLog::new();
    log.metadata.base_commit_sha = commit_a.clone();
    let note_content = log.serialize_to_string().expect("serialize authorship log");
    write_note(&gitai_repo, &commit_a, &note_content).expect("add note A");

    let blob_oids = note_blob_oids_for_commits(&gitai_repo, std::slice::from_ref(&commit_a))
        .expect("resolve note blob oid");
    let blob_oid = blob_oids
        .get(&commit_a)
        .expect("blob oid for commit A")
        .clone();

    let blob_entry = (commit_b.clone(), blob_oid);
    notes_add_blob_batch(&gitai_repo, std::slice::from_ref(&blob_entry))
        .expect("batch add blob-backed note");

    let raw_note_b = read_note(&gitai_repo, &commit_b).expect("note B");
    assert_eq!(raw_note_b, note_content);

    let parsed_note_b = read_authorship_v3(&gitai_repo, &commit_b).expect("parse B");
    assert_eq!(parsed_note_b.metadata.base_commit_sha, commit_b);
}

#[test]
fn test_eng_214_notes_add_blob_batch_replaces_notes_at_every_legacy_fanout_depth() {
    let (repo, gitai_repo) = repo_with_handle();
    let commit_sha = commit_unattributed_file(
        &repo,
        "legacy-blob.txt",
        "legacy\n",
        "Legacy blob note target",
    );
    let canonical_path = format!("{}/{}", &commit_sha[..2], &commit_sha[2..]);
    let legacy_paths = vec![
        commit_sha.clone(),
        format!(
            "{}/{}/{}",
            &commit_sha[..2],
            &commit_sha[2..4],
            &commit_sha[4..]
        ),
        deepest_note_path(&commit_sha),
    ];
    install_note_at_paths(&gitai_repo, &legacy_paths, "stale");
    assert_eq!(note_tree_paths(&repo).len(), legacy_paths.len());

    let replacement_blob = git_stdin_stdout(
        &gitai_repo,
        &["hash-object", "-w", "--stdin"],
        b"replacement",
    );
    notes_add_blob_batch(&gitai_repo, &[(commit_sha.clone(), replacement_blob)])
        .expect("replace mixed-fanout blob note");

    assert_eq!(note_tree_paths(&repo), vec![canonical_path]);
    assert_eq!(
        read_note(&gitai_repo, &commit_sha).as_deref(),
        Some("replacement")
    );
}
