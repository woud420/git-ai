//! Typed Cap'n Proto codec for live CLI-to-daemon checkpoint delivery.

use crate::error::GitAiError;
use crate::model::checkpoint_delivery::CheckpointDelivery;

mod decode;
mod encode;
mod enums;

pub const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;

pub fn encode_delivery(delivery: &CheckpointDelivery) -> Result<Vec<u8>, GitAiError> {
    delivery
        .validate()
        .map_err(|error| GitAiError::PresetError(error.to_string()))?;
    let message = encode::build_message(delivery).map_err(invalid_payload)?;
    if capnp::serialize::compute_serialized_size_in_words(&message) > MAX_ENCODED_BYTES / 8 {
        return Err(invalid_payload("encoded message exceeds size limit"));
    }
    Ok(capnp::serialize::write_message_to_words(&message))
}

pub fn decode_delivery(bytes: &[u8]) -> Result<CheckpointDelivery, GitAiError> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(invalid_payload("encoded message exceeds size limit"));
    }
    // Charge repeated pointer traversals against the actual frame size, so
    // aliased input cannot expand arbitrarily when converted into owned DTOs.
    let options = capnp::message::ReaderOptions {
        traversal_limit_in_words: Some(bytes.len().div_ceil(8)),
        nesting_limit: 16,
    };
    let mut remaining = bytes;
    let message = capnp::serialize::read_message_from_flat_slice(&mut remaining, options)
        .map_err(invalid_payload)?;
    if !remaining.is_empty() {
        return Err(invalid_payload("trailing bytes after checkpoint message"));
    }
    let root = message
        .get_root::<crate::checkpoint_capnp::checkpoint_delivery::Reader<'_>>()
        .map_err(invalid_payload)?;
    let delivery = decode::read_delivery(root).map_err(invalid_payload)?;
    delivery
        .validate()
        .map_err(|error| GitAiError::PresetError(error.to_string()))?;
    Ok(delivery)
}

fn invalid_payload(error: impl std::fmt::Display) -> GitAiError {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("invalid checkpoint Cap'n Proto payload: {error}"),
    )
    .into()
}

#[cfg(test)]
mod tests;
