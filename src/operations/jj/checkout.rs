//! Bounded checkout context decoding for the pinned jj simple-op-store profile.
//!
//! Checkout fields follow jj v0.45.1 lib/src/protos/local_working_copy.proto
//! and CheckoutState in lib/src/local_working_copy.rs (Apache-2.0). These bytes
//! carry no content checksum; decoding does not verify the referenced operation,
//! completed filesystem state, ancestry, journal admission, or attribution.

use super::content_hash::hex;
use super::wire::{Fields, WireError, identity, singular};
use crate::model::jj_observation::{JJ_OBSERVATION_SCHEMA_VERSION, is_root, validate_profile};
use std::fmt;

pub const MAX_CHECKOUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedJjCheckout {
    pub operation_id: String,
    pub workspace_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjCheckoutDecodeError(&'static str);

impl fmt::Display for JjCheckoutDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "jj checkout {}", self.0)
    }
}

impl std::error::Error for JjCheckoutDecodeError {}

impl From<WireError> for JjCheckoutDecodeError {
    fn from(error: WireError) -> Self {
        Self(error.0)
    }
}

/// Decodes supplied checkout bytes without filesystem access or subprocesses.
/// The caller owns bounded file reads and rechecking this mutable metadata.
pub fn decode_checkout(
    profile: &str,
    raw: &[u8],
) -> Result<DecodedJjCheckout, JjCheckoutDecodeError> {
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, profile)
        .map_err(|_| JjCheckoutDecodeError("unsupported reader profile"))?;
    if raw.len() > MAX_CHECKOUT_BYTES {
        return Err(JjCheckoutDecodeError("raw byte limit exceeded"));
    }
    let mut fields = Fields::new(raw);
    let mut operation_id = None;
    let mut workspace_name = None;
    while let Some(field) = fields.next(&[(2, 2), (3, 2)])? {
        match field.tag {
            2 => singular(&mut operation_id, identity(field.bytes()?, 64)?)?,
            3 => singular(&mut workspace_name, field.bytes()?)?,
            _ => return Err(JjCheckoutDecodeError("unknown checkout field")),
        }
    }
    let operation_id = operation_id.ok_or(JjCheckoutDecodeError("missing operation identity"))?;
    let workspace_name = workspace_name.ok_or(JjCheckoutDecodeError("missing workspace name"))?;
    let workspace_name = std::str::from_utf8(workspace_name)
        .map_err(|_| JjCheckoutDecodeError("invalid utf8 workspace name"))?;
    if workspace_name.is_empty() {
        // jj repairs legacy empty names to "default"; this profile requires explicit context.
        return Err(JjCheckoutDecodeError(
            "unsupported legacy empty workspace name",
        ));
    }
    let operation_id = hex(operation_id);
    if is_root(&operation_id) {
        return Err(JjCheckoutDecodeError(
            "virtual root is not a checkout operation",
        ));
    }
    Ok(DecodedJjCheckout {
        operation_id,
        workspace_name: workspace_name.to_owned(),
    })
}
