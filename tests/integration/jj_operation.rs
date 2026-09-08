use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::model::jj_observation::JJ_OBSERVATION_READER_PROFILE;
use git_ai::operations::jj::operation::{
    MAX_OPERATION_ATTRIBUTES, MAX_OPERATION_BYTES, MAX_OPERATION_METADATA_BYTES,
    MAX_OPERATION_PARENTS, MAX_OPERATION_PREDECESSOR_REFERENCES, decode_operation,
};
use std::collections::BTreeMap;

#[path = "jj_operation_limits.rs"]
mod limits;
#[path = "jj_operation_real.rs"]
mod real;
#[path = "jj_operation_support.rs"]
mod support;
#[path = "jj_operation_vectors.rs"]
mod vectors;

use support::*;
use vectors::*;

fn reject(raw: &[u8], expected: &str) {
    let error = decode_operation(JJ_OBSERVATION_READER_PROFILE, RICH_ID, raw).unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains(expected),
        "expected {expected:?} error, received {error}"
    );
}

#[test]
fn jj_operation_decodes_independent_synthetic_domain_hash_from_test_repo() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let fixture = repo.path().join("synthetic-operation");
    std::fs::write(&fixture, unhex(RICH_HEX)).unwrap();
    let raw = std::fs::read(&fixture).unwrap();
    let decoded = decode_operation(JJ_OBSERVATION_READER_PROFILE, RICH_ID, &raw).unwrap();
    assert_eq!(decoded.operation_id, RICH_ID);
    assert_eq!(decoded.view_id, "11".repeat(64));
    assert_eq!(decoded.parent_ids, ["22".repeat(64), "33".repeat(64)]);
    assert_eq!(decoded.workspace_name.as_deref(), Some("main-工"));
    assert!(decoded.is_snapshot);
    assert_eq!(
        decoded.commit_predecessors,
        Some(BTreeMap::from([
            ("aa".repeat(20), vec!["bb".repeat(20), "cc".repeat(20)]),
            ("dd".repeat(20), vec![]),
        ]))
    );
    assert_eq!(std::fs::read(fixture).unwrap(), raw);
}

#[test]
fn jj_operation_normalizes_protobuf_field_and_map_order_before_hashing() {
    let mut metadata = rich_metadata_fields();
    metadata.reverse();
    let mut predecessors = rich_predecessors();
    predecessors.reverse();
    let raw = [
        scalar(5, 1),
        bytes_field(4, &predecessors[0]),
        bytes_field(3, &metadata.concat()),
        bytes_field(2, &[0x22; 64]),
        bytes_field(1, &[0x11; 64]),
        bytes_field(4, &predecessors[1]),
        bytes_field(2, &[0x33; 64]),
    ]
    .concat();
    assert_ne!(raw, unhex(RICH_HEX));
    let decoded = decode_operation(JJ_OBSERVATION_READER_PROFILE, RICH_ID, &raw).unwrap();
    assert_eq!(decoded.operation_id, RICH_ID);
}

#[test]
fn jj_operation_hash_preserves_absent_and_present_empty_workspace() {
    let none = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        NO_WORKSPACE_ID,
        &unhex(NO_WORKSPACE_HEX),
    )
    .unwrap();
    let empty = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        EMPTY_WORKSPACE_ID,
        &unhex(EMPTY_WORKSPACE_HEX),
    )
    .unwrap();
    assert_eq!(none.workspace_name, None);
    assert_eq!(empty.workspace_name.as_deref(), Some(""));
    assert_ne!(none.operation_id, empty.operation_id);
    assert!(
        decode_operation(
            JJ_OBSERVATION_READER_PROFILE,
            NO_WORKSPACE_ID,
            &unhex(EMPTY_WORKSPACE_HEX)
        )
        .is_err()
    );
}

#[test]
fn jj_operation_hash_preserves_unrecorded_and_recorded_empty_predecessors() {
    let none = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        NO_PREDECESSORS_ID,
        &unhex(NO_PREDECESSORS_HEX),
    )
    .unwrap();
    let empty = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        EMPTY_PREDECESSORS_ID,
        &unhex(EMPTY_PREDECESSORS_HEX),
    )
    .unwrap();
    assert_eq!(none.commit_predecessors, None);
    assert_eq!(empty.commit_predecessors, Some(BTreeMap::new()));
    assert_ne!(none.operation_id, empty.operation_id);
}

