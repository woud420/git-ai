use super::{CheckpointJournalError, sha256_hex};
#[cfg(test)]
use crate::error::GitAiError;
use crate::model::working_log::Checkpoint;
#[cfg(test)]
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) const VERSION: u64 = 1;
pub(super) const VERSION_FIELD: &str = "_git_ai_record_version";
pub(super) const CHECKSUM_FIELD: &str = "_git_ai_record_checksum";
const CHECKSUM_MARKER: &[u8] = b",\"_git_ai_record_checksum\":\"";

#[cfg(test)]
#[derive(Serialize)]
struct UnsignedRecord<'a> {
    #[serde(flatten)]
    checkpoint: &'a Checkpoint,
    #[serde(rename = "_git_ai_record_version")]
    record_version: u64,
}

#[cfg(test)]
pub(super) fn encode(checkpoint: &Checkpoint) -> Result<Vec<u8>, GitAiError> {
    let unsigned = UnsignedRecord {
        checkpoint,
        record_version: VERSION,
    };
    let unsigned = serde_json_canonicalizer::to_vec(&unsigned)?;
    sign(unsigned).map_err(Into::into)
}

pub(super) fn decode(bytes: &[u8]) -> Result<Checkpoint, CheckpointJournalError> {
    let raw_checksum = if let Some((marker_start, supplied_checksum)) = terminal_checksum(bytes)? {
        if checksum_raw_record(bytes, marker_start) != supplied_checksum {
            return Err(CheckpointJournalError::Integrity(
                "checkpoint record checksum mismatch".to_string(),
            ));
        }
        Some(supplied_checksum)
    } else {
        None
    };

    let mut value: Value = serde_json::from_slice(bytes)?;
    let object = value.as_object_mut().ok_or_else(|| {
        CheckpointJournalError::Integrity("checkpoint record is not a JSON object".to_string())
    })?;
    let supplied_checksum = object
        .remove(CHECKSUM_FIELD)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| {
            CheckpointJournalError::Integrity(
                "checkpoint record checksum is missing or invalid".to_string(),
            )
        })?;
    let expected_checksum = match raw_checksum {
        Some(checksum) => checksum.to_string(),
        None => checksum_value(&value)?,
    };
    if supplied_checksum != expected_checksum {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record checksum mismatch".to_string(),
        ));
    }

    let object = value.as_object_mut().expect("object shape checked above");
    let record_version = object
        .remove(VERSION_FIELD)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| {
            CheckpointJournalError::Integrity(
                "checkpoint record version is missing or invalid".to_string(),
            )
        })?;
    if record_version != VERSION {
        return Err(CheckpointJournalError::Integrity(format!(
            "unsupported checkpoint record version {record_version}"
        )));
    }
    let mut checkpoint: Checkpoint = serde_json::from_value(value)?;
    checkpoint.mark_journal_record_version(record_version);
    Ok(checkpoint)
}

pub(super) fn has_terminal_checksum(bytes: &[u8]) -> Result<bool, CheckpointJournalError> {
    terminal_checksum(bytes).map(|checksum| checksum.is_some())
}

pub(super) fn claims_version(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key(VERSION_FIELD))
}

pub(super) fn claims_checksum(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key(CHECKSUM_FIELD))
}

pub(super) fn is_record(bytes: &[u8]) -> bool {
    terminal_checksum(bytes).is_ok_and(|checksum| checksum.is_some())
        || serde_json::from_slice::<Value>(bytes)
            .ok()
            .is_some_and(|value| claims_version(&value))
}

fn checksum_value(value: &Value) -> Result<String, serde_json::Error> {
    let canonical = serde_json_canonicalizer::to_vec(value)?;
    Ok(sha256_hex(&canonical))
}

#[cfg(test)]
fn sign(mut unsigned: Vec<u8>) -> Result<Vec<u8>, CheckpointJournalError> {
    let checksum = sha256_hex(&unsigned);
    if unsigned.pop() != Some(b'}') {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record is not a JSON object".to_string(),
        ));
    }
    unsigned.extend_from_slice(CHECKSUM_MARKER);
    unsigned.extend_from_slice(checksum.as_bytes());
    unsigned.extend_from_slice(b"\"}");
    Ok(unsigned)
}

fn terminal_checksum(bytes: &[u8]) -> Result<Option<(usize, &str)>, CheckpointJournalError> {
    let Some(marker_start) = bytes
        .windows(CHECKSUM_MARKER.len())
        .rposition(|window| window == CHECKSUM_MARKER)
    else {
        return Ok(None);
    };
    let checksum_start = marker_start + CHECKSUM_MARKER.len();
    let checksum_end = checksum_start + 64;
    if bytes.get(checksum_end..) != Some(b"\"}") {
        return Ok(None);
    }
    let supplied = &bytes[checksum_start..checksum_end];
    if !supplied.iter().all(u8::is_ascii_hexdigit) {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record checksum is not hexadecimal".to_string(),
        ));
    }

    let supplied = std::str::from_utf8(supplied).map_err(|_| {
        CheckpointJournalError::Integrity("checkpoint record checksum is not UTF-8".to_string())
    })?;
    Ok(Some((marker_start, supplied)))
}

fn checksum_raw_record(bytes: &[u8], marker_start: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(&bytes[..marker_start]);
    hasher.update(b"}");
    format!("{:x}", hasher.finalize())
}
