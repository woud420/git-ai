//! Bounded verification of immutable views in the pinned jj simple-op-store profile.
//!
//! Definitions follow jj v0.45.1 lib/src/{op_store,simple_op_store}.rs,
//! lib/src/protos/simple_op_store.proto, and core/src/{content_hash,merge}.rs
//! (Apache-2.0). Compatibility mirrors are checked but excluded from domain hashing.

mod entries;
mod hash;
mod parse;
mod refs;
mod types;

use super::content_hash::hex;
use super::wire::WireError;
use crate::model::jj_observation::{
    JJ_OBSERVATION_SCHEMA_VERSION, is_root, validate_operation_id, validate_profile,
};
use std::collections::BTreeMap;
use std::fmt;

pub const MAX_VIEW_BYTES: usize = 2 * 1024 * 1024;
/// Normalized heads, workspace values, and non-null terms in authoritative refs.
pub const MAX_VIEW_COMMIT_REFERENCES: usize = 4096;
/// UTF-8 bytes in every on-wire name occurrence, including compatibility mirrors.
pub const MAX_VIEW_NAME_BYTES: usize = 64 * 1024;
/// Repeated/map messages and explicit terms, including mirrors; excludes head bytes.
pub const MAX_VIEW_WIRE_ENTRIES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedJjView {
    pub view_id: String,
    pub head_ids: Vec<String>,
    pub wc_commit_ids: BTreeMap<String, String>,
    pub commit_references: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjViewDecodeError(&'static str);

impl fmt::Display for JjViewDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "jj view {}", self.0)
    }
}

impl std::error::Error for JjViewDecodeError {}

impl From<WireError> for JjViewDecodeError {
    fn from(error: WireError) -> Self {
        Self(error.0)
    }
}

/// Verifies one view's semantic hash without filesystem access or subprocesses.
/// Success does not establish ancestry, current checkout state, journal admission,
/// attribution, or the version of the binary that wrote these bytes.
pub fn decode_view(
    profile: &str,
    expected_id: &str,
    raw: &[u8],
) -> Result<DecodedJjView, JjViewDecodeError> {
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, profile)
        .map_err(|_| JjViewDecodeError("unsupported reader profile"))?;
    validate_operation_id(expected_id)
        .map_err(|_| JjViewDecodeError("invalid expected view identity"))?;
    if is_root(expected_id) {
        return Err(JjViewDecodeError("virtual root has no view file"));
    }
    if raw.len() > MAX_VIEW_BYTES {
        return Err(JjViewDecodeError("raw byte limit exceeded"));
    }
    let (view, commit_references) = parse::decode(raw)?;
    let view_id = hash::view_id(&view);
    if view_id != expected_id {
        return Err(JjViewDecodeError("semantic hash mismatch"));
    }
    Ok(DecodedJjView {
        view_id,
        head_ids: view.heads.into_iter().map(hex).collect(),
        wc_commit_ids: view
            .workspaces
            .into_iter()
            .map(|(name, commit)| (name.to_owned(), hex(commit)))
            .collect(),
        commit_references,
    })
}
