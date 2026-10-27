use super::replay::{Prefix, Session, extend, require_snapshot};
use super::*;
use crate::model::repository::jj_observation_journal::native_admission::{
    ExpectedAdmissionCursor, NativeAdmissionScope, PreparedNativeAdmission,
};
use crate::operations::jj::admission::verify;
use crate::operations::jj::ancestry::ClosedJjFrontier;
use crate::operations::jj::capture::registration::HistoryCaptureBudget;
use crate::operations::jj::registration::unix::{load_registered_context, policy, saved};

type E = JjNativeAdmissionError;

pub(super) fn observe(
    session: &mut Session,
    prefix: &Prefix,
    epoch: u64,
    config: &Config,
    deadline: Instant,
    reads: &mut ReadBudget,
) -> Result<Option<(ObservedJjSegment, Prefix)>, E> {
    let mut budget = HistoryCaptureBudget::new(deadline);
    let mut current = budget.open(&session.context)?;
    policy::authorize(&mut current, config, deadline)?;
    let registered = load_registered_context(&session.journal, &current, deadline, reads)?;
    verify::scope(&prefix.cursor.expectation(), &registered)?;
    if registered.workspace_name() != session.registered.workspace_name()
        || registered.attachment_id() != session.registered.attachment_id()
    {
        return Err(E::Input("incremental selected workspace changed"));
    }
    if current.captured().head_ids() == prefix.cursor.admitted_head_ids() {
        let transaction = session.journal.begin_native_admission(
            prefix.cursor.source_id(),
            Some(registered.workspace_name()),
            None,
            reads,
        )?;
        transaction.require_replay_epoch(epoch)?;
        require_snapshot(
            transaction.snapshot(),
            &prefix.cursor,
            prefix.receipt.as_deref(),
            &session.registered,
        )?;
        saved::validate(&current, &transaction.snapshot().registration)?;
        current.final_recheck()?;
        policy::check_deadline(deadline)?;
        drop(transaction);
        return Ok(None);
    }
    current.collect_segment(registered.baseline(), &prefix.frontier)?;
    policy::check_deadline(deadline)?;
    let history = current.borrow_history()?;
    let scope = NativeAdmissionScope {
        source_id: prefix.cursor.source_id(),
        reader_profile: prefix.cursor.reader_profile(),
        initialization_receipt_id: prefix.cursor.initialization_receipt_id(),
        baseline_id: prefix.cursor.baseline_id(),
        baseline_generation: prefix.cursor.baseline_generation(),
    };
    let expected = ExpectedAdmissionCursor {
        generation: prefix.cursor.generation(),
        admitted_head_ids: prefix.cursor.admitted_head_ids(),
    };
    let prepared = match &prefix.receipt {
        Some(previous) => PreparedNativeAdmission::new_segment(
            scope,
            expected,
            history.head_ids(),
            history.ordered_operations(),
            previous,
        )?,
        None => PreparedNativeAdmission::new(
            scope,
            expected,
            history.head_ids(),
            history.ordered_operations(),
        )?,
    };
    let admission_id = prepared.admission_id().to_owned();
    let transaction = session.journal.begin_native_admission(
        prefix.cursor.source_id(),
        Some(registered.workspace_name()),
        Some(&admission_id),
        reads,
    )?;
    // Recheck the epoch only after obtaining the writer reservation, and forbid
    // own-write triggers before any DML can invalidate the cached dependency.
    transaction.require_replay_epoch(epoch)?;
    require_snapshot(
        transaction.snapshot(),
        &prefix.cursor,
        prefix.receipt.as_deref(),
        &session.registered,
    )?;
    saved::validate(&current, &transaction.snapshot().registration)?;
    let staged = transaction.stage(prepared, reads)?;
    let (commit, _, snapshot) = staged.into_parts();
    saved::validate(&current, &snapshot.registration)?;
    let packet = snapshot
        .requested()
        .ok_or(E::Input("staged segment is absent"))?;
    let next = if prefix.receipt.is_some() {
        extend(prefix, packet)?
    } else {
        let refs: Vec<_> = packet.record.operations.iter().collect();
        let proof = crate::operations::jj::ancestry::verify_ancestry_to_receipt(
            registered.baseline().receipt(),
            crate::operations::jj::ancestry::JjAncestryInput {
                source_id: &packet.record.source_id,
                reader_profile: &packet.record.reader_profile,
                baseline_id: &packet.record.baseline_id,
                expected_native_generation: packet.record.baseline_generation,
                head_ids: &packet.record.captured_head_ids,
                operations: &refs,
            },
        )?;
        Prefix {
            receipt: Some(packet.admission_id.clone()),
            cursor: verify::receipt(packet).cursor().clone(),
            frontier: ClosedJjFrontier::from_proof(&proof),
        }
    };
    if packet.admission_id != admission_id
        || next.cursor.generation() != prefix.cursor.generation() + 1
        || packet.record.captured_head_ids != history.head_ids()
        || next.frontier.head_closures() != history.head_closures()
        || snapshot.cursor.generation != next.cursor.generation()
        || snapshot.cursor.admitted_head_ids != next.cursor.admitted_head_ids()
    {
        return Err(E::Input(
            "staged native segment differs from captured evidence",
        ));
    }
    let (_, _, packet) = snapshot.into_requested();
    let packet = packet.ok_or(E::Input("staged segment was not retained"))?;
    let segment = ObservedJjSegment {
        receipt: verify::receipt(&packet),
        local_operations: packet.record.operations,
        head_closures: next.frontier.head_closures().to_vec(),
    };
    drop(history);
    current.final_recheck()?;
    policy::check_deadline(deadline)?;
    commit.commit()?;
    Ok(Some((segment, next)))
}
