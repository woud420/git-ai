use super::{GitAiError, HashMap, InitialAttributions, LineAttribution, TestRepo, fs, storage_for};

// ---------------------------------------------------------------------------
// 3. test_persisted_working_log_blob_storage
// ---------------------------------------------------------------------------

#[test]
fn test_persisted_working_log_blob_storage() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();

    let content = "abc";
    let expected_sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let sha = working_log
        .persist_file_version(content)
        .expect("Failed to persist file version");

    assert_eq!(sha, expected_sha, "blob name should be lowercase SHA-256");

    let retrieved_content = working_log
        .get_file_version(&sha)
        .expect("Failed to get file version");

    assert_eq!(
        content, retrieved_content,
        "Retrieved content should match original"
    );

    let blob_path = working_log.dir.join("blobs").join(&sha);
    assert!(blob_path.exists(), "Blob file should exist");
    assert!(blob_path.is_file(), "Blob should be a file");
    assert_eq!(
        fs::read(&blob_path).unwrap(),
        content.as_bytes(),
        "Blob should preserve the exact input bytes"
    );

    fs::write(&blob_path, b"stale").unwrap();
    let sha2 = working_log
        .persist_file_version(content)
        .expect("Failed to persist file version again");

    assert_eq!(sha, sha2, "Same content should produce same SHA");
    assert_eq!(
        fs::read(&blob_path).unwrap(),
        content.as_bytes(),
        "Repeated persistence should unconditionally rewrite the blob"
    );
    assert_eq!(
        fs::read_dir(working_log.dir.join("blobs")).unwrap().count(),
        1,
        "Repeated persistence should keep one content-addressed blob"
    );
}

#[test]
fn test_persisted_working_log_reuses_an_existing_valid_blob_without_rewriting() {
    let repo = TestRepo::new();
    let working_log = storage_for(&repo)
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();
    let content = "already durable";
    let sha = working_log.persist_file_version(content).unwrap();
    let blob_path = working_log.dir.join("blobs").join(&sha);
    let original_mtime = filetime::FileTime::from_unix_time(1, 0);
    filetime::set_file_mtime(&blob_path, original_mtime).unwrap();

    assert_eq!(working_log.persist_file_version(content).unwrap(), sha);

    let observed_mtime =
        filetime::FileTime::from_last_modification_time(&fs::metadata(&blob_path).unwrap());
    assert_eq!(observed_mtime, original_mtime);
    assert_eq!(fs::read(blob_path).unwrap(), content.as_bytes());
}

#[test]
fn test_persisted_working_log_blob_storage_preserves_io_error() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();
    let blobs_dir = working_log.dir.join("blobs");
    fs::write(&blobs_dir, b"blocker").unwrap();

    let expected = fs::create_dir_all(&blobs_dir).unwrap_err();
    let error = working_log
        .persist_file_version("abc")
        .expect_err("a file at the blobs path should prevent persistence");

    match &error {
        GitAiError::IoError(actual) => {
            assert_eq!(actual.kind(), expected.kind());
            assert_eq!(actual.to_string(), expected.to_string());
        }
        other => panic!("expected lossless IO error, got {other:?}"),
    }
    assert_eq!(error.to_string(), format!("IO error: {expected}"));
    assert_eq!(fs::read(&blobs_dir).unwrap(), b"blocker");
}

// ---------------------------------------------------------------------------
// 8. test_write_initial_with_contents_persists_snapshot_blob
// ---------------------------------------------------------------------------

#[test]
fn test_write_initial_with_contents_persists_snapshot_blob() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();

    let mut attributions = HashMap::new();
    attributions.insert(
        "src/test.rs".to_string(),
        vec![LineAttribution {
            start_line: 1,
            end_line: 1,
            author_id: "ai-1".to_string(),
            overrode: None,
        }],
    );
    let mut contents = HashMap::new();
    contents.insert("src/test.rs".to_string(), "fn main() {}\n".to_string());

    working_log
        .write_initial_attributions_with_contents(
            attributions,
            HashMap::new(),
            std::collections::BTreeMap::new(),
            contents,
            std::collections::BTreeMap::new(),
        )
        .expect("write INITIAL with contents");

    let initial = working_log.read_initial_attributions();
    let blob_sha = initial
        .file_blobs
        .get("src/test.rs")
        .expect("snapshot blob should exist");
    let persisted = working_log
        .get_file_version(blob_sha)
        .expect("read snapshot blob");
    assert_eq!(persisted, "fn main() {}\n");
}

#[test]
fn test_write_initial_with_contents_rejects_missing_snapshot() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();

    let mut attributions = HashMap::new();
    attributions.insert(
        "src/test.rs".to_string(),
        vec![LineAttribution {
            start_line: 1,
            end_line: 1,
            author_id: "ai-1".to_string(),
            overrode: None,
        }],
    );

    let error = working_log
        .write_initial_attributions_with_contents(
            attributions,
            HashMap::new(),
            std::collections::BTreeMap::new(),
            HashMap::new(),
            std::collections::BTreeMap::new(),
        )
        .expect_err("missing content snapshot must be rejected");

    assert!(
        matches!(error, GitAiError::Persistence(_)),
        "expected structured persistence error, got {error:?}"
    );
    assert_eq!(
        error.to_string(),
        "Generic error: INITIAL missing file content snapshot for src/test.rs"
    );
}

// ---------------------------------------------------------------------------
// 9. test_write_initial_empty_removes_existing_file
// ---------------------------------------------------------------------------

#[test]
fn test_write_initial_empty_removes_existing_file() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();

    let mut attributions = HashMap::new();
    attributions.insert(
        "src/test.rs".to_string(),
        vec![LineAttribution {
            start_line: 1,
            end_line: 1,
            author_id: "ai-1".to_string(),
            overrode: None,
        }],
    );
    working_log
        .write_initial_attributions_with_contents(
            attributions,
            HashMap::new(),
            std::collections::BTreeMap::new(),
            HashMap::from([("src/test.rs".to_string(), "fn main() {}\n".to_string())]),
            std::collections::BTreeMap::new(),
        )
        .expect("write INITIAL");
    assert!(working_log.initial_file.exists(), "INITIAL should exist");

    working_log
        .write_initial(InitialAttributions::default())
        .expect("clear INITIAL");
    assert!(
        !working_log.initial_file.exists(),
        "INITIAL should be removed when empty"
    );
}
