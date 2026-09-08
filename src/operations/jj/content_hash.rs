use blake2::{Blake2b512, Digest};

pub(super) fn blob(hash: &mut Blake2b512, bytes: &[u8]) {
    count(hash, bytes.len());
    hash.update(bytes);
}

pub(super) fn count(hash: &mut Blake2b512, count: usize) {
    hash.update((count as u64).to_le_bytes());
}

pub(super) fn sequence(hash: &mut Blake2b512, values: &[&[u8]]) {
    count(hash, values.len());
    for value in values {
        blob(hash, value);
    }
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
