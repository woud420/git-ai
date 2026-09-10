use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::model::jj_observation::JJ_OBSERVATION_READER_PROFILE;
use git_ai::operations::jj::checkout::{
    DecodedJjCheckout, JjCheckoutDecodeError, MAX_CHECKOUT_BYTES, decode_checkout,
};

#[path = "jj_checkout_real.rs"]
mod real;

use crate::jj_operation::support::{bytes_field, scalar};

fn checkout(operation_id: &[u8], workspace: &[u8]) -> Vec<u8> {
    [bytes_field(2, operation_id), bytes_field(3, workspace)].concat()
}

fn decode(raw: &[u8]) -> Result<DecodedJjCheckout, JjCheckoutDecodeError> {
    decode_checkout(JJ_OBSERVATION_READER_PROFILE, raw)
}

fn reject(raw: &[u8], category: &str) {
    let error = decode(raw).unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains(category),
        "expected {category:?}, received {error}"
    );
}

#[test]
fn jj_checkout_decodes_completed_workspace_context_from_test_repo_without_writes() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo.path().join("synthetic-checkout");
    let raw = checkout(&[0x11; 64], "linked-工".as_bytes());
    std::fs::write(&path, &raw).unwrap();
    let before = crate::debug_context::snapshot(repo.path());
    let decoded = decode(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(decoded.operation_id, "11".repeat(64));
    assert_eq!(decoded.workspace_name, "linked-工");
    assert_eq!(std::fs::read(path).unwrap(), raw);
    assert_eq!(crate::debug_context::snapshot(repo.path()), before);
}

#[test]
fn jj_checkout_field_order_does_not_change_context() {
    let normal = checkout(&[0x23; 64], b"default");
    let reversed = [bytes_field(3, b"default"), bytes_field(2, &[0x23; 64])].concat();
    assert_ne!(normal, reversed);
    assert_eq!(decode(&normal).unwrap(), decode(&reversed).unwrap());
}

#[test]
fn jj_checkout_preserves_nonempty_workspace_names_without_trimming_or_normalizing() {
    for workspace in ["default", " linked workspace ", "space-工", "e\u{301}", "é"] {
        let decoded = decode(&checkout(&[0x22; 64], workspace.as_bytes())).unwrap();
        assert_eq!(decoded.workspace_name, workspace);
    }
    assert_ne!(
        decode(&checkout(&[0x22; 64], "e\u{301}".as_bytes())).unwrap(),
        decode(&checkout(&[0x22; 64], "é".as_bytes())).unwrap()
    );
}

#[test]
fn jj_checkout_requires_exact_reader_profile() {
    let raw = checkout(&[0x11; 64], b"default");
    for profile in [
        "",
        "jj-simple-op-store/0.45.0",
        "jj-simple-op-store/0.45.1 ",
    ] {
        assert!(
            decode_checkout(profile, &raw)
                .unwrap_err()
                .to_string()
                .contains("profile")
        );
    }
}

#[test]
fn jj_checkout_requires_a_full_nonroot_operation_identity() {
    for length in [0, 20, 32, 63, 65, 128] {
        reject(&checkout(&vec![0x11; length], b"default"), "identity");
    }
    reject(&bytes_field(3, b"default"), "identity");
    reject(&checkout(&[0; 64], b"default"), "root");
    let mut leading_zero = [0; 64];
    leading_zero[63] = 1;
    assert_eq!(
        decode(&checkout(&leading_zero, b"default"))
            .unwrap()
            .operation_id,
        format!("{}01", "00".repeat(63))
    );
}

#[test]
fn jj_checkout_rejects_legacy_missing_or_empty_workspace_fallback() {
    reject(&bytes_field(2, &[0x11; 64]), "workspace");
    reject(&checkout(&[0x11; 64], b""), "workspace");
    assert!(decode(&[]).is_err());
}

#[test]
fn jj_checkout_rejects_invalid_utf8_workspace_names() {
    for workspace in [
        vec![0xff],
        vec![0xc3],
        vec![0xc0, 0x80],
        vec![0xed, 0xa0, 0x80],
    ] {
        reject(&checkout(&[0x11; 64], &workspace), "utf");
    }
}

#[test]
fn jj_checkout_rejects_duplicate_fields_even_when_values_are_identical() {
    let raw = checkout(&[0x11; 64], b"default");
    for extra in [
        bytes_field(2, &[0x11; 64]),
        bytes_field(2, &[0x22; 64]),
        bytes_field(3, b"default"),
        bytes_field(3, b"different"),
    ] {
        reject(&[raw.clone(), extra].concat(), "duplicate");
    }
}

#[test]
fn jj_checkout_rejects_reserved_unknown_and_wrong_wire_fields() {
    let raw = checkout(&[0x11; 64], b"default");
    for extra in [
        vec![0],
        bytes_field(1, b"reserved"),
        bytes_field(4, b"future"),
        scalar(2, 0),
        scalar(3, 0),
        vec![0x11],
        vec![0x15],
        vec![0x13],
        vec![0x14],
    ] {
        reject(&[raw.clone(), extra].concat(), "field");
    }
}

#[test]
fn jj_checkout_rejects_truncated_and_overflowing_varints_before_allocation() {
    for malformed in [
        vec![0x80],
        vec![0x80; 11],
        [vec![0x80; 9], vec![0x02]].concat(),
        vec![0x12, 0x80],
        [vec![0x12], vec![0x80; 9], vec![0x02]].concat(),
    ] {
        reject(&malformed, "varint");
    }
    let mut huge_length = vec![0x12];
    huge_length.extend([0xff; 9]);
    huge_length.push(1);
    reject(&huge_length, "truncated");
}

#[test]
fn jj_checkout_rejects_every_incomplete_prefix_and_trailing_partial_field() {
    let raw = checkout(&[0x11; 64], "linked-工".as_bytes());
    for end in 0..raw.len() {
        assert!(
            decode(&raw[..end]).is_err(),
            "accepted incomplete prefix {end}"
        );
    }
    for extra in [vec![0x12], vec![0x1a], vec![0x80]] {
        assert!(decode(&[raw.clone(), extra].concat()).is_err());
    }
}

#[test]
fn jj_checkout_raw_byte_limit_is_inclusive_for_ascii_and_utf8_names() {
    assert_eq!(MAX_CHECKOUT_BYTES, 16 * 1024);
    // Field 2 takes 66 bytes; a name near 16 KiB has a one-byte key and two-byte length.
    let maximum_name_bytes = MAX_CHECKOUT_BYTES - bytes_field(2, &[0x11; 64]).len() - 3;
    let ascii = vec![b'x'; maximum_name_bytes];
    let raw = checkout(&[0x11; 64], &ascii);
    assert_eq!(raw.len(), MAX_CHECKOUT_BYTES);
    assert_eq!(decode(&raw).unwrap().workspace_name.as_bytes(), ascii);
    let mut unicode = vec![b'x'; maximum_name_bytes - 2];
    unicode.extend("é".as_bytes());
    let raw = checkout(&[0x11; 64], &unicode);
    assert_eq!(raw.len(), MAX_CHECKOUT_BYTES);
    assert_eq!(decode(&raw).unwrap().workspace_name.as_bytes(), unicode);
    unicode.insert(0, b'x');
    reject(&checkout(&[0x11; 64], &unicode), "limit");
    reject(&vec![0; MAX_CHECKOUT_BYTES + 1], "limit");
}
