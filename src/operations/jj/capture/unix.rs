use super::directories::DirectoryRegistry;
use super::metadata::MetadataSamples;
use super::{
    CaptureBudget, CaptureHooks, CapturePhase, CapturedJjCurrentState, CheckoutEvidence,
    JjCaptureError as E, evidence, heads, source,
};
use crate::model::jj_observation::JJ_OBSERVATION_READER_PROFILE;
use crate::operations::jj::baseline::prepare_current_state_baseline;
use crate::operations::workspace_context::WorkspaceContext;

pub(super) fn capture_with(
    context: &WorkspaceContext,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<CapturedJjCurrentState, E> {
    budget.check(hooks)?;
    source::preflight(context)?;
    let directories = DirectoryRegistry::new(budget, hooks)?;
    let mut capture = Capture {
        directories,
        metadata: MetadataSamples::default(),
        budget,
        hooks,
    };
    capture.run(context)
}

struct Capture<'a, H> {
    directories: DirectoryRegistry,
    metadata: MetadataSamples,
    budget: &'a mut CaptureBudget,
    hooks: &'a mut H,
}

impl<H: CaptureHooks> Capture<'_, H> {
    fn run(&mut self, context: &WorkspaceContext) -> Result<CapturedJjCurrentState, E> {
        let source = source::bind(
            context,
            &mut self.directories,
            &mut self.metadata,
            self.budget,
            self.hooks,
        )?;
        let mut captured = sample_source(
            source,
            &self.directories,
            &self.metadata,
            self.budget,
            self.hooks,
        )?;
        captured._metadata = std::mem::take(&mut self.metadata).into_captured(&self.directories);
        self.budget.check(self.hooks)?;
        Ok(captured)
    }
}

pub(super) fn sample_source(
    source: source::BoundSource,
    directories: &DirectoryRegistry,
    metadata: &MetadataSamples,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<CapturedJjCurrentState, E> {
    let head_ids = heads::scan(directories, source.heads, 0, budget, hooks)?;
    let checkout_bytes = evidence::checkout_bytes(directories, source.working_copy, budget, hooks)?;
    let checkout = evidence::checkout(&checkout_bytes, budget, hooks)?;
    phase(CapturePhase::InitialSamplesRead, budget, hooks)?;

    let mut anchors = Vec::with_capacity(head_ids.len());
    for id in &head_ids {
        anchors.push(evidence::operation_pair(
            id,
            &source,
            directories,
            true,
            budget,
            hooks,
        )?);
    }
    budget.check(hooks)?;
    let prepared =
        prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, &head_ids, &anchors);
    budget.check(hooks)?;
    prepared.map_err(|error| E::caused("evidence", error))?;

    let checkout_evidence = if let Some(index) = anchors
        .iter()
        .position(|anchor| anchor.operation_id == checkout.operation_id)
    {
        evidence::require_workspace(&anchors[index], &checkout, budget, hooks)?;
        CheckoutEvidence::Anchor(index)
    } else {
        let own = evidence::operation_pair(
            &checkout.operation_id,
            &source,
            directories,
            false,
            budget,
            hooks,
        )?;
        evidence::require_workspace(&own, &checkout, budget, hooks)?;
        CheckoutEvidence::Outside(own)
    };
    phase(CapturePhase::EvidenceVerified, budget, hooks)?;

    let final_checkout = evidence::checkout_bytes(directories, source.working_copy, budget, hooks)?;
    if final_checkout != checkout_bytes {
        return Err(E::invalid("checkout", "changed checkout bytes"));
    }
    let final_heads = heads::scan(directories, source.heads, 1, budget, hooks)?;
    if final_heads != head_ids {
        return Err(E::invalid("heads", "changed raw head set"));
    }
    metadata.recheck(directories, budget, hooks)?;
    phase(CapturePhase::FinalSamplesRead, budget, hooks)?;
    directories.recheck(budget, hooks)?;
    Ok(CapturedJjCurrentState {
        source_binding: source.binding,
        head_ids,
        anchors,
        checkout_bytes,
        checkout,
        checkout_evidence,
        _workspace_directories: source.workspace_directories,
        _metadata: Vec::new(),
    })
}

fn phase(
    phase: CapturePhase,
    budget: &CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<(), E> {
    hooks.phase(phase);
    budget.check(hooks)
}

impl<H> Drop for Capture<'_, H> {
    fn drop(&mut self) {
        self.directories.close(self.budget);
    }
}
