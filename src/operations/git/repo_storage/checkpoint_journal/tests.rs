use super::*;
use crate::model::attribution::{Attribution, LineAttribution};
use crate::model::working_log::{
    AgentId, Checkpoint, CheckpointKind, CheckpointLineStats, KnownHumanMetadata, WorkingLogEntry,
};
use serde_json::Value;
use std::collections::HashMap;
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

fn benchmark_checkpoint() -> Checkpoint {
    let mut checkpoint = Checkpoint::new(
        CheckpointKind::AiAgent,
        "acca49ac01ce3faf77458c5e2170e81d533fbb1c2f38e6ee2d2a57600828bd54".to_string(),
        "ENG-364 Benchmark <eng364@example.invalid>".to_string(),
        vec![WorkingLogEntry::new(
            "sample.txt".to_string(),
            "37f8c7fa59d1dc54eb1a3b9e063b2e8a496322b9c9bb29f60f637510100a8c08".to_string(),
            vec![
                Attribution::new(
                    0,
                    0,
                    "s_6825489e8808db::t_dd4d4964d7d736".to_string(),
                    1_788_375_035_718,
                ),
                Attribution::new(
                    0,
                    65,
                    "s_6825489e8808db::t_dd4d4964d7d736".to_string(),
                    1_788_375_035_718,
                ),
            ],
            vec![LineAttribution::new(
                2,
                4,
                "s_6825489e8808db::t_dd4d4964d7d736".to_string(),
                None,
            )],
        )],
    );
    checkpoint.timestamp = 1_788_375_035;
    checkpoint.agent_id = Some(AgentId {
        tool: "mock_ai".to_string(),
        id: "ai-thread-1788375035675706000".to_string(),
        model: "unknown".to_string(),
    });
    checkpoint.agent_metadata = Some(HashMap::from([(
        "edit_kind".to_string(),
        "file_edit".to_string(),
    )]));
    checkpoint.line_stats = CheckpointLineStats {
        additions: 1,
        deletions: 2,
        additions_sloc: 3,
        deletions_sloc: 4,
    };
    checkpoint.api_version = CHECKPOINT_API_VERSION.to_string();
    checkpoint.git_ai_version = Some("1.6.16".to_string());
    checkpoint.trace_id = Some("t_dd4d4964d7d736".to_string());
    checkpoint.delivery_id = Some("a13f0762-1a2a-4a59-bd88-bba7783711db".to_string());
    checkpoint
}

fn sign_v1_value(mut value: Value) -> Vec<u8> {
    value
        .as_object_mut()
        .expect("v1 record should be an object")
        .remove(wire_v1::CHECKSUM_FIELD);
    let mut unsigned = serde_json_canonicalizer::to_vec(&value).unwrap();
    let checksum = sha256_hex(&unsigned);
    assert_eq!(unsigned.pop(), Some(b'}'));
    unsigned
        .extend_from_slice(format!(",\"{}\":\"{checksum}\"}}", wire_v1::CHECKSUM_FIELD).as_bytes());
    unsigned
}

fn sign_v2_value(mut value: Value) -> Vec<u8> {
    value
        .as_object_mut()
        .expect("v2 record should be an object")
        .remove("c");
    let mut unsigned = serde_json::to_vec(&value).unwrap();
    let checksum = sha256_hex(&unsigned);
    assert_eq!(unsigned.pop(), Some(b'}'));
    unsigned.extend_from_slice(format!(",\"c\":\"{checksum}\"}}").as_bytes());
    unsigned
}

#[test]
fn v2_benchmark_record_is_655_bytes_and_roundtrips() {
    let checkpoint = benchmark_checkpoint();

    let encoded = wire_v2::encode(&checkpoint).expect("v2 record should encode");

    assert_eq!(encoded.len(), 655);
    assert_eq!(encoded.len() + 1, 656);
    assert_eq!(encoded.first(), Some(&b'{'));
    assert!(encoded.ends_with(b"\"}"));
    let encoded_text = std::str::from_utf8(&encoded).unwrap();
    let ordered_fields = [
        "{\"a\":", ",\"d\":", ",\"e\":", ",\"g\":", ",\"i\":", ",\"k\":", ",\"m\":", ",\"r\":",
        ",\"s\":", ",\"t\":", ",\"v\":", ",\"y\":", ",\"c\":",
    ];
    let mut previous = 0;
    for field in ordered_fields {
        let position = encoded_text.find(field).expect("wire field should exist");
        assert!(
            position >= previous,
            "wire fields must have stable ordering"
        );
        previous = position;
    }
    let decoded = wire_v2::decode(&encoded).expect("v2 record should decode");
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(checkpoint).unwrap()
    );
}

