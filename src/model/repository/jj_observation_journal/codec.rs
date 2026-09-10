use super::{JournalError, invalid};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::Write;

pub(super) fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn encode<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, JournalError> {
    let mut writer = CappedWriter {
        bytes: Vec::new(),
        limit,
    };
    ciborium::into_writer(value, &mut writer)
        .map_err(|_| invalid("encoded payload limit exceeded"))?;
    Ok(writer.bytes)
}

struct CappedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("jj observation payload limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn decode<T: DeserializeOwned>(
    bytes: &[u8],
    stored_length: u64,
    expected_checksum: &str,
    limit: usize,
) -> Result<T, JournalError> {
    if stored_length > limit as u64 || bytes.len() > limit {
        return Err(invalid("stored payload byte limit exceeded"));
    }
    if stored_length != bytes.len() as u64 || checksum(bytes) != expected_checksum {
        return Err(invalid("stored payload checksum mismatch"));
    }
    // Check declared CBOR lengths before serde can reserve from an untrusted
    // size hint. The stored writer uses only definite, shallow collections.
    let mut cursor = 0;
    validate_item(bytes, &mut cursor, 0)?;
    if cursor != bytes.len() {
        return Err(invalid("stored payload decode has trailing bytes"));
    }
    ciborium::from_reader(bytes).map_err(|_| invalid("stored payload decode failed"))
}

fn validate_item(bytes: &[u8], cursor: &mut usize, depth: usize) -> Result<(), JournalError> {
    if depth > 16 {
        return Err(invalid("stored payload nesting limit exceeded"));
    }
    let initial = *bytes
        .get(*cursor)
        .ok_or_else(|| invalid("stored payload decode truncated"))?;
    *cursor += 1;
    let major = initial >> 5;
    let argument = initial & 31;
    let length = match argument {
        0..=23 => u64::from(argument),
        24..=27 => {
            let width = 1usize << (argument - 24);
            let end = cursor
                .checked_add(width)
                .ok_or_else(|| invalid("stored payload length limit"))?;
            let encoded = bytes
                .get(*cursor..end)
                .ok_or_else(|| invalid("stored payload decode truncated"))?;
            *cursor = end;
            encoded
                .iter()
                .fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
        }
        _ => {
            return Err(invalid(
                "stored payload decode uses unsupported CBOR framing",
            ));
        }
    };
    match major {
        0 | 1 => Ok(()),
        2 | 3 => {
            let length =
                usize::try_from(length).map_err(|_| invalid("stored payload length limit"))?;
            if length > bytes.len().saturating_sub(*cursor) {
                return Err(invalid("stored payload decode truncated"));
            }
            *cursor += length;
            Ok(())
        }
        4 | 5 => {
            let count = if major == 5 {
                length.checked_mul(2)
            } else {
                Some(length)
            }
            .ok_or_else(|| invalid("stored payload collection limit"))?;
            if count > bytes.len().saturating_sub(*cursor) as u64 {
                return Err(invalid("stored payload collection limit exceeded"));
            }
            for _ in 0..count {
                validate_item(bytes, cursor, depth + 1)?;
            }
            Ok(())
        }
        7 if matches!(argument, 20..=22) => Ok(()),
        _ => Err(invalid("stored payload decode uses unsupported CBOR type")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksummed_extreme_length_and_deep_nesting_fail_before_deserialization() {
        let extreme = [0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert!(
            decode::<Vec<u8>>(&extreme, extreme.len() as u64, &checksum(&extreme), 1024).is_err()
        );
        let mut deep = vec![0x81; 18];
        deep.push(0);
        assert!(decode::<Vec<u8>>(&deep, deep.len() as u64, &checksum(&deep), 1024).is_err());
    }
}
