//! Exact operation/view transitions, without inventing rewrite provenance.
//!
//! A recorded predecessor may be synthetic (Git import); an empty predecessor
//! map can accompany a view change (undo/restore). Neither is an authorship claim.
//! Admission, checkpoint ordering and content mapping remain separate gates.

use super::evidence::{JjEvidenceError, VerifiedJjEvidence, verify_evidence};
use super::operation::{DecodedJjOperation, MAX_OPERATION_PARENTS};
use super::view::MAX_VIEW_COMMIT_REFERENCES;
use crate::model::jj_observation::{JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, is_root};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct JjWorkspaceTransition {
    pub workspace_name: String,
    /// One value per operation parent, in the operation's original parent order.
    /// None means that workspace is absent from that parent's immutable view.
    pub before: Vec<Option<String>>,
    pub after: Option<String>,
}

pub struct VerifiedJjTransition<'a> {
    current: VerifiedJjEvidence<'a>,
    workspaces: Vec<JjWorkspaceTransition>,
}

impl VerifiedJjTransition<'_> {
    pub fn operation(&self) -> &DecodedJjOperation {
        self.current.operation()
    }

    /// Exact recorded candidates, including new commits with empty predecessors.
    /// Some(empty) differs from unrecorded evidence. No candidate is promoted to
    /// a proven content rewrite merely because its native hash verifies.
    pub fn recorded_predecessors(&self) -> Option<&BTreeMap<String, Vec<String>>> {
        self.operation().commit_predecessors.as_ref()
    }

    pub fn workspaces(&self) -> &[JjWorkspaceTransition] {
        &self.workspaces
    }
}

#[derive(Debug)]
pub enum JjLineageError {
    Evidence(JjEvidenceError),
    Input(&'static str),
}

impl fmt::Display for JjLineageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("jj transition: ")?;
        match self {
            Self::Evidence(error) => fmt::Display::fmt(error, f),
            Self::Input(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for JjLineageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evidence(error) => Some(error),
            Self::Input(_) => None,
        }
    }
}

/// Join a captured operation with exactly its nonroot parents. Inputs can be in
/// any order; output keeps native parent order and never collapses divergence.
/// All hashes and aggregate limits are checked without filesystem or Git access.
/// The virtual root has no metadata file and contains no workspaces.
pub fn verify_transition<'a>(
    profile: &str,
    current: &'a JjOperationEvidence,
    parents: &[&JjOperationEvidence],
) -> Result<VerifiedJjTransition<'a>, JjLineageError> {
    use JjLineageError::{Evidence, Input};
    if parents.len() > MAX_OPERATION_PARENTS {
        return Err(Input("parent count limit exceeded"));
    }
    let mut bytes = 0usize;
    for evidence in std::iter::once(current).chain(parents.iter().copied()) {
        bytes = bytes
            .checked_add(evidence.operation_bytes.len())
            .and_then(|bytes| bytes.checked_add(evidence.view_bytes.len()))
            .filter(|bytes| *bytes <= MAX_JJ_OBSERVATION_BATCH_BYTES)
            .ok_or(Input("evidence byte limit exceeded"))?;
    }
    let current = verify_evidence(profile, current).map_err(Evidence)?;
    let parent_ids = &current.operation().parent_ids;
    if parents.len() != parent_ids.iter().filter(|id| !is_root(id)).count() {
        return Err(Input("exact nonroot parent set required"));
    }
    let mut verified_parents = BTreeMap::new();
    let mut references = current.view().commit_references;
    let mut names: BTreeSet<_> = current.view().wc_commit_ids.keys().cloned().collect();
    for evidence in parents {
        if !parent_ids.contains(&evidence.operation_id)
            || verified_parents.contains_key(evidence.operation_id.as_str())
        {
            return Err(Input("exact nonroot parent set required"));
        }
        let parent = verify_evidence(profile, evidence).map_err(Evidence)?;
        references = references
            .checked_add(parent.view().commit_references)
            .filter(|count| *count <= MAX_VIEW_COMMIT_REFERENCES)
            .ok_or(Input("commit reference limit exceeded"))?;
        names.extend(parent.view().wc_commit_ids.keys().cloned());
        verified_parents.insert(evidence.operation_id.as_str(), parent);
    }
    let workspaces = names
        .into_iter()
        .map(|name| JjWorkspaceTransition {
            before: parent_ids
                .iter()
                .map(|id| {
                    verified_parents
                        .get(id.as_str())
                        .and_then(|parent| parent.view().wc_commit_ids.get(&name))
                        .cloned()
                })
                .collect(),
            after: current.view().wc_commit_ids.get(&name).cloned(),
            workspace_name: name,
        })
        .collect();
    Ok(VerifiedJjTransition {
        current,
        workspaces,
    })
}
