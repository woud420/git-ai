use super::super::pos_encoded::{
    PosEncoded, PosField, sparse_get_string, sparse_get_u32, sparse_get_u64, sparse_get_vec_string,
    sparse_get_vec_u32, sparse_set, string_to_json, u32_to_json, u64_to_json, vec_string_to_json,
    vec_u32_to_json,
};
use super::super::types::{EventValues, MetricEventId, SparseArray};

/// Value positions for "committed" event.
pub mod committed_pos {
    // Scalar fields
    pub const HUMAN_ADDITIONS: usize = 0;
    pub const GIT_DIFF_DELETED_LINES: usize = 1;
    pub const GIT_DIFF_ADDED_LINES: usize = 2;

    // Array fields (parallel arrays, index 0 = "all" aggregate, index 1+ = per tool/model)
    pub const TOOL_MODEL_PAIRS: usize = 3;
    pub const MIXED_ADDITIONS: usize = 4;
    pub const AI_ADDITIONS: usize = 5;
    pub const AI_ACCEPTED: usize = 6;
    pub const TOTAL_AI_ADDITIONS: usize = 7;
    pub const TOTAL_AI_DELETIONS: usize = 8;
    // Position 9 was time_waiting_for_ai (removed)

    // New scalar fields
    pub const FIRST_CHECKPOINT_TS: usize = 10; // u64 (null if no checkpoints)
    pub const COMMIT_SUBJECT: usize = 11; // String
    pub const COMMIT_BODY: usize = 12; // String (null if empty)
    pub const AUTHORSHIP_NOTE: usize = 13; // String (full serialized authorship note)
    pub const HUNKS: usize = 14; // String (JSON array of DiffJsonHunk)
    pub const AUTHOR_TS: usize = 15; // u64 (git author timestamp, %at)
    pub const COMMIT_TS: usize = 16; // u64 (git committer timestamp, %ct)
    pub const PATCH_ID: usize = 17; // String (git patch-id --stable)
}

/// Values for Event ID 1: committed
///
/// Recorded when AI-assisted code is committed.
///
/// **Scalar fields:**
/// | Position | Name | Type |
/// |----------|------|------|
/// | 0 | human_additions | u32 |
/// | 1 | git_diff_deleted_lines | u32 |
/// | 2 | git_diff_added_lines | u32 |
///
/// **Array fields (parallel arrays, index 0 = "all" for aggregate, index 1+ = per tool/model):**
/// | Position | Name | Type |
/// |----------|------|------|
/// | 3 | tool_model_pairs | `Vec<String>` |
/// | 4 | (removed) | - |
/// | 5 | ai_additions | `Vec<u32>` |
/// | 6 | ai_accepted | `Vec<u32>` |
/// | 7 | (removed) | - |
/// | 8 | (removed) | - |
/// | 9 | (removed) | - |
/// | 10 | first_checkpoint_ts | u64 |
/// | 11 | commit_subject | String |
/// | 12 | commit_body | String |
/// | 13 | authorship_note | String |
/// | 14 | hunks | String |
/// | 15 | author_ts | u64 |
/// | 16 | commit_ts | u64 |
/// | 17 | patch_id | String |
#[derive(Debug, Clone, Default)]
pub struct CommittedValues {
    // Scalar fields
    pub human_additions: PosField<u32>,
    pub git_diff_deleted_lines: PosField<u32>,
    pub git_diff_added_lines: PosField<u32>,

    // Array fields (parallel arrays)
    pub tool_model_pairs: PosField<Vec<String>>,
    pub ai_additions: PosField<Vec<u32>>,
    pub ai_accepted: PosField<Vec<u32>>,

    // New scalar fields
    pub first_checkpoint_ts: PosField<u64>,
    pub commit_subject: PosField<String>,
    pub commit_body: PosField<String>,
    pub authorship_note: PosField<String>,
    pub hunks: PosField<String>,
    pub author_ts: PosField<u64>,
    pub commit_ts: PosField<u64>,
    pub patch_id: PosField<String>,
}

impl CommittedValues {
    pub fn new() -> Self {
        Self::default()
    }

    // Builder methods for scalar fields

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

