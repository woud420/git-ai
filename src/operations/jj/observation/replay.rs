use super::*;
use crate::model::repository::jj_observation_journal::native_admission::{
    ReplaySelection, StoredAdmissionSnapshot, StoredNativeAdmission,
};
use crate::operations::jj::admission::{unix::read_state_for_replay, verify};
use crate::operations::jj::ancestry::{
    ClosedJjFrontier, JjAncestryInput, verify_ancestry_segment, verify_ancestry_to_receipt,
};
use crate::operations::jj::registration::{
    RegisteredJjCurrentState,
    unix::{policy, saved},
};

type E = JjNativeAdmissionError;

#[derive(Clone)]
pub(super) struct Prefix {
    pub receipt: Option<String>,
    pub cursor: NativeAdmissionCursor,
    pub frontier: ClosedJjFrontier,
}

enum Phase {
    Search { id: String, generation: u64 },
    Forward(Prefix),
    Ready(Prefix),
}

struct Cache {
    epoch: u64,
    phase: Phase,
}

pub(super) struct Session {
    pub journal: JjObservationJournal,
    pub context: WorkspaceContext,
    pub registered: RegisteredJjCurrentState,
    pub expected: NativeAdmissionCursor,
    pub receipt: Option<String>,
    cache: Option<Cache>,
}

