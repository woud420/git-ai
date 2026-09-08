use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::model::jj_observation::JJ_OBSERVATION_READER_PROFILE;
use git_ai::operations::jj::view::{
    DecodedJjView, JjViewDecodeError, MAX_VIEW_BYTES, MAX_VIEW_COMMIT_REFERENCES,
    MAX_VIEW_NAME_BYTES, MAX_VIEW_WIRE_ENTRIES, decode_view,
};
use std::collections::BTreeMap;

#[path = "jj_view_compatibility.rs"]
mod compatibility;
#[path = "jj_view_limits.rs"]
mod limits;
#[path = "jj_view_malformed.rs"]
mod malformed;
#[path = "jj_view_real.rs"]
mod real;
#[path = "jj_view_support.rs"]
mod support;
#[path = "jj_view_vectors.rs"]
mod vectors;

use support::*;
use vectors::*;

fn decode(id: &str, raw: &[u8]) -> Result<DecodedJjView, JjViewDecodeError> {
    decode_view(JJ_OBSERVATION_READER_PROFILE, id, raw)
}

fn reject(raw: &[u8], expected: &str) {
    let error = decode(RICH_ID, raw).unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains(expected),
        "expected {expected:?} error, received {error}"
    );
}

#[test]
fn jj_view_decodes_independent_domain_vector_from_test_repo_without_writes() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let fixture = repo.path().join("synthetic-view");
    let raw = unhex(RICH_HEX);
    assert_eq!(rich(), raw);
    std::fs::write(&fixture, &raw).unwrap();
    let decoded = decode(RICH_ID, &std::fs::read(&fixture).unwrap()).unwrap();
    assert_eq!(decoded.view_id, RICH_ID);
    assert_eq!(decoded.head_ids, ["aa".repeat(20), "bb".repeat(20)]);
    assert_eq!(
        decoded.wc_commit_ids,
        BTreeMap::from([
            ("default".to_owned(), "bb".repeat(20)),
            ("workspace-工".to_owned(), "cc".repeat(20)),
        ])
    );
    assert_eq!(decoded.commit_references, 15);
    assert_eq!(std::fs::read(fixture).unwrap(), raw);
}

#[test]
fn jj_view_normalizes_wire_map_and_head_order_before_hashing() {
    let mut entries = rich_entries();
    entries.reverse();
    let mut raw = scalar(12, 1);
    raw.extend(entries.concat());
    raw.extend(bytes_field(1, &[0xaa; 20]));
    raw.extend(bytes_field(1, &[0xbb; 20]));
    assert_ne!(raw, rich());
    assert_eq!(
        decode(RICH_ID, &raw).unwrap(),
        decode(RICH_ID, &rich()).unwrap()
    );
}

#[test]
fn jj_view_normalizes_nested_remote_maps_and_ref_wire_field_order() {
    let conflict = vec![Some(vec![0xaa; 20]), None, Some(vec![0xbb; 20])];
    let mut entries = rich_entries();
    entries[6] = remote_view(
        b"origin",
        &[
            remote_ref(b"remoteonly", &[Some(vec![0xcc; 20])], Some(0)),
            remote_ref(b"conflict", &conflict, Some(1)),
        ],
        &[
            remote_ref(b"tombstone", &[None], Some(0)),
            remote_ref(b"release", &[Some(vec![0xbb; 20])], Some(1)),
        ],
    );
    let mixed_wire_conflict = bytes_field(
        3,
        &[
            bytes_field(2, &term(Some(&[0xaa; 20]))),
            bytes_field(1, &term(None)),
            bytes_field(2, &term(Some(&[0xbb; 20]))),
        ]
        .concat(),
    );
    entries[0] = bytes_field(
        5,
        &[
            bytes_field(
                3,
                &legacy_remote(b"origin", Some(&mixed_wire_conflict), Some(1)),
            ),
            bytes_field(2, &mixed_wire_conflict),
            bytes_field(1, b"conflict"),
        ]
        .concat(),
    );
    assert_eq!(
        decode(RICH_ID, &view(&[vec![0xaa; 20], vec![0xbb; 20]], &entries)).unwrap(),
        decode(RICH_ID, &rich()).unwrap()
    );
}

