//! Proves a supplied native DAG closes on one saved baseline or virtual root.
//! This pure snapshot proof does not establish source residency, current heads,
//! post-baseline newness, admission, or attribution eligibility.

mod graph;
mod preflight;

use super::baseline_persistence::{BaselineReceipt, DurableCurrentStateBaseline};
use super::evidence::{JjEvidenceError, VerifiedJjEvidence, verify_evidence};
use crate::model::jj_observation::{JjObservationError, JjOperationEvidence};
use std::fmt;

const MAX_PREDECESSOR_REFERENCES: usize = 4096;
const MAX_VIEW_REFERENCES: usize = 4096;

pub struct JjAncestryInput<'a> {
    pub source_id: &'a str,
    pub reader_profile: &'a str,
    pub baseline_id: &'a str,
    pub expected_native_generation: u64,
    pub head_ids: &'a [String],
    pub operations: &'a [&'a JjOperationEvidence],
}

/// Borrows the cutoff authority and exact input bytes of a complete bounded DAG.
/// Storage changes do not update this proof; a later writer must check live state.
pub struct VerifiedJjAncestry<'a> {
    baseline: &'a DurableCurrentStateBaseline,
    head_ids: &'a [String],
    ordered_operations: Vec<VerifiedJjEvidence<'a>>,
    reached_baseline_ids: Vec<String>,
    reaches_root: bool,
}

impl<'a> VerifiedJjAncestry<'a> {
    pub fn baseline_receipt(&self) -> &BaselineReceipt {
        self.baseline.receipt()
    }

    pub fn head_ids(&self) -> &'a [String] {
        self.head_ids
    }

    pub fn ordered_operations(&self) -> &[VerifiedJjEvidence<'a>] {
        &self.ordered_operations
    }

    pub fn reached_baseline_ids(&self) -> &[String] {
        &self.reached_baseline_ids
    }

    pub fn reaches_root(&self) -> bool {
        self.reaches_root
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjAncestryError {
    Heads(JjObservationError),
    Envelope(JjObservationError),
    Evidence(JjEvidenceError),
    Input(&'static str),
}

impl fmt::Display for JjAncestryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Heads(error) => write!(formatter, "jj ancestry heads: {error}"),
            Self::Envelope(error) => write!(formatter, "jj ancestry envelope: {error}"),
            Self::Evidence(error) => fmt::Display::fmt(error, formatter),
            Self::Input(message) => write!(formatter, "jj ancestry {message}"),
        }
    }
}

impl std::error::Error for JjAncestryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Heads(error) | Self::Envelope(error) => Some(error),
            Self::Evidence(error) => Some(error),
            Self::Input(_) => None,
        }
    }
}

/// Stops only at exact saved head IDs or root, never at observed IDs or an
/// anchor's parents. Root-only closure may contain no post-baseline work.
pub fn verify_ancestry_to_baseline<'a>(
    baseline: &'a DurableCurrentStateBaseline,
    input: JjAncestryInput<'a>,
) -> Result<VerifiedJjAncestry<'a>, JjAncestryError> {
    preflight::validate(baseline.receipt(), &input)?;
    let mut verified = Vec::with_capacity(input.operations.len());
    let mut predecessor_references = 0usize;
    let mut view_references = 0usize;
    for &evidence in input.operations {
        let proof =
            verify_evidence(input.reader_profile, evidence).map_err(JjAncestryError::Evidence)?;
        for edges in proof
            .operation()
            .commit_predecessors
            .iter()
            .flat_map(|predecessors| predecessors.values())
        {
            charge(
                &mut predecessor_references,
                1 + edges.len(),
                MAX_PREDECESSOR_REFERENCES,
                "aggregate predecessor reference limit exceeded",
            )?;
        }
        charge(
            &mut view_references,
            proof.view().commit_references,
            MAX_VIEW_REFERENCES,
            "aggregate view reference limit exceeded",
        )?;
        verified.push(proof);
    }

    let nodes: Vec<_> = verified
        .iter()
        .map(|proof| graph::TopologyNode {
            id: &proof.operation().operation_id,
            parents: &proof.operation().parent_ids,
        })
        .collect();
    let order = graph::order_to_baseline(
        input.head_ids,
        &nodes,
        baseline.receipt().captured_head_ids(),
    )?;
    let mut remaining: Vec<_> = verified.into_iter().map(Some).collect();
    let ordered_operations = order
        .operation_indices
        .into_iter()
        .map(|index| {
            remaining
                .get_mut(index)
                .and_then(Option::take)
                .ok_or(JjAncestryError::Input("invalid ancestry ordering"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VerifiedJjAncestry {
        baseline,
        head_ids: input.head_ids,
        ordered_operations,
        reached_baseline_ids: order.reached_baseline_ids,
        reaches_root: order.reaches_root,
    })
}

fn charge(
    total: &mut usize,
    additional: usize,
    limit: usize,
    message: &'static str,
) -> Result<(), JjAncestryError> {
    *total = total
        .checked_add(additional)
        .filter(|total| *total <= limit)
        .ok_or(JjAncestryError::Input(message))?;
    Ok(())
}