impl Session {
    pub fn new(
        journal: JjObservationJournal,
        context: WorkspaceContext,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<Self, E> {
        let state = read_state_for_replay(&journal, &context, config, deadline, reads)?;
        Ok(Self {
            expected: state.cursor().clone(),
            receipt: state
                .latest_receipt()
                .map(|receipt| receipt.admission_id().to_owned()),
            registered: state.into_registration(),
            journal,
            context,
            cache: None,
        })
    }

    pub fn verified_cursor(&self) -> Option<&NativeAdmissionCursor> {
        match self.cache.as_ref().map(|cache| &cache.phase) {
            Some(Phase::Ready(prefix)) => Some(&prefix.cursor),
            _ => None,
        }
    }

    pub fn step(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<JjObservationStep, E> {
        self.step_with_mode(config, deadline, reads, true)
    }

    pub fn step_with_mode(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
        observe: bool,
    ) -> Result<JjObservationStep, E> {
        let result = self.step_inner(config, deadline, reads, observe);
        if result.is_err() {
            self.cache = None;
        }
        result
    }

    fn step_inner(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
        observe: bool,
    ) -> Result<JjObservationStep, E> {
        // Pure persisted-evidence replay performs no Git work per dependency.
        // Full physical/policy checks occur at session opening and once more
        // before a ready result or an admission, independently of history size.
        policy::require_opt_in(config)?;
        policy::check_deadline(deadline)?;
        let selection = match self.cache.as_ref().map(|cache| &cache.phase) {
            Some(Phase::Search { id, .. }) => ReplaySelection::Receipt(id),
            Some(Phase::Forward(prefix)) => {
                ReplaySelection::Generation(prefix.cursor.generation() + 1)
            }
            _ => ReplaySelection::Latest,
        };
        let (epoch, snapshot) = self.journal.read_replay_snapshot(
            self.expected.source_id(),
            self.registered.workspace_name(),
            selection,
            reads,
        )?;
        require_snapshot(
            &snapshot,
            &self.expected,
            self.receipt.as_deref(),
            &self.registered,
        )?;
        let baseline = self.registered.baseline().receipt();
        if self
            .cache
            .as_ref()
            .is_some_and(|cache| cache.epoch != epoch)
        {
            self.cache = None;
            return Ok(JjObservationStep::Replaying);
        }
        let phase = self.cache.take().map(|cache| cache.phase);
        let phase = match phase {
            Some(Phase::Ready(prefix)) => Phase::Ready(prefix),
            Some(Phase::Forward(prefix)) => {
                let packet = snapshot
                    .requested()
                    .ok_or(E::Input("replay dependency is absent"))?;
                let prefix = extend(&prefix, packet)?;
                if prefix.cursor == self.expected && prefix.receipt == self.receipt {
                    Phase::Ready(prefix)
                } else {
                    Phase::Forward(prefix)
                }
            }
            search => {
                let packet = if matches!(search, Some(Phase::Search { .. })) {
                    snapshot.requested()
                } else {
                    snapshot.latest.as_ref()
                };
                if let Some(packet) = packet {
                    if let Some(Phase::Search { id, generation }) = search
                        && (packet.admission_id != id || packet.generation != generation)
                    {
                        return Err(E::Input("replay dependency identity differs"));
                    }
                    if let Some(previous) = &packet.record.previous_admission_id {
                        Phase::Search {
                            id: previous.clone(),
                            generation: packet.generation - 1,
                        }
                    } else {
                        let frontier = complete(baseline, packet)?;
                        let prefix = Prefix {
                            receipt: Some(packet.admission_id.clone()),
                            cursor: verify::receipt(packet).cursor().clone(),
                            frontier,
                        };
                        if prefix.cursor == self.expected && prefix.receipt == self.receipt {
                            Phase::Ready(prefix)
                        } else {
                            Phase::Forward(prefix)
                        }
                    }
                } else if self.expected.generation() == 0 {
                    Phase::Ready(Prefix {
                        receipt: None,
                        cursor: self.expected.clone(),
                        frontier: ClosedJjFrontier::from_baseline(self.registered.baseline()),
                    })
                } else {
                    return Err(E::Input("native replay packet gap"));
                }
            }
        };
        policy::check_deadline(deadline)?;
        let ready = match &phase {
            Phase::Ready(prefix) => Some(prefix.clone()),
            _ => None,
        };
        self.cache = Some(Cache { epoch, phase });
        drop(snapshot);
        let Some(prefix) = ready else {
            return Ok(JjObservationStep::Replaying);
        };
        if !observe {
            let live =
                read_state_for_replay(&self.journal, &self.context, config, deadline, reads)?;
            if live.cursor() != &self.expected
                || live.registration().workspace_name() != self.registered.workspace_name()
                || live.registration().attachment_id() != self.registered.attachment_id()
            {
                return Err(E::Input("active observation scope or cursor changed"));
            }
            return Ok(JjObservationStep::Unchanged);
        }
        let result = super::writer::observe(self, &prefix, epoch, config, deadline, reads)?;
        if let Some((segment, prefix)) = result {
            self.expected = prefix.cursor.clone();
            self.receipt = prefix.receipt.clone();
            self.cache = Some(Cache {
                epoch,
                phase: Phase::Ready(prefix),
            });
            Ok(JjObservationStep::Admitted(Box::new(segment)))
        } else {
            Ok(JjObservationStep::Unchanged)
        }
    }

    pub fn read_latest_segment(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<Option<ObservedJjSegment>, E> {
        let result = self.read_latest_inner(config, deadline, reads);
        if result.is_err() {
            self.cache = None;
        }
        result
    }

    fn read_latest_inner(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<Option<ObservedJjSegment>, E> {
        let Some(Cache {
            epoch: cached_epoch,
            phase: Phase::Ready(prefix),
        }) = &self.cache
        else {
            return Ok(None);
        };
        let closures = prefix.frontier.head_closures().to_vec();
        let cached_epoch = *cached_epoch;
        let live = read_state_for_replay(&self.journal, &self.context, config, deadline, reads)?;
        if live.cursor() != &self.expected {
            return Err(E::Input("active observation cursor changed"));
        }
        let (epoch, snapshot) = self.journal.read_replay_snapshot(
            self.expected.source_id(),
            self.registered.workspace_name(),
            ReplaySelection::Latest,
            reads,
        )?;
        require_snapshot(
            &snapshot,
            &self.expected,
            self.receipt.as_deref(),
            &self.registered,
        )?;
        if epoch != cached_epoch {
            self.cache = None;
            return Ok(None);
        }
        policy::check_deadline(deadline)?;
        Ok(snapshot.latest.map(|packet| ObservedJjSegment {
            receipt: verify::receipt(&packet),
            local_operations: packet.record.operations,
            head_closures: closures,
        }))
    }
}

pub(super) fn require_snapshot(
    snapshot: &StoredAdmissionSnapshot,
    expected: &NativeAdmissionCursor,
    receipt: Option<&str>,
    target: &RegisteredJjCurrentState,
) -> Result<(), E> {
    saved::validate_historical(&snapshot.registration)?;
    let registered = &snapshot.registration.registration;
    let selected = &snapshot.registration.selected_workspace().record;
    if snapshot.cursor.generation != expected.generation()
        || snapshot.cursor.admitted_head_ids != expected.admitted_head_ids()
        || snapshot
            .latest
            .as_ref()
            .map(|packet| packet.admission_id.as_str())
            != receipt
        || registered.checksum != expected.initialization_receipt_id()
        || registered.record.baseline_id != expected.baseline_id()
        || selected.workspace_name != target.workspace_name()
        || selected.attachment_id != target.attachment_id()
    {
        return Err(E::Input("active observation scope or cursor changed"));
    }
    Ok(())
}

pub(super) fn complete(
    baseline: &super::super::baseline_persistence::BaselineReceipt,
    packet: &StoredNativeAdmission,
) -> Result<ClosedJjFrontier, E> {
    let record = &packet.record;
    let refs: Vec<_> = record.operations.iter().collect();
    let proof = verify_ancestry_to_receipt(
        baseline,
        JjAncestryInput {
            source_id: &record.source_id,
            reader_profile: &record.reader_profile,
            baseline_id: &record.baseline_id,
            expected_native_generation: record.baseline_generation,
            head_ids: &record.captured_head_ids,
            operations: &refs,
        },
    )?;
    if !proof
        .ordered_operations()
        .iter()
        .map(|proof| proof.operation().operation_id.as_str())
        .eq(record
            .operations
            .iter()
            .map(|record| record.operation_id.as_str()))
    {
        return Err(E::Input(
            "saved evidence is not in deterministic ancestry order",
        ));
    }
    Ok(ClosedJjFrontier::from_proof(&proof))
}

pub(super) fn extend(prefix: &Prefix, packet: &StoredNativeAdmission) -> Result<Prefix, E> {
    if packet.generation != prefix.cursor.generation() + 1
        || packet.record.expected_admitted_head_ids != prefix.cursor.admitted_head_ids()
        || packet.record.previous_admission_id.as_deref() != prefix.receipt.as_deref()
    {
        return Err(E::Input(
            "native segment dependency differs from verified prefix",
        ));
    }
    let record = &packet.record;
    let refs: Vec<_> = record.operations.iter().collect();
    let proof = verify_ancestry_segment(
        &prefix.frontier,
        JjAncestryInput {
            source_id: &record.source_id,
            reader_profile: &record.reader_profile,
            baseline_id: &record.baseline_id,
            expected_native_generation: record.baseline_generation,
            head_ids: &record.captured_head_ids,
            operations: &refs,
        },
    )?;
    if !proof
        .ordered_operations()
        .iter()
        .map(|proof| proof.operation().operation_id.as_str())
        .eq(record
            .operations
            .iter()
            .map(|record| record.operation_id.as_str()))
    {
        return Err(E::Input(
            "segment evidence is not in deterministic ancestry order",
        ));
    }
    Ok(Prefix {
        receipt: Some(packet.admission_id.clone()),
        cursor: verify::receipt(packet).cursor().clone(),
        frontier: ClosedJjFrontier::from_proof(&proof),
    })
}
