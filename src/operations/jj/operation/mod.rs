//! Strict, bounded operation-file decoding for the pinned jj simple-op-store profile.
//!
//! Success verifies the operation's semantic-domain hash. Referenced views,
//! integrated ancestry, journal admission, and attribution require later checks.
//! Field and hash definitions: jj v0.45.1 lib/src/{op_store,simple_op_store}.rs,
//! lib/src/protos/simple_op_store.proto, and core/src/content_hash.rs (Apache-2.0).

mod hash;
mod metadata;
use super::wire;

use super::content_hash::hex;
use crate::model::jj_observation::{
    JJ_OBSERVATION_SCHEMA_VERSION, is_root, validate_operation_id, validate_profile,
};
use metadata::Metadata;
use std::collections::BTreeMap;
use std::fmt;
use wire::{Budget, Fields, boolean, identity, singular};

pub const MAX_OPERATION_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_OPERATION_PARENTS: usize = 32;
/// Counts each predecessor-map key and every vector edge, including repeats.
pub const MAX_OPERATION_PREDECESSOR_REFERENCES: usize = 4096;
pub const MAX_OPERATION_ATTRIBUTES: usize = 256;
/// Aggregate UTF-8 bytes in all metadata strings, including attribute keys/values.
pub const MAX_OPERATION_METADATA_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedJjOperation {
    pub operation_id: String,
    pub view_id: String,
    pub parent_ids: Vec<String>,
    pub workspace_name: Option<String>,
    pub is_snapshot: bool,
    pub commit_predecessors: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjDecodeError(&'static str);

impl fmt::Display for JjDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "jj operation {}", self.0)
    }
}

impl std::error::Error for JjDecodeError {}

impl From<wire::WireError> for JjDecodeError {
    fn from(error: wire::WireError) -> Self {
        Self(error.0)
    }
}

type Predecessors<'a> = BTreeMap<&'a [u8], Vec<&'a [u8]>>;

struct RawOperation<'a> {
    view_id: &'a [u8],
    parent_ids: Vec<&'a [u8]>,
    metadata: Metadata<'a>,
    predecessors: Option<Predecessors<'a>>,
}

/// Validates one immutable operation without reading its view or repository.
/// The profile records a format assumption, not proof of the writing binary.
pub fn decode_operation(
    reader_profile: &str,
    expected_operation_id: &str,
    bytes: &[u8],
) -> Result<DecodedJjOperation, JjDecodeError> {
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, reader_profile)
        .map_err(|_| JjDecodeError("unsupported reader profile"))?;
    validate_operation_id(expected_operation_id)
        .map_err(|_| JjDecodeError("invalid expected operation identity"))?;
    if is_root(expected_operation_id) {
        return Err(JjDecodeError("virtual root has no operation file"));
    }
    if bytes.len() > MAX_OPERATION_BYTES {
        return Err(JjDecodeError("raw byte limit exceeded"));
    }
    let operation = parse(bytes)?;
    let operation_id = hash::operation_id(&operation);
    if operation_id != expected_operation_id {
        return Err(JjDecodeError("semantic hash mismatch"));
    }
    Ok(DecodedJjOperation {
        operation_id,
        view_id: hex(operation.view_id),
        parent_ids: operation.parent_ids.into_iter().map(hex).collect(),
        workspace_name: operation.metadata.workspace_name.map(str::to_owned),
        is_snapshot: operation.metadata.is_snapshot,
        commit_predecessors: operation.predecessors.map(|predecessors| {
            predecessors
                .into_iter()
                .map(|(commit, edges)| (hex(commit), edges.into_iter().map(hex).collect()))
                .collect()
        }),
    })
}

fn parse(bytes: &[u8]) -> Result<RawOperation<'_>, JjDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut view = None;
    let mut parents = Vec::new();
    let mut metadata = None;
    let mut predecessors = BTreeMap::new();
    let mut stores_predecessors = None;
    let mut parent_budget = Budget::new(MAX_OPERATION_PARENTS, "parent count limit exceeded");
    let mut predecessor_budget = Budget::new(
        MAX_OPERATION_PREDECESSOR_REFERENCES,
        "predecessor reference limit exceeded",
    );
    while let Some(field) = fields.next(&[(1, 2), (2, 2), (3, 2), (4, 2), (5, 0)])? {
        match field.tag {
            1 => singular(&mut view, identity(field.bytes()?, 64)?)?,
            2 => {
                parent_budget.take(1)?;
                let parent = identity(field.bytes()?, 64)?;
                if parents.contains(&parent) {
                    return Err(JjDecodeError("duplicate operation parent"));
                }
                parents.push(parent);
            }
            3 => singular(&mut metadata, field.bytes()?)?,
            4 => {
                predecessor_budget.take(1)?;
                let (commit, edges) = predecessor(field.bytes()?, &mut predecessor_budget)?;
                if predecessors.contains_key(commit) {
                    return Err(JjDecodeError("duplicate predecessor commit"));
                }
                predecessors.insert(commit, edges);
            }
            5 => singular(&mut stores_predecessors, boolean(field.varint()?)?)?,
            _ => return Err(JjDecodeError("unknown operation field")),
        }
    }
    let view_id = view.ok_or(JjDecodeError("missing view identity"))?;
    if parents.is_empty() {
        return Err(JjDecodeError(
            "unsupported legacy operation without parents",
        ));
    }
    let stores_predecessors = stores_predecessors.unwrap_or_default();
    if !stores_predecessors && !predecessors.is_empty() {
        return Err(JjDecodeError("predecessor entries without recorded flag"));
    }
    Ok(RawOperation {
        view_id,
        parent_ids: parents,
        metadata: metadata::decode(metadata.unwrap_or_default())?,
        predecessors: stores_predecessors.then_some(predecessors),
    })
}

fn predecessor<'a>(
    bytes: &'a [u8],
    budget: &mut Budget,
) -> Result<(&'a [u8], Vec<&'a [u8]>), JjDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut commit = None;
    let mut edges = Vec::new();
    while let Some(field) = fields.next(&[(1, 2), (2, 2)])? {
        match field.tag {
            1 => singular(&mut commit, identity(field.bytes()?, 20)?)?,
            2 => {
                budget.take(1)?;
                edges.push(identity(field.bytes()?, 20)?);
            }
            _ => return Err(JjDecodeError("unknown predecessor field")),
        }
    }
    Ok((
        commit.ok_or(JjDecodeError("missing predecessor commit identity"))?,
        edges,
    ))
}