#[test]
fn jj_operation_absent_and_explicit_default_metadata_have_same_semantics() {
    assert_eq!(ABSENT_METADATA_ID, NO_PREDECESSORS_ID);
    let absent = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        ABSENT_METADATA_ID,
        &unhex(ABSENT_METADATA_HEX),
    )
    .unwrap();
    let explicit = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        NO_PREDECESSORS_ID,
        &unhex(NO_PREDECESSORS_HEX),
    )
    .unwrap();
    assert_eq!(absent.workspace_name, explicit.workspace_name);
    assert_eq!(absent.is_snapshot, explicit.is_snapshot);
    assert!(!absent.is_snapshot);
}

#[test]
fn jj_operation_signed_timestamp_and_int32_timezone_follow_jj_cast_semantics() {
    let mut metadata = rich_metadata_fields();
    metadata[1] = bytes_field(
        2,
        &[scalar(1, 123456800), scalar(2, (1_u64 << 32) + 330)].concat(),
    );
    let raw = operation(
        Some(&metadata.concat()),
        &[vec![0x22; 64], vec![0x33; 64]],
        &rich_predecessors(),
        Some(1),
    );
    decode_operation(JJ_OBSERVATION_READER_PROFILE, RICH_ID, &raw).unwrap();
}

#[test]
fn jj_operation_vector_order_is_hash_significant() {
    let raw = operation(
        Some(&rich_metadata()),
        &[vec![0x33; 64], vec![0x22; 64]],
        &rich_predecessors(),
        Some(1),
    );
    reject(&raw, "hash");
    let mut predecessors = rich_predecessors();
    predecessors[1] = predecessor(&[0xaa; 20], &[vec![0xcc; 20], vec![0xbb; 20]]);
    reject(
        &operation(
            Some(&rich_metadata()),
            &[vec![0x22; 64], vec![0x33; 64]],
            &predecessors,
            Some(1),
        ),
        "hash",
    );
}

#[test]
fn jj_operation_does_not_silently_deduplicate_predecessor_vector() {
    let decoded = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        DUPLICATE_PREDECESSOR_EDGE_ID,
        &unhex(DUPLICATE_PREDECESSOR_EDGE_HEX),
    )
    .unwrap();
    assert_eq!(
        decoded.commit_predecessors.unwrap()[&"aa".repeat(20)],
        ["bb".repeat(20), "bb".repeat(20)]
    );
}

#[test]
fn jj_operation_requires_exact_profile_and_full_lowercase_expected_identity() {
    let raw = unhex(RICH_HEX);
    for profile in [
        "",
        "jj-simple-op-store/0.45.0",
        "jj-simple-op-store/0.45.1 ",
    ] {
        let error = decode_operation(profile, RICH_ID, &raw).unwrap_err();
        assert!(error.to_string().contains("profile"));
    }
    for id in [
        RICH_ID[..12].to_owned(),
        RICH_ID.to_uppercase(),
        format!(" {RICH_ID}"),
        "g".repeat(128),
        format!("{RICH_ID}00"),
    ] {
        let error = decode_operation(JJ_OBSERVATION_READER_PROFILE, &id, &raw).unwrap_err();
        assert!(error.to_string().contains("identity"));
    }
    let error =
        decode_operation(JJ_OBSERVATION_READER_PROFILE, &"00".repeat(64), &raw).unwrap_err();
    assert!(error.to_string().contains("root"));
    let error =
        decode_operation(JJ_OBSERVATION_READER_PROFILE, &"ee".repeat(64), &raw).unwrap_err();
    assert!(error.to_string().contains("hash"));
}

#[test]
fn jj_operation_accepts_virtual_root_parent_but_rejects_legacy_parentless_records() {
    let decoded = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        ROOT_PARENT_ID,
        &unhex(ROOT_PARENT_HEX),
    )
    .unwrap();
    assert_eq!(decoded.parent_ids, ["00".repeat(64)]);
    reject(&operation(None, &[], &[], None), "parent");
    reject(
        &operation(None, &[vec![0x22; 64], vec![0x22; 64]], &[], None),
        "duplicate",
    );
}

#[test]
fn jj_operation_rejects_wrong_operation_view_and_sha1_commit_id_lengths() {
    for length in [0, 19, 20, 63, 65] {
        reject(
            &[
                bytes_field(1, &vec![0x11; length]),
                bytes_field(2, &[0x22; 64]),
            ]
            .concat(),
            "identity",
        );
        reject(
            &operation(None, &[vec![0x22; length]], &[], None),
            "identity",
        );
    }
    for length in [0, 19, 21, 32] {
        reject(
            &operation(
                None,
                &[vec![0x22; 64]],
                &[predecessor(&vec![0xaa; length], &[])],
                Some(1),
            ),
            "identity",
        );
        reject(
            &operation(
                None,
                &[vec![0x22; 64]],
                &[predecessor(&[0xaa; 20], &[vec![0xbb; length]])],
                Some(1),
            ),
            "identity",
        );
    }
}

