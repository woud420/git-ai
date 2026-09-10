use crate::model::jj_observation::JjOperationEvidence;
use crate::operations::jj::ancestry::JjHeadClosure;

/// Owned historical closure; it is not a live source lease or admission receipt.
pub(crate) struct CapturedJjHistoryEvidence {
    pub(super) head_ids: Vec<String>,
    pub(super) ordered_operations: Vec<JjOperationEvidence>,
    pub(super) head_closures: Vec<JjHeadClosure>,
    pub(super) reached_baseline_ids: Vec<String>,
    pub(super) reaches_root: bool,
}

impl CapturedJjHistoryEvidence {
    pub(crate) fn head_ids(&self) -> &[String] {
        &self.head_ids
    }

    pub(crate) fn ordered_operations(&self) -> &[JjOperationEvidence] {
        &self.ordered_operations
    }

    pub(crate) fn head_closures(&self) -> &[JjHeadClosure] {
        &self.head_closures
    }

    pub(crate) fn reached_baseline_ids(&self) -> &[String] {
        &self.reached_baseline_ids
    }

    pub(crate) fn reaches_root(&self) -> bool {
        self.reaches_root
    }
}
