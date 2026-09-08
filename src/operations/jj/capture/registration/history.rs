use super::super::CapturedJjHistoryEvidence;
use super::super::evidence::operation_pair_at;
use super::*;
use crate::model::jj_observation::{
    JjOperationEvidence, MAX_JJ_OBSERVATION_OPERATION_BYTES, MAX_JJ_OBSERVATION_OPERATIONS, is_root,
};
use crate::operations::jj::ancestry::{JjAncestryInput, verify_ancestry_to_baseline};
use crate::operations::jj::baseline_persistence::DurableCurrentStateBaseline;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HistoryPhase {
    BeforeWalk,
    EvidenceComplete,
}

pub(super) trait HistoryHooks: SealHooks {
    fn history_phase(&mut self, _phase: HistoryPhase) {}
    fn before_history_pair(&mut self, _operation_id: &str) {}
    fn history_pair_verified(&mut self, _operation_id: &str) {}
}

impl HistoryHooks for DirectCapture {}

pub(super) enum HistoryState {
    NotStarted,
    Failed,
    Complete(HistoryDraft),
}

pub(super) struct HistoryDraft {
    ancestors: Vec<JjOperationEvidence>,
    order: Vec<Origin>,
    reached_baseline_ids: Vec<String>,
    reaches_root: bool,
}

pub(crate) struct BorrowedJjHistoryEvidence<'a> {
    head_ids: &'a [String],
    ordered_operations: Vec<&'a JjOperationEvidence>,
    reached_baseline_ids: &'a [String],
    reaches_root: bool,
}

impl<'a> BorrowedJjHistoryEvidence<'a> {
    pub(crate) fn head_ids(&self) -> &[String] {
        self.head_ids
    }

    pub(crate) fn ordered_operations(&self) -> &[&'a JjOperationEvidence] {
        &self.ordered_operations
    }

    pub(crate) fn reached_baseline_ids(&self) -> &[String] {
        self.reached_baseline_ids
    }

    pub(crate) fn reaches_root(&self) -> bool {
        self.reaches_root
    }
}

#[derive(Clone, Copy)]
enum Origin {
    Head(usize),
    Ancestor(usize),
}

struct Node {
    id: String,
    origin: Option<Origin>,
}