#[test]
fn v2_benchmark_record_matches_the_independent_golden_bytes() {
    let encoded = wire_v2::encode(&benchmark_checkpoint()).unwrap();
    let expected = r#"{"a":"ENG-364 Benchmark <eng364@example.invalid>","d":"acca49ac01ce3faf77458c5e2170e81d533fbb1c2f38e6ee2d2a57600828bd54","e":[["sample.txt","37f8c7fa59d1dc54eb1a3b9e063b2e8a496322b9c9bb29f60f637510100a8c08",[[0,0,"s_6825489e8808db::t_dd4d4964d7d736",1788375035718],[0,65,"s_6825489e8808db::t_dd4d4964d7d736",1788375035718]],[[2,4,"s_6825489e8808db::t_dd4d4964d7d736",null]]]],"g":"1.6.16","i":["mock_ai","ai-thread-1788375035675706000","unknown"],"k":1,"m":{"edit_kind":"file_edit"},"r":"t_dd4d4964d7d736","s":[1,2,3,4],"t":1788375035,"v":2,"y":"a13f0762-1a2a-4a59-bd88-bba7783711db","c":"0983d6ab788414f217a574d33c7ebc9d82c0a2a8ef649c695cc5aa8212f84a84"}"#;

    assert_eq!(std::str::from_utf8(&encoded).unwrap(), expected);
}

#[test]
fn v2_wire_version_pins_the_checkpoint_api_version() {
    assert_eq!(wire_v2::API_VERSION, CHECKPOINT_API_VERSION);
}

#[test]
fn v2_record_preserves_optional_shapes_and_all_checkpoint_kinds() {
    for kind in [
        CheckpointKind::Human,
        CheckpointKind::AiAgent,
        CheckpointKind::AiTab,
        CheckpointKind::KnownHuman,
    ] {
        let mut checkpoint =
            Checkpoint::new(kind, "diff".to_string(), "author".to_string(), Vec::new());
        checkpoint.agent_metadata = Some(HashMap::new());
        checkpoint.git_ai_version = None;
        checkpoint.known_human_metadata = Some(KnownHumanMetadata {
            editor: "editor".to_string(),
            editor_version: "1.2.3".to_string(),
            extension_version: "4.5.6".to_string(),
        });

        let decoded = wire_v2::decode(&wire_v2::encode(&checkpoint).unwrap()).unwrap();

        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(checkpoint).unwrap()
        );
    }
}

#[test]
fn v2_kind_codes_are_pinned_for_encoding_and_decoding() {
    for (kind, code) in [
        (CheckpointKind::Human, 0),
        (CheckpointKind::AiAgent, 1),
        (CheckpointKind::AiTab, 2),
        (CheckpointKind::KnownHuman, 3),
    ] {
        let checkpoint =
            Checkpoint::new(kind, "diff".to_string(), "author".to_string(), Vec::new());
        let mut value: Value =
            serde_json::from_slice(&wire_v2::encode(&checkpoint).unwrap()).unwrap();
        assert_eq!(value["k"], code);

        value["k"] = Value::from(code);
        let decoded = wire_v2::decode(&sign_v2_value(value)).unwrap();
        assert_eq!(decoded.kind, kind);
    }
}

#[test]
fn v2_known_human_tuple_order_is_pinned_for_encoding_and_decoding() {
    let mut checkpoint = Checkpoint::new(
        CheckpointKind::KnownHuman,
        "diff".to_string(),
        "author".to_string(),
        Vec::new(),
    );
    checkpoint.known_human_metadata = Some(KnownHumanMetadata {
        editor: "editor".to_string(),
        editor_version: "editor-version".to_string(),
        extension_version: "extension-version".to_string(),
    });
    let mut value: Value = serde_json::from_slice(&wire_v2::encode(&checkpoint).unwrap()).unwrap();
    assert_eq!(
        value["h"],
        serde_json::json!(["editor", "editor-version", "extension-version"])
    );

    value["h"] = serde_json::json!([
        "decoded-editor",
        "decoded-editor-version",
        "decoded-extension-version"
    ]);
    let decoded = wire_v2::decode(&sign_v2_value(value)).unwrap();
    let metadata = decoded.known_human_metadata.unwrap();
    assert_eq!(metadata.editor, "decoded-editor");
    assert_eq!(metadata.editor_version, "decoded-editor-version");
    assert_eq!(metadata.extension_version, "decoded-extension-version");
}

