use super::*;
use crate::model::working_log::{Checkpoint, WorkingLogEntry};

#[test]
fn test_path_matches_any_exact() {
    let specs = vec!["src/main.rs".to_string()];
    assert!(path_matches_any("src/main.rs", &specs));
    assert!(!path_matches_any("src/lib.rs", &specs));
}

#[test]
fn test_path_matches_any_directory_prefix() {
    let specs = vec!["src/".to_string()];
    assert!(path_matches_any("src/main.rs", &specs));
    assert!(path_matches_any("src/lib.rs", &specs));
    assert!(!path_matches_any("tests/main.rs", &specs));
}

#[test]
fn test_path_matches_any_directory_without_slash() {
    let specs = vec!["src".to_string()];
    assert!(path_matches_any("src/main.rs", &specs));
    assert!(!path_matches_any("src2/main.rs", &specs));
}

#[test]
fn test_path_matches_any_trailing_slash_normalized() {
    let specs = vec!["dir/".to_string()];
    assert!(path_matches_any("dir", &specs));
    assert!(path_matches_any("dir/file.txt", &specs));
}

#[test]
fn test_path_matches_any_empty_specs() {
    let specs: Vec<String> = vec![];
    assert!(!path_matches_any("anything", &specs));
}

#[test]
fn test_path_matches_any_trailing_glob() {
    // Regression (#5): the pre-rewrite matcher honored a trailing `*`
    // prefix-glob; path_matches_any dropped it, so `git stash push --
    // 'src/foo*'` no longer matched src/foobar.txt.
    let specs = vec!["src/foo*".to_string()];
    assert!(path_matches_any("src/foobar.txt", &specs));
    assert!(path_matches_any("src/foo.rs", &specs));
    assert!(!path_matches_any("src/bar.rs", &specs));
    // A bare `*` matches anything.
    assert!(path_matches_any("anything/at/all.txt", &["*".to_string()]));
}

#[test]
fn test_stash_metadata_serialization_roundtrip() {
    let metadata = StashMetadata {
        base_commit: "abc123def456".to_string(),
        timestamp: 1700000000,
        pathspecs: vec!["src/".to_string(), "Cargo.toml".to_string()],
    };

    let json = serde_json::to_string_pretty(&metadata).unwrap();
    let deserialized: StashMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.base_commit, "abc123def456");
    assert_eq!(deserialized.timestamp, 1700000000);
    assert_eq!(deserialized.pathspecs.len(), 2);
    assert_eq!(deserialized.pathspecs[0], "src/");
    assert_eq!(deserialized.pathspecs[1], "Cargo.toml");
}

#[test]
fn test_stash_metadata_empty_pathspecs_default() {
    let json = r#"{"base_commit":"abc123","timestamp":100}"#;
    let metadata: StashMetadata = serde_json::from_str(json).unwrap();
    assert!(metadata.pathspecs.is_empty());
}

#[test]
fn path_filtered_copy_rejects_a_tampered_journal_record() {
    let root = tempfile::tempdir().unwrap();
    let source_dir = root.path().join("source");
    let filtered_dir = root.path().join("filtered");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&filtered_dir).unwrap();
    let source = PersistedWorkingLog::new(
        source_dir,
        "source",
        root.path().to_path_buf(),
        root.path().to_path_buf(),
        None,
    );
    let filtered = PersistedWorkingLog::new(
        filtered_dir,
        "filtered",
        root.path().to_path_buf(),
        root.path().to_path_buf(),
        None,
    );
    let blob_sha = source.persist_file_version("AI state\n").unwrap();
    let checkpoint = Checkpoint::new(
        CheckpointKind::AiAgent,
        "diff".to_string(),
        "mock_ai".to_string(),
        vec![WorkingLogEntry::new(
            "sample.txt".to_string(),
            blob_sha,
            Vec::new(),
            Vec::new(),
        )],
    );
    let mut checkpoints = Vec::new();
    source
        .append_checkpoint_record_to(&mut checkpoints, checkpoint)
        .unwrap();
    let path = source.checkpoints_file();
    let tampered = fs::read_to_string(&path)
        .unwrap()
        .replace("mock_ai", "tampered-agent");
    fs::write(path, tampered).unwrap();

    let error = write_path_filtered_checkpoints(&source, &filtered, &["sample.txt".to_string()])
        .expect_err("stash filtering must not launder a bad checksum");

    assert!(error.to_string().contains("checksum"), "{error}");
}

#[test]
fn missing_stash_content_uses_structured_persistence_error() {
    let root = tempfile::tempdir().unwrap();
    let working_log = PersistedWorkingLog::new(
        root.path().join("working"),
        "base",
        root.path().to_path_buf(),
        root.path().to_path_buf(),
        None,
    );
    let files = HashMap::from([(
        "sample.txt".to_string(),
        vec![LineAttribution::new(1, 1, "ai".to_string(), None)],
    )]);

    let error = merge_initial_replacing_paths_with_contents(
        &working_log,
        files,
        HashMap::new(),
        BTreeMap::new(),
        HashMap::new(),
        BTreeMap::new(),
    )
    .expect_err("missing stash content must fail");

    assert!(matches!(&error, GitAiError::Persistence(_)));
    assert_eq!(
        error.to_string(),
        "Generic error: stash restore missing file content snapshot for sample.txt"
    );
}
