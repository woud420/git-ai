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
        let head_ids = heads::scan(&self.directories, source.heads, 0, self.budget, self.hooks)?;
        let checkout_bytes = evidence::checkout_bytes(
            &self.directories,
            source.working_copy,
            self.budget,
            self.hooks,
        )?;
        let checkout = evidence::checkout(&checkout_bytes, self.budget, self.hooks)?;
        self.phase(CapturePhase::InitialSamplesRead)?;

        let mut anchors = Vec::with_capacity(head_ids.len());
        for id in &head_ids {
            anchors.push(evidence::operation_pair(
                id,
                &source,
                &self.directories,
                true,
                self.budget,
                self.hooks,
            )?);
        }
        self.budget.check(self.hooks)?;
        let prepared =
            prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, &head_ids, &anchors);
        self.budget.check(self.hooks)?;
        prepared.map_err(|error| E::caused("evidence", error))?;

        let checkout_evidence = if let Some(index) = anchors
            .iter()
            .position(|anchor| anchor.operation_id == checkout.operation_id)
        {
            evidence::require_workspace(&anchors[index], &checkout, self.budget, self.hooks)?;
            CheckoutEvidence::Anchor(index)
        } else {
            let own = evidence::operation_pair(
                &checkout.operation_id,
                &source,
                &self.directories,
                false,
                self.budget,
                self.hooks,
            )?;
            evidence::require_workspace(&own, &checkout, self.budget, self.hooks)?;
            CheckoutEvidence::Outside(own)
        };
        self.phase(CapturePhase::EvidenceVerified)?;

        let final_checkout = evidence::checkout_bytes(
            &self.directories,
            source.working_copy,
            self.budget,
            self.hooks,
        )?;
        if final_checkout != checkout_bytes {
            return Err(E::invalid("checkout", "changed checkout bytes"));
        }
        let final_heads = heads::scan(&self.directories, source.heads, 1, self.budget, self.hooks)?;
        if final_heads != head_ids {
            return Err(E::invalid("heads", "changed raw head set"));
        }
        self.metadata
            .recheck(&self.directories, self.budget, self.hooks)?;
        self.phase(CapturePhase::FinalSamplesRead)?;
        self.directories.recheck(self.budget, self.hooks)?;
        let metadata = std::mem::take(&mut self.metadata).into_captured(&self.directories);
        self.budget.check(self.hooks)?;
        Ok(CapturedJjCurrentState {
            source_binding: source.binding,
            head_ids,
            anchors,
            checkout_bytes,
            checkout,
            checkout_evidence,
            _workspace_directories: source.workspace_directories,
            _metadata: metadata,
        })
    }

    fn phase(&mut self, phase: CapturePhase) -> Result<(), E> {
        self.hooks.phase(phase);
        self.budget.check(self.hooks)
    }
}

impl<H> Drop for Capture<'_, H> {
    fn drop(&mut self) {
        self.directories.close(self.budget);
    }
}