#[test]
fn v2_line_attribution_and_stats_tuple_order_is_pinned_for_decoding() {
    let unsigned = br#"{"a":"author","d":"diff","e":[["file","blob",[],[[2,4,"line-author","overrode-author"]]]],"k":1,"s":[1,2,3,4],"t":5,"v":2}"#;
    let checksum = sha256_hex(unsigned);
    let mut encoded = unsigned[..unsigned.len() - 1].to_vec();
    encoded.extend_from_slice(format!(r#","c":"{checksum}"}}"#).as_bytes());

    let decoded = wire_v2::decode(&encoded).unwrap();
    let line = &decoded.entries[0].line_attributions[0];
    assert_eq!(line.start_line, 2);
    assert_eq!(line.end_line, 4);
    assert_eq!(line.author_id, "line-author");
    assert_eq!(line.overrode.as_deref(), Some("overrode-author"));
    assert_eq!(decoded.line_stats.additions, 1);
    assert_eq!(decoded.line_stats.deletions, 2);
    assert_eq!(decoded.line_stats.additions_sloc, 3);
    assert_eq!(decoded.line_stats.deletions_sloc, 4);
}

#[test]
fn v2_record_sorts_agent_metadata_before_signing() {
    let mut first = benchmark_checkpoint();
    first.agent_metadata = Some(HashMap::from([
        ("z".to_string(), "last".to_string()),
        ("a".to_string(), "first".to_string()),
    ]));
    let mut second = first.clone();
    second.agent_metadata = Some(HashMap::new());
    second
        .agent_metadata
        .as_mut()
        .unwrap()
        .insert("a".to_string(), "first".to_string());
    second
        .agent_metadata
        .as_mut()
        .unwrap()
        .insert("z".to_string(), "last".to_string());

    assert_eq!(
        wire_v2::encode(&first).unwrap(),
        wire_v2::encode(&second).unwrap()
    );
}

#[test]
fn v2_record_rejects_a_tampered_payload() {
    let mut encoded = wire_v2::encode(&benchmark_checkpoint()).unwrap();
    let offset = encoded
        .windows(b"Benchmark".len())
        .position(|window| window == b"Benchmark")
        .unwrap();
    encoded[offset] = b'b';

    let error = wire_v2::decode(&encoded).expect_err("tampered record must fail closed");

    assert!(error.to_string().contains("checksum"), "{error}");
}

#[test]
fn v2_record_roundtrips_multiple_entries_and_overridden_authorship() {
    let mut checkpoint = benchmark_checkpoint();
    checkpoint.entries.push(WorkingLogEntry::new(
        "src/second.rs".to_string(),
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        vec![Attribution::new(2, 9, "second-author".to_string(), 99)],
        vec![LineAttribution::new(
            2,
            4,
            "second-author".to_string(),
            Some("original-author".to_string()),
        )],
    ));

    let decoded = wire_v2::decode(&wire_v2::encode(&checkpoint).unwrap()).unwrap();

    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(checkpoint).unwrap()
    );
}

#[test]
fn reserved_short_version_field_never_falls_back_to_legacy() {
    let mut value = serde_json::to_value(checkpoint_for_blob("reserved\n")).unwrap();
    value["v"] = Value::from(99);

    let error = decode(&serde_json::to_vec(&value).unwrap())
        .expect_err("reserved wire version must fail closed");

    assert!(error.to_string().contains("checksum"), "{error}");
}

#[test]
fn reserved_short_checksum_field_without_version_never_falls_back_to_legacy() {
    let mut value = serde_json::to_value(checkpoint_for_blob("reserved\n")).unwrap();
    value["c"] = Value::String("0".repeat(64));

    let error = decode(&serde_json::to_vec(&value).unwrap())
        .expect_err("reserved short checksum must fail closed");

    assert!(error.to_string().contains("version"), "{error}");
}

#[test]
fn signed_v1_record_with_short_version_field_fails_closed() {
    let checkpoint = checkpoint_for_blob("signed-v1-reserved\n");
    let mut value: Value = serde_json::from_slice(&wire_v1::encode(&checkpoint).unwrap()).unwrap();
    value["v"] = Value::from(99);
    let encoded = sign_v1_value(value);

    let error = decode(&encoded).expect_err("short version must take precedence over v1");

    assert!(error.to_string().contains("checksum"), "{error}");
}

#[test]
fn reader_accepts_legacy_v1_and_v2_records_in_one_journal() {
    let directory = tempfile::tempdir().unwrap();
    let log_directory = directory.path().join("log");
    fs::create_dir_all(&log_directory).unwrap();
    let location = JournalLocation::new(&log_directory, "base");
    let mut legacy = checkpoint_for_blob("legacy\n");
    legacy.author = "legacy".to_string();
    let mut v1 = checkpoint_for_blob("v1\n");
    v1.author = "v1".to_string();
    let mut v2 = checkpoint_for_blob("v2\n");
    v2.author = "v2".to_string();
    let mut bytes = serde_json::to_vec(&legacy).unwrap();
    bytes.push(b'\n');
    bytes.extend(wire_v1::encode(&v1).unwrap());
    bytes.push(b'\n');
    bytes.extend(wire_v2::encode(&v2).unwrap());
    bytes.push(b'\n');
    fs::write(location.checkpoints_file(), bytes).unwrap();

    let checkpoints = read(&location, u64::MAX).unwrap();

    assert!(checkpoints.contains_legacy_records());
    assert_eq!(
        checkpoints
            .iter()
            .map(|checkpoint| checkpoint.author.as_str())
            .collect::<Vec<_>>(),
        ["legacy", "v1", "v2"]
    );
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

    let encoded = wire_v1::encode(&checkpoint).expect("v1 record should encode");
    assert!(
        encoded.ends_with(b"\"}"),
        "journal record must end with its checksum string"
    );
    let checksum_marker = format!(",\"{}\":\"", wire_v1::CHECKSUM_FIELD);
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

    assert_eq!(value[wire_v1::VERSION_FIELD], wire_v1::VERSION);
    assert!(value[wire_v1::CHECKSUM_FIELD].as_str().is_some());
    assert!(value.get("_git_ai_blobs").is_none());

    let decoded = decode(&encoded).expect("v1 record should decode");
    assert_eq!(decoded.entries[0].blob_sha, checkpoint.entries[0].blob_sha);
}

#[test]
fn v1_index_record_size_does_not_scale_with_blob_content() {
    let small = wire_v1::encode(&checkpoint_for_blob(&"s".repeat(1024))).unwrap();
    let large = wire_v1::encode(&checkpoint_for_blob(&"l".repeat(1024 * 1024))).unwrap();

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

    let decoded: Checkpoint =
        serde_json::from_slice(&wire_v1::encode(&checkpoint).unwrap()).unwrap();

    assert_eq!(decoded.author, checkpoint.author);
    assert_eq!(decoded.entries[0].blob_sha, checkpoint.entries[0].blob_sha);
}

#[test]
fn canonical_nonterminal_v1_record_remains_readable() {
    let checkpoint = checkpoint_for_blob("prior-v1\n");
    let value: Value = serde_json::from_slice(&wire_v1::encode(&checkpoint).unwrap()).unwrap();
    let prior_v1 = serde_json_canonicalizer::to_vec(&value).unwrap();

    let decoded = decode(&prior_v1).expect("prior canonical v1 record should decode");

    assert_eq!(decoded.author, checkpoint.author);
}

#[test]
fn v1_checkpoint_record_rejects_tampered_payload() {
    let checkpoint = checkpoint_for_blob("original\n");
    let encoded = wire_v1::encode(&checkpoint).unwrap();
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
fn loaded_legacy_provenance_keeps_the_full_blob_fence() {
    let directory = tempfile::tempdir().unwrap();
    let log_directory = directory.path().join("log");
    let blobs_directory = log_directory.join("blobs");
    fs::create_dir_all(&blobs_directory).unwrap();
    let location = JournalLocation::new(&log_directory, "base");
    let missing_legacy = checkpoint_for_blob("missing legacy blob\n");
    let durable_legacy = checkpoint_for_blob("durable legacy blob\n");
    fs::write(
        blobs_directory.join(&durable_legacy.entries[0].blob_sha),
        b"durable legacy blob\n",
    )
    .unwrap();
    let mut legacy_bytes = serde_json::to_vec(&missing_legacy).unwrap();
    legacy_bytes.push(b'\n');
    legacy_bytes.extend(serde_json::to_vec(&durable_legacy).unwrap());
    legacy_bytes.push(b'\n');
    fs::write(location.checkpoints_file(), legacy_bytes).unwrap();
    let mut checkpoints = read(&location, u64::MAX).unwrap();

    ensure_durable(&location, &checkpoints[1]).unwrap();
    let next = checkpoint_for_blob("next checkpoint\n");
    fs::write(
        blobs_directory.join(&next.entries[0].blob_sha),
        b"next checkpoint\n",
    )
    .unwrap();
    checkpoints.push(next);

    assert!(checkpoints.contains_legacy_records());
    let error = rewrite(&location, &checkpoints)
        .expect_err("rewriting legacy records must still fence every referenced blob");
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