#[test]
fn jj_operation_rejects_unknown_fields_and_wrong_wire_types_at_each_level() {
    for extra in [
        bytes_field(6, b"unknown"),
        scalar(1, 1),
        vec![0],
        vec![0x0b],
    ] {
        reject(&[unhex(RICH_HEX), extra].concat(), "field");
    }
    for metadata in [
        bytes_field(9, b"unknown"),
        scalar(3, 1),
        bytes_field(1, &bytes_field(1, b"wrong")),
        bytes_field(1, &scalar(3, 0)),
        bytes_field(6, &bytes_field(3, b"unknown")),
        bytes_field(6, &scalar(1, 1)),
    ] {
        reject(&base(Some(&metadata)), "field");
    }
    for entry in [bytes_field(3, b"unknown"), scalar(1, 1)] {
        reject(
            &operation(None, &[vec![0x22; 64]], &[entry], Some(1)),
            "field",
        );
    }
}

#[test]
fn jj_operation_rejects_duplicate_singular_and_map_fields() {
    for extra in [
        bytes_field(1, &[0x11; 64]),
        bytes_field(3, &[]),
        scalar(5, 1),
    ] {
        reject(&[unhex(RICH_HEX), extra].concat(), "duplicate");
    }
    for tag in [1, 2, 3, 4, 5, 8] {
        reject(
            &base(Some(
                &[bytes_field(tag, &[]), bytes_field(tag, &[])].concat(),
            )),
            "duplicate",
        );
    }
    reject(
        &base(Some(&[scalar(7, 0), scalar(7, 1)].concat())),
        "duplicate",
    );
    for tag in [1, 2] {
        reject(
            &base(Some(&bytes_field(
                1,
                &[scalar(tag, 0), scalar(tag, 1)].concat(),
            ))),
            "duplicate",
        );
        reject(
            &base(Some(&bytes_field(
                6,
                &[bytes_field(tag, b"x"), bytes_field(tag, b"x")].concat(),
            ))),
            "duplicate",
        );
    }
    reject(
        &base(Some(
            &[attribute(b"key", b"one"), attribute(b"key", b"two")].concat(),
        )),
        "duplicate",
    );
    let entry = predecessor(&[0xaa; 20], &[]);
    reject(
        &operation(None, &[vec![0x22; 64]], &[entry.clone(), entry], Some(1)),
        "duplicate",
    );
    reject(
        &operation(
            None,
            &[vec![0x22; 64]],
            &[[bytes_field(1, &[0xaa; 20]), bytes_field(1, &[0xaa; 20])].concat()],
            Some(1),
        ),
        "duplicate",
    );
}

#[test]
fn jj_operation_rejects_invalid_bools_and_inconsistent_predecessor_presence() {
    reject(&base(Some(&scalar(7, 2))), "bool");
    reject(&operation(None, &[vec![0x22; 64]], &[], Some(2)), "bool");
    for flag in [None, Some(0)] {
        reject(
            &operation(
                None,
                &[vec![0x22; 64]],
                &[predecessor(&[0xaa; 20], &[])],
                flag,
            ),
            "predecessor",
        );
    }
}

#[test]
fn jj_operation_rejects_invalid_utf8_in_all_metadata_strings() {
    for tag in [3, 4, 5, 8] {
        reject(&base(Some(&bytes_field(tag, &[0xff]))), "utf");
    }
    reject(&base(Some(&attribute(&[0xff], b"value"))), "utf");
    reject(&base(Some(&attribute(b"key", &[0xff]))), "utf");
}

#[test]
fn jj_operation_rejects_truncated_and_overflowing_varints_without_panicking() {
    for malformed in [
        vec![0x80],
        vec![0x80; 11],
        [vec![0x80; 9], vec![0x02]].concat(),
        vec![0x0a, 0x80],
    ] {
        reject(&malformed, "varint");
    }
    reject(&[vec![0x0a, 64], vec![0x11; 63]].concat(), "truncated");
    let raw = unhex(RICH_HEX);
    for end in 0..raw.len() {
        assert!(
            decode_operation(JJ_OBSERVATION_READER_PROFILE, RICH_ID, &raw[..end]).is_err(),
            "accepted truncated prefix {end}"
        );
    }
}
