use super::{
    BaselineReceipt, DurableCurrentStateBaseline, JjAncestryError, JjAncestryInput, JjHeadClosure,
    VerifiedJjAncestry, verify_with_frontier,
};

/// A bounded native-verified frontier, retaining its exact original-cutoff scope.
/// Persistence callers must separately bind this value to a verified admission
/// receipt and ensure database continuity; merely storing these fields is not proof.
#[derive(Clone)]
pub struct ClosedJjFrontier {
    baseline: BaselineReceipt,
    head_closures: Vec<JjHeadClosure>,
}

impl ClosedJjFrontier {
    pub fn from_proof(proof: &VerifiedJjAncestry<'_>) -> Self {
        Self {
            baseline: proof.baseline_receipt().clone(),
            head_closures: proof.head_closures().to_vec(),
        }
    }

    pub fn from_baseline(baseline: &DurableCurrentStateBaseline) -> Self {
        Self::from_receipt(baseline.receipt())
    }

    pub(crate) fn from_receipt(baseline: &BaselineReceipt) -> Self {
        Self {
            baseline: baseline.clone(),
            head_closures: baseline
                .captured_head_ids()
                .iter()
                .map(|id| JjHeadClosure {
                    head_id: id.clone(),
                    reached_baseline_ids: vec![id.clone()],
                    reaches_root: false,
                })
                .collect(),
        }
    }

    pub fn baseline_receipt(&self) -> &BaselineReceipt {
        &self.baseline
    }

    pub fn head_closures(&self) -> &[JjHeadClosure] {
        &self.head_closures
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.baseline
            .captured_head_ids()
            .iter()
            .any(|head| head == id)
            || self
                .head_closures
                .iter()
                .any(|closure| closure.head_id == id)
    }
}

/// Verify only this segment's operations, composing original-cutoff reachability
/// through exact prior frontier heads. A late branch must supply its own complete
/// ancestry to a prior head, original anchor or root. This never rebases the cutoff.
pub fn verify_ancestry_segment<'a>(
    frontier: &'a ClosedJjFrontier,
    input: JjAncestryInput<'a>,
) -> Result<VerifiedJjAncestry<'a>, JjAncestryError> {
    verify_with_frontier(&frontier.baseline, input, &frontier.head_closures)
}