    // Builder methods for array fields

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

    // Builder methods for new scalar fields

    pub fn first_checkpoint_ts(mut self, value: u64) -> Self {
        self.first_checkpoint_ts = Some(Some(value));
        self
    }

    pub fn first_checkpoint_ts_null(mut self) -> Self {
        self.first_checkpoint_ts = Some(None);
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

    pub fn author_ts(mut self, value: u64) -> Self {
        self.author_ts = Some(Some(value));
        self
    }

    pub fn author_ts_null(mut self) -> Self {
        self.author_ts = Some(None);
        self
    }

    pub fn commit_ts(mut self, value: u64) -> Self {
        self.commit_ts = Some(Some(value));
        self
    }

    pub fn commit_ts_null(mut self) -> Self {
        self.commit_ts = Some(None);
        self
    }

    pub fn patch_id(mut self, value: impl Into<String>) -> Self {
        self.patch_id = Some(Some(value.into()));
        self
    }

    pub fn patch_id_null(mut self) -> Self {
        self.patch_id = Some(None);
        self
    }
}

impl PosEncoded for CommittedValues {
    fn to_sparse(&self) -> SparseArray {
        let mut map = SparseArray::new();

        // Scalar fields
        sparse_set(
            &mut map,
            committed_pos::HUMAN_ADDITIONS,
            u32_to_json(&self.human_additions),
        );
        sparse_set(
            &mut map,
            committed_pos::GIT_DIFF_DELETED_LINES,
            u32_to_json(&self.git_diff_deleted_lines),
        );
        sparse_set(
            &mut map,
            committed_pos::GIT_DIFF_ADDED_LINES,
            u32_to_json(&self.git_diff_added_lines),
        );

        // Array fields
        sparse_set(
            &mut map,
            committed_pos::TOOL_MODEL_PAIRS,
            vec_string_to_json(&self.tool_model_pairs),
        );
        sparse_set(
            &mut map,
            committed_pos::AI_ADDITIONS,
            vec_u32_to_json(&self.ai_additions),
        );
        sparse_set(
            &mut map,
            committed_pos::AI_ACCEPTED,
            vec_u32_to_json(&self.ai_accepted),
        );

        // New scalar fields
        sparse_set(
            &mut map,
            committed_pos::FIRST_CHECKPOINT_TS,
            u64_to_json(&self.first_checkpoint_ts),
        );
        sparse_set(
            &mut map,
            committed_pos::COMMIT_SUBJECT,
            string_to_json(&self.commit_subject),
        );
        sparse_set(
            &mut map,
            committed_pos::COMMIT_BODY,
            string_to_json(&self.commit_body),
        );
        sparse_set(
            &mut map,
            committed_pos::AUTHORSHIP_NOTE,
            string_to_json(&self.authorship_note),
        );
        sparse_set(&mut map, committed_pos::HUNKS, string_to_json(&self.hunks));
        sparse_set(
            &mut map,
            committed_pos::AUTHOR_TS,
            u64_to_json(&self.author_ts),
        );
        sparse_set(
            &mut map,
            committed_pos::COMMIT_TS,
            u64_to_json(&self.commit_ts),
        );
        sparse_set(
            &mut map,
            committed_pos::PATCH_ID,
            string_to_json(&self.patch_id),
        );

        map
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        Self {
            // Scalar fields
            human_additions: sparse_get_u32(arr, committed_pos::HUMAN_ADDITIONS),
            git_diff_deleted_lines: sparse_get_u32(arr, committed_pos::GIT_DIFF_DELETED_LINES),
            git_diff_added_lines: sparse_get_u32(arr, committed_pos::GIT_DIFF_ADDED_LINES),

            // Array fields
            tool_model_pairs: sparse_get_vec_string(arr, committed_pos::TOOL_MODEL_PAIRS),
            ai_additions: sparse_get_vec_u32(arr, committed_pos::AI_ADDITIONS),
            ai_accepted: sparse_get_vec_u32(arr, committed_pos::AI_ACCEPTED),

            // New scalar fields
            first_checkpoint_ts: sparse_get_u64(arr, committed_pos::FIRST_CHECKPOINT_TS),
            commit_subject: sparse_get_string(arr, committed_pos::COMMIT_SUBJECT),
            commit_body: sparse_get_string(arr, committed_pos::COMMIT_BODY),
            authorship_note: sparse_get_string(arr, committed_pos::AUTHORSHIP_NOTE),
            hunks: sparse_get_string(arr, committed_pos::HUNKS),
            author_ts: sparse_get_u64(arr, committed_pos::AUTHOR_TS),
            commit_ts: sparse_get_u64(arr, committed_pos::COMMIT_TS),
            patch_id: sparse_get_string(arr, committed_pos::PATCH_ID),
        }
    }
}

impl EventValues for CommittedValues {
    fn event_id() -> MetricEventId {
        MetricEventId::Committed
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
    fn test_committed_values_builder() {
        let values = CommittedValues::new()
            .human_additions(50)
            .git_diff_deleted_lines(20)
            .git_diff_added_lines(150)
            .tool_model_pairs(vec!["all".to_string(), "claude-code:claude-3".to_string()])
            .ai_additions(vec![100, 70])
            .ai_accepted(vec![80, 55]);

        assert_eq!(values.human_additions, Some(Some(50)));
        assert_eq!(
            values.tool_model_pairs,
            Some(Some(vec![
                "all".to_string(),
                "claude-code:claude-3".to_string()
            ]))
        );
        assert_eq!(values.ai_additions, Some(Some(vec![100, 70])));
    }

    #[test]
    fn test_committed_values_to_sparse() {
        let values = CommittedValues::new()
            .human_additions(50)
            .git_diff_deleted_lines(20)
            .git_diff_added_lines(150)
            .tool_model_pairs(vec!["all".to_string(), "cursor:gpt-4".to_string()])
            .ai_additions(vec![100, 30]);

        let sparse = PosEncoded::to_sparse(&values);

        assert_eq!(sparse.get("0"), Some(&Value::Number(50.into())));
        assert_eq!(sparse.get("1"), Some(&Value::Number(20.into())));
        assert_eq!(sparse.get("2"), Some(&Value::Number(150.into())));
        assert_eq!(
            sparse.get("3"),
            Some(&Value::Array(vec![
                Value::String("all".to_string()),
                Value::String("cursor:gpt-4".to_string())
            ]))
        );
        assert_eq!(
            sparse.get("5"),
            Some(&Value::Array(vec![
                Value::Number(100.into()),
                Value::Number(30.into())
            ]))
        );
    }

    #[test]
    fn test_committed_values_with_commit_timestamps_and_patch_id() {
        let values = CommittedValues::new()
            .author_ts(1_704_067_200)
            .commit_ts(1_704_067_260)
            .patch_id("abc123");

        let sparse = PosEncoded::to_sparse(&values);

        assert_eq!(
            sparse.get("15"),
            Some(&Value::Number(1_704_067_200u64.into()))
        );
        assert_eq!(
            sparse.get("16"),
            Some(&Value::Number(1_704_067_260u64.into()))
        );
        assert_eq!(sparse.get("17"), Some(&Value::String("abc123".to_string())));
    }

    #[test]
    fn test_committed_values_from_sparse() {
        let mut sparse = SparseArray::new();
        sparse.insert("0".to_string(), Value::Number(75.into()));
        sparse.insert(
            "3".to_string(),
            Value::Array(vec![
                Value::String("all".to_string()),
                Value::String("copilot:gpt-4".to_string()),
            ]),
        );
        sparse.insert(
            "5".to_string(),
            Value::Array(vec![Value::Number(200.into()), Value::Number(100.into())]),
        );

        let values = <CommittedValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(values.human_additions, Some(Some(75)));
        assert_eq!(
            values.tool_model_pairs,
            Some(Some(vec!["all".to_string(), "copilot:gpt-4".to_string()]))
        );
        assert_eq!(values.ai_additions, Some(Some(vec![200, 100])));
        assert_eq!(values.git_diff_deleted_lines, None); // not set
    }

    #[test]
    fn test_committed_values_event_id() {
        assert_eq!(CommittedValues::event_id(), MetricEventId::Committed);
        assert_eq!(CommittedValues::event_id() as u16, 1);
    }

    #[test]
    fn test_committed_values_null_fields() {
        let values = CommittedValues::new()
            .human_additions_null()
            .git_diff_deleted_lines_null()
            .tool_model_pairs_null();

        assert_eq!(values.human_additions, Some(None));
        assert_eq!(values.git_diff_deleted_lines, Some(None));
        assert_eq!(values.tool_model_pairs, Some(None));
    }

    #[test]
    fn test_committed_values_with_commit_info() {
        let values = CommittedValues::new()
            .human_additions(10)
            .first_checkpoint_ts(1704067200)
            .commit_subject("Initial commit")
            .commit_body("This is the commit body\n\nWith multiple lines");

        assert_eq!(values.first_checkpoint_ts, Some(Some(1704067200)));
        assert_eq!(
            values.commit_subject,
            Some(Some("Initial commit".to_string()))
        );
        assert_eq!(
            values.commit_body,
            Some(Some(
                "This is the commit body\n\nWith multiple lines".to_string()
            ))
        );
    }

    #[test]
    fn test_committed_values_roundtrip_with_new_fields() {
        let original = CommittedValues::new()
            .human_additions(25)
            .first_checkpoint_ts(1700000000)
            .commit_subject("Test commit")
            .commit_body_null()
            .author_ts(1700000100)
            .commit_ts(1700000200)
            .patch_id("stable-patch-id");

        let sparse = PosEncoded::to_sparse(&original);
        let restored = <CommittedValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(restored.human_additions, Some(Some(25)));
        assert_eq!(restored.first_checkpoint_ts, Some(Some(1700000000)));
        assert_eq!(
            restored.commit_subject,
            Some(Some("Test commit".to_string()))
        );
        assert_eq!(restored.commit_body, Some(None));
        assert_eq!(restored.author_ts, Some(Some(1700000100)));
        assert_eq!(restored.commit_ts, Some(Some(1700000200)));
        assert_eq!(restored.patch_id, Some(Some("stable-patch-id".to_string())));
    }

    #[test]
    fn test_committed_values_with_hunks() {
        let hunks_json = r#"[{"commit_sha":"abc123","content_hash":"def456","hunk_kind":"addition","start_line":1,"end_line":5,"file_path":"src/main.rs"}]"#;
        let values = CommittedValues::new().human_additions(10).hunks(hunks_json);

        assert_eq!(values.hunks, Some(Some(hunks_json.to_string())));
    }

    #[test]
    fn test_committed_values_hunks_null() {
        let values = CommittedValues::new().hunks_null();
        assert_eq!(values.hunks, Some(None));
    }

    #[test]
    fn test_committed_values_hunks_roundtrip() {
        let hunks_json = r#"[{"commit_sha":"abc","content_hash":"def","hunk_kind":"addition","start_line":1,"end_line":3,"file_path":"test.rs"}]"#;
        let original = CommittedValues::new().human_additions(5).hunks(hunks_json);

        let sparse = PosEncoded::to_sparse(&original);
        assert_eq!(
            sparse.get("14"),
            Some(&Value::String(hunks_json.to_string()))
        );

        let restored = <CommittedValues as PosEncoded>::from_sparse(&sparse);
        assert_eq!(restored.hunks, Some(Some(hunks_json.to_string())));
    }

    #[test]
    fn test_committed_values_with_all_arrays() {
        let values = CommittedValues::new()
            .tool_model_pairs(vec!["all".to_string(), "cursor:gpt-4".to_string()])
            .ai_additions(vec![100, 50])
            .ai_accepted(vec![80, 40]);

        assert_eq!(
            values.tool_model_pairs,
            Some(Some(vec!["all".to_string(), "cursor:gpt-4".to_string()]))
        );
        assert_eq!(values.ai_additions, Some(Some(vec![100, 50])));
        assert_eq!(values.ai_accepted, Some(Some(vec![80, 40])));
    }

    #[test]
    fn test_committed_values_array_nulls() {
        let values = CommittedValues::new().ai_accepted_null();

        assert_eq!(values.ai_accepted, Some(None));
    }
}
