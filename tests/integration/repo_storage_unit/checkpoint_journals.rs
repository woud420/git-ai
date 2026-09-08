use super::{CHECKPOINT_API_VERSION, Checkpoint, CheckpointKind, TestRepo, fs, storage_for};

// ---------------------------------------------------------------------------
// 4. test_persisted_working_log_checkpoint_storage
// ---------------------------------------------------------------------------

#[test]
fn test_persisted_working_log_checkpoint_storage() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();

    let checkpoint = Checkpoint::new(
        CheckpointKind::Human,
        "test-diff".to_string(),
        "test-author".to_string(),
        vec![],
    );

    working_log
        .append_checkpoint(&checkpoint)
        .expect("Failed to append checkpoint");

    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Failed to read checkpoints");

    assert_eq!(checkpoints.len(), 1, "Should have one checkpoint");
    assert_eq!(checkpoints[0].author, "test-author");

    let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
    assert!(checkpoints_file.exists(), "Checkpoints file should exist");

    let checkpoint2 = Checkpoint::new(
        CheckpointKind::Human,
        "test-diff-2".to_string(),
        "test-author-2".to_string(),
        vec![],
    );

    working_log
        .append_checkpoint(&checkpoint2)
        .expect("Failed to append second checkpoint");

    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Failed to read checkpoints after second append");

    assert_eq!(checkpoints.len(), 2, "Should have two checkpoints");
    assert_eq!(checkpoints[1].author, "test-author-2");
}

#[test]
fn test_append_checkpoint_to_persists_the_materialized_collection_without_rereading() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();

    let first = Checkpoint::new(
        CheckpointKind::Human,
        "first-diff".to_string(),
        "first-author".to_string(),
        vec![],
    );
    working_log
        .append_checkpoint(&first)
        .expect("Failed to append first checkpoint");
    let mut materialized = working_log
        .read_all_checkpoints()
        .expect("Failed to materialize checkpoints");

    // Corrupt the on-disk file after materializing: the append must persist
    // from the in-memory collection, not from a second read of the file.
    let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
    fs::write(&checkpoints_file, b"not json\n").unwrap();

    let second = Checkpoint::new(
        CheckpointKind::Human,
        "second-diff".to_string(),
        "second-author".to_string(),
        vec![],
    );
    working_log
        .append_checkpoint_to(&mut materialized, second)
        .expect("Failed to append to the materialized collection");

    assert_eq!(
        materialized.len(),
        2,
        "The in-memory collection should now hold both checkpoints"
    );
    let persisted = working_log
        .read_all_checkpoints()
        .expect("Failed to read checkpoints back");
    assert_eq!(
        persisted
            .iter()
            .map(|checkpoint| checkpoint.author.as_str())
            .collect::<Vec<_>>(),
        vec!["first-author", "second-author"],
        "The persisted file must reflect the materialized collection plus the append"
    );
}

// ---------------------------------------------------------------------------
// 5. test_read_all_checkpoints_filters_incompatible_versions
// ---------------------------------------------------------------------------

#[test]
fn test_read_all_checkpoints_filters_incompatible_versions() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();

    let base_checkpoint = Checkpoint::new(
        CheckpointKind::Human,
        "diff --git a/file b/file".to_string(),
        "base-author".to_string(),
        vec![],
    );

    let missing_version_json = {
        let mut value = serde_json::to_value(&base_checkpoint).unwrap();
        if let serde_json::Value::Object(ref mut map) = value {
            map.remove("api_version");
        }
        serde_json::to_string(&value).unwrap()
    };

    let mut wrong_version_checkpoint = base_checkpoint.clone();
    wrong_version_checkpoint.api_version = "checkpoint/0.9.0".to_string();
    let wrong_version_json = serde_json::to_string(&wrong_version_checkpoint).unwrap();

    let mut correct_checkpoint = base_checkpoint.clone();
    correct_checkpoint.author = "correct-author".to_string();
    let correct_json = serde_json::to_string(&correct_checkpoint).unwrap();

    let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
    let combined = [missing_version_json, wrong_version_json, correct_json].join("\n");
    fs::write(&checkpoints_file, combined).expect("Failed to write checkpoints.jsonl");

    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Failed to read checkpoints");

    assert_eq!(
        checkpoints.len(),
        1,
        "Only the correct version should remain"
    );
    assert_eq!(checkpoints[0].author, "correct-author");
    assert_eq!(checkpoints[0].api_version, CHECKPOINT_API_VERSION);
}

#[test]
fn test_oversized_checkpoints_file_is_truncated_before_read() {
    let repo = TestRepo::new();
    let repo_storage = storage_for(&repo);
    let working_log = repo_storage
        .working_log_for_base_commit("test-commit-sha")
        .unwrap();
    let checkpoints_file = working_log.dir.join("checkpoints.jsonl");

    fs::write(&checkpoints_file, "this is intentionally not valid json\n")
        .expect("write oversized checkpoints fixture");

    let checkpoints = working_log
        .read_all_checkpoints_with_size_limit_for_test(8)
        .expect("oversized checkpoint file should be reset before parsing");

    assert!(
        checkpoints.is_empty(),
        "oversized checkpoints file should read back as empty"
    );
    assert_eq!(
        fs::metadata(&checkpoints_file)
            .expect("empty checkpoints file should remain")
            .len(),
        0,
        "oversized checkpoints file should be truncated to an empty file"
    );
}

#[test]
fn append_checkpoint_preserves_a_corrupt_journal_and_returns_the_error() {
    let repo = TestRepo::new();
    let working_log = storage_for(&repo)
        .working_log_for_base_commit("corrupt-journal")
        .unwrap();
    let path = working_log.checkpoints_file();
    let corrupt = b"{\"_git_ai_record_version\":1,\"_git_ai_record_checksum\":\"bad\"}\n";
    fs::write(&path, corrupt).unwrap();
    let checkpoint = Checkpoint::new(
        CheckpointKind::Human,
        "diff".to_string(),
        "author".to_string(),
        Vec::new(),
    );

    let error = working_log
        .append_checkpoint(&checkpoint)
        .expect_err("an unreadable journal must fail closed");

    assert!(error.to_string().contains("checksum"), "{error}");
    assert_eq!(fs::read(path).unwrap(), corrupt);
}
