use super::super::pos_encoded::{
    PosEncoded, PosField, sparse_get_string, sparse_get_u32, sparse_get_vec_string,
    sparse_get_vec_u32, sparse_set, string_to_json, u32_to_json, vec_string_to_json,
    vec_u32_to_json,
};
use super::super::types::{EventValues, MetricEventId, SparseArray};

/// Value positions for "rewrite_committed" event.
pub mod rewrite_committed_pos {
    pub const HUMAN_ADDITIONS: usize = 0;
    pub const GIT_DIFF_DELETED_LINES: usize = 1;
    pub const GIT_DIFF_ADDED_LINES: usize = 2;
    pub const TOOL_MODEL_PAIRS: usize = 3;
    // Keep positions 0-14 aligned with committed_pos for ingestion consistency.
    // Position 4 mirrors committed_pos::MIXED_ADDITIONS, which is no longer emitted.
    pub const AI_ADDITIONS: usize = 5;
    pub const AI_ACCEPTED: usize = 6;
    // Positions 7-9 mirror removed committed event fields.
    // Position 10 is intentionally omitted: rewrite events have no first checkpoint timestamp.
    pub const COMMIT_SUBJECT: usize = 11;
    pub const COMMIT_BODY: usize = 12;
    pub const AUTHORSHIP_NOTE: usize = 13;
    pub const HUNKS: usize = 14;
    pub const OPERATION_KIND: usize = 15;
    pub const ORIGINAL_COMMIT_SHAS: usize = 16;
}

/// Values for Event ID 7: rewrite_committed.
///
/// Recorded after rewrite operations create new commit SHAs and authorship
/// notes have been migrated to those post-rewrite commits.
#[derive(Debug, Clone, Default)]
pub struct RewriteCommittedValues {
    pub human_additions: PosField<u32>,
    pub git_diff_deleted_lines: PosField<u32>,
    pub git_diff_added_lines: PosField<u32>,
    pub tool_model_pairs: PosField<Vec<String>>,
    pub ai_additions: PosField<Vec<u32>>,
    pub ai_accepted: PosField<Vec<u32>>,
    pub commit_subject: PosField<String>,
    pub commit_body: PosField<String>,
    pub authorship_note: PosField<String>,
    pub hunks: PosField<String>,
    pub operation_kind: PosField<String>,
    pub original_commit_shas: PosField<Vec<String>>,
}

impl RewriteCommittedValues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn human_additions(mut self, value: u32) -> Self {
        self.human_additions = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn human_additions_null(mut self) -> Self {
        self.human_additions = Some(None);
        self
    }

