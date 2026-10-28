//! Compatibility reads retain the complete-history contract and its 256-node
//! limit. Larger histories use the resumable, segment-local observation API.

use super::replay::{Prefix, complete, extend};
use super::*;
use crate::model::jj_observation::{MAX_JJ_OBSERVATION_OPERATIONS, is_root};
use crate::model::repository::jj_observation_journal::native_admission::ReplaySelection;
use crate::operations::jj::admission::{DurableNativeAdmission, verify};
use crate::operations::jj::ancestry::{JjAncestryInput, verify_ancestry_to_receipt};
use crate::operations::jj::baseline_persistence::verify_native_baseline_snapshot_ref;
use crate::operations::jj::registration::unix::{policy, saved};
use std::collections::BTreeMap;

type E = JjNativeAdmissionError;

pub(in crate::operations::jj) fn read_complete(
    journal: &JjObservationJournal,
    source: &str,
    workspace: &str,
    id: &str,
    deadline: Instant,
    reads: &mut ReadBudget,
) -> Result<DurableNativeAdmission, E> {
    let mut selected = id.to_owned();
    let mut epoch = None;
    let mut expected_generation = None;
    let mut packets = Vec::new();
    let mut baseline = None;
    for _ in 0..64 {
        policy::check_deadline(deadline)?;
        let (observed_epoch, snapshot) = journal.read_replay_snapshot(
            source,
            workspace,
            ReplaySelection::Receipt(&selected),
            reads,
        )?;
        if epoch.is_some_and(|epoch| epoch != observed_epoch) {
            return Err(E::Input("native compatibility replay connection changed"));
        }
        epoch = Some(observed_epoch);
        saved::validate_historical(&snapshot.registration)?;
        let original = verify_native_baseline_snapshot_ref(&snapshot.registration.native)?;
        if baseline
            .as_ref()
            .is_some_and(|baseline| baseline != &original)
        {
            return Err(E::Input("native compatibility replay scope changed"));
        }
        baseline = Some(original);
        let (_, _, packet) = snapshot.into_requested();
        let packet = packet.ok_or(E::Input("native compatibility replay dependency gap"))?;
        if expected_generation.is_some_and(|generation| generation != packet.generation) {
            return Err(E::Input(
                "native compatibility replay dependency generation differs",
            ));
        }
        let previous = packet.record.previous_admission_id.clone();
        expected_generation = packet.generation.checked_sub(1);
        packets.push(packet);
        match previous {
            Some(previous) => selected = previous,
            None => break,
        }
    }
    let baseline = baseline.ok_or(E::Input("native compatibility replay is empty"))?;
    let base = packets
        .last()
        .ok_or(E::Input("native compatibility replay is empty"))?;
    if base.record.previous_admission_id.is_some() {
        return Err(E::Input(
            "native compatibility replay limit exceeded; use resumable replay",
        ));
    }
    let mut prefix = Prefix {
        receipt: Some(base.admission_id.clone()),
        cursor: verify::receipt(base).cursor().clone(),
        frontier: complete(&baseline, base)?,
    };
    for packet in packets.iter().rev().skip(1) {
        policy::check_deadline(deadline)?;
        prefix = extend(&prefix, packet)?;
    }
    let target = &packets[0];
    let records: BTreeMap<_, _> = packets
        .iter()
        .flat_map(|packet| packet.record.operations.iter())
        .map(|record| (record.operation_id.as_str(), record))
        .collect();
    let mut needed = BTreeMap::new();
    let mut pending = target.record.captured_head_ids.clone();
    while let Some(id) = pending.pop() {
        policy::check_deadline(deadline)?;
        if is_root(&id) || baseline.captured_head_ids().contains(&id) || needed.contains_key(&id) {
            continue;
        }
        if needed.len() >= MAX_JJ_OBSERVATION_OPERATIONS {
            return Err(E::Input(
                "complete history exceeds 256 operations; use segment-local replay",
            ));
        }
        let record = *records
            .get(id.as_str())
            .ok_or(E::Input("native complete history gap"))?;
        pending.extend(record.parent_ids.iter().cloned());
        needed.insert(id, record);
    }
    let refs: Vec<_> = needed.into_values().collect();
    let record = &target.record;
    let proof = verify_ancestry_to_receipt(
        &baseline,
        JjAncestryInput {
            source_id: &record.source_id,
            reader_profile: &record.reader_profile,
            baseline_id: &record.baseline_id,
            expected_native_generation: record.baseline_generation,
            head_ids: &record.captured_head_ids,
            operations: &refs,
        },
    )?;
    if proof.head_closures() != prefix.frontier.head_closures() {
        return Err(E::Input(
            "native complete history differs from segment composition",
        ));
    }
    policy::check_deadline(deadline)?;
    Ok(DurableNativeAdmission::from_proof(
        verify::receipt(target),
        &proof,
    ))
}
