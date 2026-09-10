use super::*;
use sha2::{Digest, Sha256};

const VALID: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-registration-records/linux_workspace.cbor"
));
const MAX_NAME: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-registration-records/max_workspace.cbor"
));
const EMPTY_NAME: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-registration-records/empty_name.cbor"
));
const OVER_NAME: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-registration-records/over_workspace_name.cbor"
));

fn decodes(raw: &[u8]) -> bool {
    let checksum = format!("{:x}", Sha256::digest(raw));
    decode_workspace(raw, raw.len() as u64, &checksum).is_ok()
}

#[test]
fn workspace_record_rejects_empty_name_without_sql_key_comparison() {
    assert!(decodes(VALID));
    assert!(!decodes(EMPTY_NAME));
}

#[test]
fn workspace_record_name_limit_counts_utf8_bytes_without_sql_key_comparison() {
    assert!(decodes(MAX_NAME));
    assert!(!decodes(OVER_NAME));
}