    pub fn git_diff_deleted_lines(mut self, value: u32) -> Self {
        self.git_diff_deleted_lines = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn git_diff_deleted_lines_null(mut self) -> Self {
        self.git_diff_deleted_lines = Some(None);
        self
    }

    pub fn git_diff_added_lines(mut self, value: u32) -> Self {
        self.git_diff_added_lines = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn git_diff_added_lines_null(mut self) -> Self {
        self.git_diff_added_lines = Some(None);
        self
    }

    pub fn tool_model_pairs(mut self, value: Vec<String>) -> Self {
        self.tool_model_pairs = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn tool_model_pairs_null(mut self) -> Self {
        self.tool_model_pairs = Some(None);
        self
    }

    pub fn ai_additions(mut self, value: Vec<u32>) -> Self {
        self.ai_additions = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn ai_additions_null(mut self) -> Self {
        self.ai_additions = Some(None);
        self
    }

    pub fn ai_accepted(mut self, value: Vec<u32>) -> Self {
        self.ai_accepted = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn ai_accepted_null(mut self) -> Self {
        self.ai_accepted = Some(None);
        self
    }

    pub fn commit_subject(mut self, value: impl Into<String>) -> Self {
        self.commit_subject = Some(Some(value.into()));
        self
    }

    pub fn commit_subject_null(mut self) -> Self {
        self.commit_subject = Some(None);
        self
    }

    pub fn commit_body(mut self, value: impl Into<String>) -> Self {
        self.commit_body = Some(Some(value.into()));
        self
    }

    pub fn commit_body_null(mut self) -> Self {
        self.commit_body = Some(None);
        self
    }

    pub fn authorship_note(mut self, value: impl Into<String>) -> Self {
        self.authorship_note = Some(Some(value.into()));
        self
    }

    pub fn authorship_note_null(mut self) -> Self {
        self.authorship_note = Some(None);
        self
    }

    pub fn hunks(mut self, value: impl Into<String>) -> Self {
        self.hunks = Some(Some(value.into()));
        self
    }

    pub fn hunks_null(mut self) -> Self {
        self.hunks = Some(None);
        self
    }

    pub fn operation_kind(mut self, value: impl Into<String>) -> Self {
        self.operation_kind = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn operation_kind_null(mut self) -> Self {
        self.operation_kind = Some(None);
        self
    }

    pub fn original_commit_shas(mut self, value: Vec<String>) -> Self {
        self.original_commit_shas = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn original_commit_shas_null(mut self) -> Self {
        self.original_commit_shas = Some(None);
        self
    }
}

impl PosEncoded for RewriteCommittedValues {
    fn to_sparse(&self) -> SparseArray {
        let mut map = SparseArray::new();

        sparse_set(
            &mut map,
            rewrite_committed_pos::HUMAN_ADDITIONS,
            u32_to_json(&self.human_additions),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::GIT_DIFF_DELETED_LINES,
            u32_to_json(&self.git_diff_deleted_lines),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::GIT_DIFF_ADDED_LINES,
            u32_to_json(&self.git_diff_added_lines),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::TOOL_MODEL_PAIRS,
            vec_string_to_json(&self.tool_model_pairs),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::AI_ADDITIONS,
            vec_u32_to_json(&self.ai_additions),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::AI_ACCEPTED,
            vec_u32_to_json(&self.ai_accepted),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::COMMIT_SUBJECT,
            string_to_json(&self.commit_subject),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::COMMIT_BODY,
            string_to_json(&self.commit_body),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::AUTHORSHIP_NOTE,
            string_to_json(&self.authorship_note),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::HUNKS,
            string_to_json(&self.hunks),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::OPERATION_KIND,
            string_to_json(&self.operation_kind),
        );
        sparse_set(
            &mut map,
            rewrite_committed_pos::ORIGINAL_COMMIT_SHAS,
            vec_string_to_json(&self.original_commit_shas),
        );

        map
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        Self {
            human_additions: sparse_get_u32(arr, rewrite_committed_pos::HUMAN_ADDITIONS),
            git_diff_deleted_lines: sparse_get_u32(
                arr,
                rewrite_committed_pos::GIT_DIFF_DELETED_LINES,
            ),
            git_diff_added_lines: sparse_get_u32(arr, rewrite_committed_pos::GIT_DIFF_ADDED_LINES),
            tool_model_pairs: sparse_get_vec_string(arr, rewrite_committed_pos::TOOL_MODEL_PAIRS),
            ai_additions: sparse_get_vec_u32(arr, rewrite_committed_pos::AI_ADDITIONS),
            ai_accepted: sparse_get_vec_u32(arr, rewrite_committed_pos::AI_ACCEPTED),
            commit_subject: sparse_get_string(arr, rewrite_committed_pos::COMMIT_SUBJECT),
            commit_body: sparse_get_string(arr, rewrite_committed_pos::COMMIT_BODY),
            authorship_note: sparse_get_string(arr, rewrite_committed_pos::AUTHORSHIP_NOTE),
            hunks: sparse_get_string(arr, rewrite_committed_pos::HUNKS),
            operation_kind: sparse_get_string(arr, rewrite_committed_pos::OPERATION_KIND),
            original_commit_shas: sparse_get_vec_string(
                arr,
                rewrite_committed_pos::ORIGINAL_COMMIT_SHAS,
            ),
        }
    }
}

impl EventValues for RewriteCommittedValues {
    fn event_id() -> MetricEventId {
        MetricEventId::RewriteCommitted
    }

    fn to_sparse(&self) -> SparseArray {
        PosEncoded::to_sparse(self)
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        PosEncoded::from_sparse(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_rewrite_committed_values_event_id() {
        assert_eq!(
            RewriteCommittedValues::event_id(),
            MetricEventId::RewriteCommitted
        );
        assert_eq!(RewriteCommittedValues::event_id() as u16, 7);
    }

    #[test]
    fn test_rewrite_committed_values_sparse_roundtrip() {
        let original = RewriteCommittedValues::new()
            .human_additions(5)
            .git_diff_deleted_lines(2)
            .git_diff_added_lines(7)
            .tool_model_pairs(vec!["all".to_string(), "codex:gpt-5".to_string()])
            .ai_additions(vec![3, 3])
            .ai_accepted(vec![3, 3])
            .commit_subject("rebased commit")
            .commit_body_null()
            .authorship_note("note")
            .hunks("[]")
            .operation_kind("rebase")
            .original_commit_shas(vec!["old1".to_string()]);

        let sparse = PosEncoded::to_sparse(&original);

        assert!(!sparse.contains_key("10"));
        assert_eq!(sparse.get("15"), Some(&Value::String("rebase".to_string())));
        assert_eq!(
            sparse.get("16"),
            Some(&Value::Array(vec![Value::String("old1".to_string())]))
        );

        let restored = <RewriteCommittedValues as PosEncoded>::from_sparse(&sparse);
        assert_eq!(restored.human_additions, Some(Some(5)));
        assert_eq!(
            restored.tool_model_pairs,
            Some(Some(vec!["all".to_string(), "codex:gpt-5".to_string()]))
        );
        assert_eq!(restored.operation_kind, Some(Some("rebase".to_string())));
        assert_eq!(
            restored.original_commit_shas,
            Some(Some(vec!["old1".to_string()]))
        );
    }
}
