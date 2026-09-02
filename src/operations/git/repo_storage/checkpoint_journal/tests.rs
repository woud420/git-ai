use super::*;
use crate::model::working_log::{Checkpoint, CheckpointKind, WorkingLogEntry};
use serde_json::Value;
use std::io::ErrorKind;

fn checkpoint_for_blob(content: &str) -> Checkpoint {
    let sha = sha256_hex(content.as_bytes());
    let entry = WorkingLogEntry::new(
        "src/example.rs".to_string(),
        sha.clone(),
        Vec::new(),
        Vec::new(),
    );
    Checkpoint::new(
        CheckpointKind::AiAgent,
        "diff".to_string(),
        "agent".to_string(),
        vec![entry],
    )
}

fn working_log(directory: &Path) -> PersistedWorkingLog {
    fs::create_dir_all(directory).unwrap();
    PersistedWorkingLog::new(
        directory.to_path_buf(),
        "base",
        directory.to_path_buf(),
        directory.to_path_buf(),
        None,
    )
}

#[test]
fn legacy_checkpoint_record_decodes_without_recovery_blobs() {
    let checkpoint = Checkpoint::new(
        CheckpointKind::Human,
        "legacy-diff".to_string(),
        "legacy-author".to_string(),
        Vec::new(),
    );
    let bytes = serde_json::to_vec(&checkpoint).unwrap();

    let decoded = decode(&bytes).expect("legacy record should decode");

    assert_eq!(decoded.diff, "legacy-diff");
    assert_eq!(decoded.author, "legacy-author");
}

#[test]
fn v1_checkpoint_record_roundtrips_with_terminal_checksum() {
    let checkpoint = checkpoint_for_blob("fn answer() -> u8 { 42 }\n");

    let encoded = encode(&checkpoint).expect("v1 record should encode");
    assert!(
        encoded.ends_with(b"\"}"),
        "journal record must end with its checksum string"
    );
    let checksum_marker = format!(",\"{RECORD_CHECKSUM_FIELD}\":\"");
    let checksum_start = encoded
        .windows(checksum_marker.len())
        .position(|window| window == checksum_marker.as_bytes())
        .expect("record should have a checksum field");
    assert_eq!(
        encoded.len() - checksum_start,
        checksum_marker.len() + 64 + 2,
        "checksum must be terminal so verification can hash raw record bytes"
    );
    let value: Value = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(value[RECORD_VERSION_FIELD], JOURNAL_RECORD_VERSION);
    assert!(value[RECORD_CHECKSUM_FIELD].as_str().is_some());
    assert!(value.get("_git_ai_blobs").is_none());

    let decoded = decode(&encoded).expect("v1 record should decode");
    assert_eq!(decoded.entries[0].blob_sha, checkpoint.entries[0].blob_sha);
}

#[test]
fn v1_index_record_size_does_not_scale_with_blob_content() {
    let small = encode(&checkpoint_for_blob(&"s".repeat(1024))).unwrap();
    let large = encode(&checkpoint_for_blob(&"l".repeat(1024 * 1024))).unwrap();

    assert!(
        large.len().abs_diff(small.len()) < 128,
        "checkpoint index records must not duplicate blob bodies: small={}, large={}",
        small.len(),
        large.len()
    );
    assert!(!String::from_utf8(large).unwrap().contains("_git_ai_blobs"));
}

#[test]
fn terminal_v1_record_remains_readable_by_the_legacy_checkpoint_shape() {
    let checkpoint = checkpoint_for_blob("compatible\n");

    let decoded: Checkpoint = serde_json::from_slice(&encode(&checkpoint).unwrap()).unwrap();

    assert_eq!(decoded.author, checkpoint.author);
    assert_eq!(decoded.entries[0].blob_sha, checkpoint.entries[0].blob_sha);
}

#[test]
fn canonical_nonterminal_v1_record_remains_readable() {
    let checkpoint = checkpoint_for_blob("prior-v1\n");
    let value: Value = serde_json::from_slice(&encode(&checkpoint).unwrap()).unwrap();
    let prior_v1 = serde_json_canonicalizer::to_vec(&value).unwrap();

    let decoded = decode(&prior_v1).expect("prior canonical v1 record should decode");

    assert_eq!(decoded.author, checkpoint.author);
}

#[test]
fn v1_checkpoint_record_rejects_tampered_payload() {
    let checkpoint = checkpoint_for_blob("original\n");
    let encoded = encode(&checkpoint).unwrap();
    let mut value: Value = serde_json::from_slice(&encoded).unwrap();
    value["author"] = Value::String("tampered".to_string());

    let error =
        decode(&serde_json::to_vec(&value).unwrap()).expect_err("tampered record must fail closed");

    assert!(error.to_string().contains("checksum"), "{error}");
}

#[test]
fn durable_replace_publishes_the_complete_new_file() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("checkpoints.jsonl");
    let replacement = directory.path().join("checkpoints.jsonl.tmp");
    fs::write(&target, b"old\n").unwrap();
    fs::write(&replacement, b"new\n").unwrap();

    replace_file_durably(&replacement, &target).unwrap();

    assert_eq!(fs::read(target).unwrap(), b"new\n");
    assert!(!replacement.exists());
}

#[test]
fn duplicate_replay_does_not_certify_unrelated_legacy_blob_references() {
    let directory = tempfile::tempdir().unwrap();
    let working_log = working_log(&directory.path().join("log"));
    let missing_legacy = checkpoint_for_blob("missing legacy blob\n");
    let durable_legacy = checkpoint_for_blob("durable legacy blob\n");
    working_log
        .persist_file_version("durable legacy blob\n")
        .unwrap();
    let mut legacy_bytes = serde_json::to_vec(&missing_legacy).unwrap();
    legacy_bytes.push(b'\n');
    legacy_bytes.extend(serde_json::to_vec(&durable_legacy).unwrap());
    legacy_bytes.push(b'\n');
    fs::write(working_log.checkpoints_file(), legacy_bytes).unwrap();
    let mut checkpoints = read(&working_log, u64::MAX).unwrap();

    ensure_durable(&working_log, &checkpoints[1]).unwrap();
    let next = checkpoint_for_blob("next checkpoint\n");
    working_log
        .persist_file_version("next checkpoint\n")
        .unwrap();
    checkpoints.push(next);

    let error = append(&working_log, &checkpoints)
        .expect_err("the next append must still fence every legacy blob");
    assert!(
        matches!(error, GitAiError::IoError(ref error) if error.kind() == ErrorKind::NotFound),
        "unexpected error: {error}"
    );
}

#[test]
fn durable_reset_atomically_publishes_an_empty_journal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("checkpoints.jsonl");
    fs::write(&path, b"old record\n").unwrap();

    reset_file_durably(&path).unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"");
    assert!(!path.with_extension("jsonl.reset.tmp").exists());
}
