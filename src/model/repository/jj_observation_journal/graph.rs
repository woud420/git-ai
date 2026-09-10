use super::{JournalError, invalid};
use crate::model::jj_observation::{JjObservationBatch, JjOperationEvidence, is_root};
use std::collections::{BTreeMap, HashSet};

pub(super) fn order_new<'a>(
    batch: &'a JjObservationBatch,
    known: &BTreeMap<String, JjOperationEvidence>,
) -> Result<Vec<&'a JjOperationEvidence>, JournalError> {
    let incoming: BTreeMap<_, _> = batch
        .operations
        .iter()
        .map(|operation| (operation.operation_id.as_str(), operation))
        .collect();
    let mut active = HashSet::new();
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    for head in &batch.captured_integrated_heads {
        visit(
            head,
            &incoming,
            known,
            &mut active,
            &mut visited,
            &mut ordered,
        )?;
    }
    if incoming
        .keys()
        .any(|id| !known.contains_key(*id) && !visited.contains(*id))
    {
        return Err(invalid(
            "operation is not reachable from captured integrated heads",
        ));
    }
    Ok(ordered)
}

fn visit<'a>(
    id: &str,
    incoming: &BTreeMap<&'a str, &'a JjOperationEvidence>,
    known: &BTreeMap<String, JjOperationEvidence>,
    active: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
    ordered: &mut Vec<&'a JjOperationEvidence>,
) -> Result<(), JournalError> {
    if is_root(id) || known.contains_key(id) || visited.contains(id) {
        return Ok(());
    }
    let operation = incoming
        .get(id)
        .ok_or_else(|| invalid("operation parent or head gap"))?;
    let id = operation.operation_id.as_str();
    if !active.insert(id) {
        return Err(invalid("operation DAG contains a cycle"));
    }
    for parent in &operation.parent_ids {
        visit(parent, incoming, known, active, visited, ordered)?;
    }
    active.remove(id);
    visited.insert(id);
    ordered.push(operation);
    Ok(())
}
