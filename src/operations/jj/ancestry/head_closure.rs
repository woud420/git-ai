/// Exact original-cutoff reachability for one head in a verified snapshot.
/// This summary alone cannot authorize a wider traversal cutoff or attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjHeadClosure {
    pub(super) head_id: String,
    pub(super) reached_baseline_ids: Vec<String>,
    pub(super) reaches_root: bool,
}

impl JjHeadClosure {
    pub fn head_id(&self) -> &str {
        &self.head_id
    }

    pub fn reached_baseline_ids(&self) -> &[String] {
        &self.reached_baseline_ids
    }

    pub fn reaches_root(&self) -> bool {
        self.reaches_root
    }
}