#[test]
fn jj_view_hash_covers_each_domain_collection_and_remote_state() {
    let replacements = [
        (
            0,
            bookmark(
                b"conflict",
                Some(&normal(0xcc)),
                &[legacy_remote(
                    b"origin",
                    Some(&target(&[Some(vec![0xaa; 20]), None, Some(vec![0xbb; 20])])),
                    Some(1),
                )],
            ),
        ),
        (5, named_target(6, b"v1", Some(&normal(0xbb)))),
        (
            7,
            remote_view(
                b"tagonly",
                &[],
                &[remote_ref(b"t", &[Some(vec![0xcc; 20])], Some(0))],
            ),
        ),
        (8, named_target(3, b"refs/heads/main", Some(&normal(0xbb)))),
        (
            11,
            named_target(13, "workspace-工".as_bytes(), Some(&normal(0xcc))),
        ),
        (12, workspace(b"default", &[0xaa; 20])),
    ];
    for (index, replacement) in replacements {
        let mut entries = rich_entries();
        entries[index] = replacement;
        reject(&view(&[vec![0xbb; 20], vec![0xaa; 20]], &entries), "hash");
    }
    reject(
        &view(&[vec![0xbb; 20], vec![0xcc; 20]], &rich_entries()),
        "hash",
    );
    assert_ne!(REMOTE_NEW_ID, REMOTE_TRACKED_ID);
    decode(REMOTE_NEW_ID, &unhex(REMOTE_NEW_HEX)).unwrap();
    decode(REMOTE_TRACKED_ID, &unhex(REMOTE_TRACKED_HEX)).unwrap();
}

#[test]
fn jj_view_preserves_conflict_order_and_repeated_terms() {
    let mut entries = rich_entries();
    let reversed = target(&[Some(vec![0xbb; 20]), None, Some(vec![0xaa; 20])]);
    entries[0] = bookmark(
        b"conflict",
        Some(&reversed),
        &[legacy_remote(
            b"origin",
            Some(&target(&[Some(vec![0xaa; 20]), None, Some(vec![0xbb; 20])])),
            Some(1),
        )],
    );
    reject(&view(&[vec![0xbb; 20], vec![0xaa; 20]], &entries), "hash");
    let decoded = decode(REPEATED_TERMS_ID, &unhex(REPEATED_TERMS_HEX)).unwrap();
    assert_eq!(decoded.commit_references, 4);
    assert!(
        decode(
            REPEATED_TERMS_ID,
            &base(&[named_target(6, b"t", Some(&normal(0xaa))),])
        )
        .is_err()
    );
}

#[test]
fn jj_view_preserves_absent_targets_and_zero_commit_distinctions() {
    assert_eq!(ABSENT_LOCAL_ID, MINIMAL_ID);
    assert_eq!(
        decode(ABSENT_LOCAL_ID, &unhex(ABSENT_LOCAL_HEX))
            .unwrap()
            .commit_references,
        1
    );
    assert_ne!(ABSENT_TAG_ID, MINIMAL_ID);
    assert_eq!(
        decode(ABSENT_TAG_ID, &unhex(ABSENT_TAG_HEX))
            .unwrap()
            .commit_references,
        1
    );
    assert_ne!(ZERO_COMMIT_TAG_ID, ABSENT_TAG_ID);
    assert_eq!(
        decode(ZERO_COMMIT_TAG_ID, &unhex(ZERO_COMMIT_TAG_HEX))
            .unwrap()
            .commit_references,
        2
    );
    let root = decode(ROOT_COMMIT_ID, &unhex(ROOT_COMMIT_HEX)).unwrap();
    assert_eq!(root.head_ids, ["00".repeat(20)]);
}

#[test]
fn jj_view_retains_empty_remote_and_workspace_maps() {
    assert_ne!(EMPTY_REMOTE_ID, MINIMAL_ID);
    assert_eq!(
        decode(EMPTY_REMOTE_ID, &unhex(EMPTY_REMOTE_HEX))
            .unwrap()
            .commit_references,
        1
    );
    let decoded = decode(EMPTY_WORKSPACE_ID, &unhex(EMPTY_WORKSPACE_HEX)).unwrap();
    assert_eq!(
        decoded.wc_commit_ids,
        BTreeMap::from([("".to_owned(), "aa".repeat(20))])
    );
    assert_eq!(decoded.commit_references, 2);
}

#[test]
fn jj_view_requires_exact_profile_full_lowercase_identity_and_nonroot_file() {
    let raw = unhex(MINIMAL_HEX);
    for profile in [
        "",
        "jj-simple-op-store/0.45.0",
        "jj-simple-op-store/0.45.1 ",
    ] {
        assert!(
            decode_view(profile, MINIMAL_ID, &raw)
                .unwrap_err()
                .to_string()
                .contains("profile")
        );
    }
    for id in [
        MINIMAL_ID[..12].to_owned(),
        MINIMAL_ID.to_uppercase(),
        format!(" {MINIMAL_ID}"),
        "g".repeat(128),
        format!("{MINIMAL_ID}00"),
    ] {
        assert!(
            decode(&id, &raw)
                .unwrap_err()
                .to_string()
                .contains("identity")
        );
    }
    assert!(
        decode(&"00".repeat(64), &raw)
            .unwrap_err()
            .to_string()
            .contains("root")
    );
    assert!(
        decode(&"ee".repeat(64), &raw)
            .unwrap_err()
            .to_string()
            .contains("hash")
    );
}