impl RetainedCapture<'_> {
    pub(crate) fn collect_history(
        &mut self,
        baseline: &DurableCurrentStateBaseline,
    ) -> Result<(), E> {
        self.collect_history_with(baseline, &mut DirectCapture)
    }

    pub(super) fn collect_history_with(
        &mut self,
        baseline: &DurableCurrentStateBaseline,
        hooks: &mut impl HistoryHooks,
    ) -> Result<(), E> {
        if !self.history_mode || self.final_checked {
            return Err(E::invalid(
                "history",
                "session is not open for history collection",
            ));
        }
        if !matches!(&self.history_state, HistoryState::NotStarted) {
            return Err(E::invalid("history", "collection already attempted"));
        }
        self.history_state = HistoryState::Failed;
        if self.canonical_paths.is_none() {
            return Err(E::invalid(
                "history",
                "canonical policy paths were not validated",
            ));
        }
        let seal = self
            .seal()
            .ok_or(E::invalid("history", "source seal is absent"))?;
        let captured = self
            .captured
            .as_ref()
            .ok_or(E::invalid("history", "missing sampled evidence"))?;
        let receipt = baseline.receipt();
        if seal.source_id() != receipt.source_id()
            || captured.reader_profile() != receipt.reader_profile()
        {
            return Err(E::invalid(
                "history",
                "saved baseline source or profile differs",
            ));
        }
        hooks.history_phase(HistoryPhase::BeforeWalk);
        self.budget.check(hooks)?;
        let draft = walk(
            captured,
            baseline,
            self.operation_directories,
            &self.directories,
            self.budget,
            hooks,
        )?;
        hooks.history_phase(HistoryPhase::EvidenceComplete);
        self.budget.check(hooks)?;
        self.history_state = HistoryState::Complete(draft);
        Ok(())
    }

    pub(crate) fn borrow_history(&self) -> Result<BorrowedJjHistoryEvidence<'_>, E> {
        if !self.history_mode || self.final_checked {
            return Err(E::invalid(
                "history",
                "session is not open for borrowing completed history",
            ));
        }
        let HistoryState::Complete(draft) = &self.history_state else {
            return Err(E::invalid("history", "successful collection is required"));
        };
        let captured = self
            .captured
            .as_ref()
            .ok_or(E::invalid("history", "missing sampled evidence"))?;
        if draft.order.len() > MAX_JJ_OBSERVATION_OPERATIONS {
            return Err(E::invalid(
                "history",
                "operation reference count limit exceeded",
            ));
        }
        let mut ordered_operations = Vec::with_capacity(draft.order.len());
        for origin in &draft.order {
            let evidence = match *origin {
                Origin::Head(index) => captured.anchors().get(index),
                Origin::Ancestor(index) => draft.ancestors.get(index),
            }
            .ok_or(E::invalid("history", "invalid verified evidence ordering"))?;
            ordered_operations.push(evidence);
        }
        Ok(BorrowedJjHistoryEvidence {
            head_ids: captured.head_ids(),
            ordered_operations,
            reached_baseline_ids: &draft.reached_baseline_ids,
            reaches_root: draft.reaches_root,
        })
    }

    pub(crate) fn into_history(mut self) -> Result<CapturedJjHistoryEvidence, E> {
        if !self.history_mode || !self.final_succeeded {
            return Err(E::invalid(
                "history",
                "successful final source recheck is required",
            ));
        }
        let HistoryState::Complete(draft) =
            std::mem::replace(&mut self.history_state, HistoryState::Failed)
        else {
            return Err(E::invalid("history", "successful collection is required"));
        };
        let CapturedJjCurrentState {
            head_ids, anchors, ..
        } = self
            .captured
            .take()
            .ok_or(E::invalid("history", "missing sampled evidence"))?;
        // Consume the complete capture before moving its anchors, so no exposed
        // checkout index can refer to a removed entry. Raw buffers only move.
        let mut heads: Vec<_> = anchors.into_iter().map(Some).collect();
        let mut ancestors: Vec<_> = draft.ancestors.into_iter().map(Some).collect();
        let ordered_operations = draft
            .order
            .into_iter()
            .map(|origin| {
                match origin {
                    Origin::Head(index) => heads.get_mut(index),
                    Origin::Ancestor(index) => ancestors.get_mut(index),
                }
                .and_then(Option::take)
                .ok_or(E::invalid("history", "invalid verified evidence ordering"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CapturedJjHistoryEvidence {
            head_ids,
            ordered_operations,
            reached_baseline_ids: draft.reached_baseline_ids,
            reaches_root: draft.reaches_root,
        })
    }
}

fn walk(
    captured: &CapturedJjCurrentState,
    baseline: &DurableCurrentStateBaseline,
    locations: [usize; 2],
    directories: &DirectoryRegistry,
    budget: &mut CaptureBudget,
    hooks: &mut impl HistoryHooks,
) -> Result<HistoryDraft, E> {
    let terminals = baseline.receipt().captured_head_ids();
    let terminal = |id: &str| is_root(id) || terminals.iter().any(|saved| saved == id);
    let mut nodes = Vec::new();
    let mut indexes = BTreeMap::new();
    for (index, head) in captured.anchors().iter().enumerate() {
        reserve(
            &mut nodes,
            &mut indexes,
            &head.operation_id,
            Some(Origin::Head(index)),
        )?;
    }
    let mut ancestors = Vec::new();
    let mut cursor = 0;
    while cursor < nodes.len() {
        budget.check(hooks)?;
        if terminal(&nodes[cursor].id) {
            cursor += 1;
            continue;
        }
        let origin = match nodes[cursor].origin {
            Some(origin) => origin,
            None => {
                if budget.anchor_remaining() == 0 {
                    return Err(E::invalid(
                        "history",
                        "retained evidence byte limit exceeded",
                    ));
                }
                hooks.before_history_pair(&nodes[cursor].id);
                budget.check(hooks)?;
                let evidence = operation_pair_at(
                    &nodes[cursor].id,
                    locations,
                    directories,
                    MAX_JJ_OBSERVATION_OPERATION_BYTES.min(budget.anchor_remaining()),
                    "history",
                    budget,
                    hooks,
                )?;
                budget.retain_anchor_bytes(
                    evidence.operation_bytes.len() + evidence.view_bytes.len(),
                )?;
                hooks.history_pair_verified(&evidence.operation_id);
                budget.check(hooks)?;
                let origin = Origin::Ancestor(ancestors.len());
                ancestors.push(evidence);
                nodes[cursor].origin = Some(origin);
                origin
            }
        };
        let evidence = match origin {
            Origin::Head(index) => &captured.anchors()[index],
            Origin::Ancestor(index) => &ancestors[index],
        };
        for parent in &evidence.parent_ids {
            if !terminal(parent) && !indexes.contains_key(parent) {
                reserve(&mut nodes, &mut indexes, parent, None)?;
            }
        }
        cursor += 1;
    }

    // Raw envelopes are bounded above; the existing proof remains the sole
    // authority for aggregate decoded references and exact cutoff closure.
    let operations: Vec<_> = captured
        .anchors()
        .iter()
        .filter(|evidence| !terminal(&evidence.operation_id))
        .chain(ancestors.iter())
        .collect();
    let receipt = baseline.receipt();
    budget.check(hooks)?;
    let proof = verify_ancestry_to_baseline(
        baseline,
        JjAncestryInput {
            source_id: receipt.source_id(),
            reader_profile: receipt.reader_profile(),
            baseline_id: receipt.baseline_id(),
            expected_native_generation: receipt.generation(),
            head_ids: captured.head_ids(),
            operations: &operations,
        },
    );
    budget.check(hooks)?;
    let proof = proof.map_err(|error| E::caused("history", error))?;
    let order = proof
        .ordered_operations()
        .iter()
        .map(|proof| {
            indexes
                .get(&proof.evidence().operation_id)
                .and_then(|index| nodes[*index].origin)
                .ok_or(E::invalid(
                    "history",
                    "verified operation has no retained evidence",
                ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let reached_baseline_ids = proof.reached_baseline_ids().to_vec();
    let reaches_root = proof.reaches_root();
    drop(proof);
    drop(operations);
    Ok(HistoryDraft {
        ancestors,
        order,
        reached_baseline_ids,
        reaches_root,
    })
}

fn reserve(
    nodes: &mut Vec<Node>,
    indexes: &mut BTreeMap<String, usize>,
    id: &str,
    origin: Option<Origin>,
) -> Result<(), E> {
    if nodes.len() >= MAX_JJ_OBSERVATION_OPERATIONS {
        return Err(E::invalid("history", "operation pair count limit exceeded"));
    }
    if indexes.insert(id.to_owned(), nodes.len()).is_some() {
        return Err(E::invalid(
            "history",
            "duplicate sampled operation identity",
        ));
    }
    nodes.push(Node {
        id: id.to_owned(),
        origin,
    });
    Ok(())
}
