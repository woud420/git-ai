use super::RawOperation;
use super::metadata::Timestamp;
use blake2::{Blake2b512, Digest};

pub(super) fn operation_id(operation: &RawOperation<'_>) -> String {
    let mut hash = Blake2b512::new();
    blob(&mut hash, operation.view_id);
    sequence(&mut hash, &operation.parent_ids);
    let metadata = &operation.metadata;
    timestamp(&mut hash, &metadata.start);
    timestamp(&mut hash, &metadata.end);
    blob(&mut hash, metadata.description.as_bytes());
    blob(&mut hash, metadata.hostname.as_bytes());
    blob(&mut hash, metadata.username.as_bytes());
    hash.update([u8::from(metadata.is_snapshot)]);
    match metadata.workspace_name {
        None => hash.update(0u32.to_le_bytes()),
        Some(workspace) => {
            hash.update(1u32.to_le_bytes());
            blob(&mut hash, workspace.as_bytes());
        }
    }
    count(&mut hash, metadata.attributes.len());
    for (key, value) in &metadata.attributes {
        blob(&mut hash, key.as_bytes());
        blob(&mut hash, value.as_bytes());
    }
    match &operation.predecessors {
        None => hash.update(0u32.to_le_bytes()),
        Some(predecessors) => {
            hash.update(1u32.to_le_bytes());
            count(&mut hash, predecessors.len());
            for (commit, edges) in predecessors {
                blob(&mut hash, commit);
                sequence(&mut hash, edges);
            }
        }
    }
    hex(&hash.finalize())
}

fn blob(hash: &mut Blake2b512, bytes: &[u8]) {
    count(hash, bytes.len());
    hash.update(bytes);
}

fn count(hash: &mut Blake2b512, count: usize) {
    hash.update((count as u64).to_le_bytes());
}

fn sequence(hash: &mut Blake2b512, values: &[&[u8]]) {
    count(hash, values.len());
    for value in values {
        blob(hash, value);
    }
}

fn timestamp(hash: &mut Blake2b512, timestamp: &Timestamp) {
    hash.update(timestamp.millis.to_le_bytes());
    hash.update(timestamp.tz_offset.to_le_bytes());
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    value
}
