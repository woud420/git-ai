use super::super::pos_encoded::{
    PosEncoded, PosField, sparse_get_string, sparse_get_u32, sparse_get_u64, sparse_set,
    string_to_json, u32_to_json, u64_to_json,
};
use super::super::types::{EventValues, MetricEventId, SparseArray};

/// Value positions for "checkpoint" event.
/// One event per file in the checkpoint.
pub mod checkpoint_pos {
    pub const CHECKPOINT_TS: usize = 0; // u64 - checkpoint timestamp
    pub const KIND: usize = 1; // String ("human", "ai_agent", "ai_tab")
    pub const FILE_PATH: usize = 2; // String - full relative file path
    pub const LINES_ADDED: usize = 3; // u32 - for this file
    pub const LINES_DELETED: usize = 4; // u32 - for this file
    pub const LINES_ADDED_SLOC: usize = 5; // u32 - for this file
    pub const LINES_DELETED_SLOC: usize = 6; // u32 - for this file
    pub const TOOL_USE_ID: usize = 7; // String - nullable
    pub const EDIT_KIND: usize = 8; // String - nullable ("file_edit" | "bash")
    pub const CHECKPOINT_TYPE: usize = 9; // String - nullable ("recovered_bash", etc.)
    pub const ATTRIBUTION_RECOVERY_METADATA: usize = 10; // String - nullable JSON
}

/// Values for Event ID 4: checkpoint
///
/// Recorded for each file in a checkpoint.
/// Uses EventAttributes for standard metadata (repo_url, author, tool, model, etc.)
///
/// **Fields:**
/// | Position | Name | Type |
/// |----------|------|------|
/// | 0 | checkpoint_ts | u64 |
/// | 1 | kind | String |
/// | 2 | file_path | String |
/// | 3 | lines_added | u32 |
/// | 4 | lines_deleted | u32 |
/// | 5 | lines_added_sloc | u32 |
/// | 6 | lines_deleted_sloc | u32 |
/// | 7 | external_tool_use_id | String (nullable) |
/// | 8 | edit_kind | String (nullable) |
/// | 9 | checkpoint_type | String (nullable) |
/// | 10 | attribution_recovery_metadata | String (nullable JSON) |
#[derive(Debug, Clone, Default)]
pub struct CheckpointValues {
    pub checkpoint_ts: PosField<u64>,
    pub kind: PosField<String>,
    pub file_path: PosField<String>,
    pub lines_added: PosField<u32>,
    pub lines_deleted: PosField<u32>,
    pub lines_added_sloc: PosField<u32>,
    pub lines_deleted_sloc: PosField<u32>,
    pub external_tool_use_id: PosField<String>,
    pub edit_kind: PosField<String>,
    pub checkpoint_type: PosField<String>,
    pub attribution_recovery_metadata: PosField<String>,
}

impl CheckpointValues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn checkpoint_ts(mut self, value: u64) -> Self {
        self.checkpoint_ts = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn checkpoint_ts_null(mut self) -> Self {
        self.checkpoint_ts = Some(None);
        self
    }

    pub fn kind(mut self, value: impl Into<String>) -> Self {
        self.kind = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn kind_null(mut self) -> Self {
        self.kind = Some(None);
        self
    }

    pub fn file_path(mut self, value: impl Into<String>) -> Self {
        self.file_path = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn file_path_null(mut self) -> Self {
        self.file_path = Some(None);
        self
    }

    pub fn lines_added(mut self, value: u32) -> Self {
        self.lines_added = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn lines_added_null(mut self) -> Self {
        self.lines_added = Some(None);
        self
    }

    pub fn lines_deleted(mut self, value: u32) -> Self {
        self.lines_deleted = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn lines_deleted_null(mut self) -> Self {
        self.lines_deleted = Some(None);
        self
    }

    pub fn lines_added_sloc(mut self, value: u32) -> Self {
        self.lines_added_sloc = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn lines_added_sloc_null(mut self) -> Self {
        self.lines_added_sloc = Some(None);
        self
    }

    pub fn lines_deleted_sloc(mut self, value: u32) -> Self {
        self.lines_deleted_sloc = Some(Some(value));
        self
    }

    #[allow(dead_code)]
    pub fn lines_deleted_sloc_null(mut self) -> Self {
        self.lines_deleted_sloc = Some(None);
        self
    }

    pub fn external_tool_use_id(mut self, value: impl Into<String>) -> Self {
        self.external_tool_use_id = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn external_tool_use_id_null(mut self) -> Self {
        self.external_tool_use_id = Some(None);
        self
    }

    pub fn edit_kind(mut self, value: impl Into<String>) -> Self {
        self.edit_kind = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn edit_kind_null(mut self) -> Self {
        self.edit_kind = Some(None);
        self
    }

    pub fn checkpoint_type(mut self, value: impl Into<String>) -> Self {
        self.checkpoint_type = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn checkpoint_type_null(mut self) -> Self {
        self.checkpoint_type = Some(None);
        self
    }

    pub fn attribution_recovery_metadata(mut self, value: impl Into<String>) -> Self {
        self.attribution_recovery_metadata = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn attribution_recovery_metadata_null(mut self) -> Self {
        self.attribution_recovery_metadata = Some(None);
        self
    }
}

impl PosEncoded for CheckpointValues {
    fn to_sparse(&self) -> SparseArray {
        let mut map = SparseArray::new();

        sparse_set(
            &mut map,
            checkpoint_pos::CHECKPOINT_TS,
            u64_to_json(&self.checkpoint_ts),
        );
        sparse_set(&mut map, checkpoint_pos::KIND, string_to_json(&self.kind));
        sparse_set(
            &mut map,
            checkpoint_pos::FILE_PATH,
            string_to_json(&self.file_path),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::LINES_ADDED,
            u32_to_json(&self.lines_added),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::LINES_DELETED,
            u32_to_json(&self.lines_deleted),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::LINES_ADDED_SLOC,
            u32_to_json(&self.lines_added_sloc),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::LINES_DELETED_SLOC,
            u32_to_json(&self.lines_deleted_sloc),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::TOOL_USE_ID,
            string_to_json(&self.external_tool_use_id),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::EDIT_KIND,
            string_to_json(&self.edit_kind),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::CHECKPOINT_TYPE,
            string_to_json(&self.checkpoint_type),
        );
        sparse_set(
            &mut map,
            checkpoint_pos::ATTRIBUTION_RECOVERY_METADATA,
            string_to_json(&self.attribution_recovery_metadata),
        );

        map
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        Self {
            checkpoint_ts: sparse_get_u64(arr, checkpoint_pos::CHECKPOINT_TS),
            kind: sparse_get_string(arr, checkpoint_pos::KIND),
            file_path: sparse_get_string(arr, checkpoint_pos::FILE_PATH),
            lines_added: sparse_get_u32(arr, checkpoint_pos::LINES_ADDED),
            lines_deleted: sparse_get_u32(arr, checkpoint_pos::LINES_DELETED),
            lines_added_sloc: sparse_get_u32(arr, checkpoint_pos::LINES_ADDED_SLOC),
            lines_deleted_sloc: sparse_get_u32(arr, checkpoint_pos::LINES_DELETED_SLOC),
            external_tool_use_id: sparse_get_string(arr, checkpoint_pos::TOOL_USE_ID),
            edit_kind: sparse_get_string(arr, checkpoint_pos::EDIT_KIND),
            checkpoint_type: sparse_get_string(arr, checkpoint_pos::CHECKPOINT_TYPE),
            attribution_recovery_metadata: sparse_get_string(
                arr,
                checkpoint_pos::ATTRIBUTION_RECOVERY_METADATA,
            ),
        }
    }
}

impl EventValues for CheckpointValues {
    fn event_id() -> MetricEventId {
        MetricEventId::Checkpoint
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
    fn test_checkpoint_values_builder() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1704067200)
            .kind("ai_agent")
            .file_path("src/main.rs")
            .lines_added(50)
            .lines_deleted(10)
            .lines_added_sloc(45)
            .lines_deleted_sloc(8);

        assert_eq!(values.checkpoint_ts, Some(Some(1704067200)));
        assert_eq!(values.kind, Some(Some("ai_agent".to_string())));
        assert_eq!(values.file_path, Some(Some("src/main.rs".to_string())));
        assert_eq!(values.lines_added, Some(Some(50)));
        assert_eq!(values.lines_deleted, Some(Some(10)));
        assert_eq!(values.lines_added_sloc, Some(Some(45)));
        assert_eq!(values.lines_deleted_sloc, Some(Some(8)));
    }

    #[test]
    fn test_checkpoint_values_with_nulls() {
        let values = CheckpointValues::new()
            .checkpoint_ts_null()
            .kind_null()
            .file_path_null()
            .lines_added_null();

        assert_eq!(values.checkpoint_ts, Some(None));
        assert_eq!(values.kind, Some(None));
        assert_eq!(values.file_path, Some(None));
        assert_eq!(values.lines_added, Some(None));
    }

    #[test]
    fn test_checkpoint_values_to_sparse() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1700000000)
            .kind("human")
            .file_path("tests/test.rs")
            .lines_added(100)
            .lines_deleted(20);

        let sparse = PosEncoded::to_sparse(&values);

        assert_eq!(sparse.get("0"), Some(&Value::Number(1700000000.into())));
        assert_eq!(sparse.get("1"), Some(&Value::String("human".to_string())));
        assert_eq!(
            sparse.get("2"),
            Some(&Value::String("tests/test.rs".to_string()))
        );
        assert_eq!(sparse.get("3"), Some(&Value::Number(100.into())));
        assert_eq!(sparse.get("4"), Some(&Value::Number(20.into())));
    }

    #[test]
    fn test_checkpoint_values_from_sparse() {
        let mut sparse = SparseArray::new();
        sparse.insert("0".to_string(), Value::Number(1704067200.into()));
        sparse.insert("1".to_string(), Value::String("ai_tab".to_string()));
        sparse.insert("2".to_string(), Value::String("lib.rs".to_string()));
        sparse.insert("3".to_string(), Value::Number(75.into()));
        sparse.insert("4".to_string(), Value::Number(15.into()));
        sparse.insert("5".to_string(), Value::Number(70.into()));
        sparse.insert("6".to_string(), Value::Number(12.into()));

        let values = <CheckpointValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(values.checkpoint_ts, Some(Some(1704067200)));
        assert_eq!(values.kind, Some(Some("ai_tab".to_string())));
        assert_eq!(values.file_path, Some(Some("lib.rs".to_string())));
        assert_eq!(values.lines_added, Some(Some(75)));
        assert_eq!(values.lines_deleted, Some(Some(15)));
        assert_eq!(values.lines_added_sloc, Some(Some(70)));
        assert_eq!(values.lines_deleted_sloc, Some(Some(12)));
    }

    #[test]
    fn test_checkpoint_event_id() {
        assert_eq!(CheckpointValues::event_id(), MetricEventId::Checkpoint);
        assert_eq!(CheckpointValues::event_id() as u16, 4);
    }

    #[test]
    fn test_checkpoint_values_with_external_tool_use_id() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1704067200)
            .kind("ai_agent")
            .file_path("src/main.rs")
            .lines_added(50)
            .external_tool_use_id("tool-use-123");

        assert_eq!(
            values.external_tool_use_id,
            Some(Some("tool-use-123".to_string()))
        );
    }

    #[test]
    fn test_checkpoint_values_external_tool_use_id_null() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1704067200)
            .kind("human")
            .external_tool_use_id_null();

        assert_eq!(values.external_tool_use_id, Some(None));
    }

    #[test]
    fn test_checkpoint_values_to_sparse_with_external_tool_use_id() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1700000000)
            .kind("ai_agent")
            .file_path("tests/test.rs")
            .lines_added(100)
            .external_tool_use_id("tool-xyz");

        let sparse = PosEncoded::to_sparse(&values);

        assert_eq!(sparse.get("0"), Some(&Value::Number(1700000000.into())));
        assert_eq!(
            sparse.get("1"),
            Some(&Value::String("ai_agent".to_string()))
        );
        assert_eq!(
            sparse.get("2"),
            Some(&Value::String("tests/test.rs".to_string()))
        );
        assert_eq!(sparse.get("3"), Some(&Value::Number(100.into())));
        assert_eq!(
            sparse.get("7"),
            Some(&Value::String("tool-xyz".to_string()))
        );
    }

    #[test]
    fn test_checkpoint_values_from_sparse_with_external_tool_use_id() {
        let mut sparse = SparseArray::new();
        sparse.insert("0".to_string(), Value::Number(1704067200.into()));
        sparse.insert("1".to_string(), Value::String("ai_tab".to_string()));
        sparse.insert("2".to_string(), Value::String("lib.rs".to_string()));
        sparse.insert("3".to_string(), Value::Number(75.into()));
        sparse.insert("7".to_string(), Value::String("tool-abc".to_string()));

        let values = <CheckpointValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(values.checkpoint_ts, Some(Some(1704067200)));
        assert_eq!(values.kind, Some(Some("ai_tab".to_string())));
        assert_eq!(values.file_path, Some(Some("lib.rs".to_string())));
        assert_eq!(values.lines_added, Some(Some(75)));
        assert_eq!(
            values.external_tool_use_id,
            Some(Some("tool-abc".to_string()))
        );
    }

    #[test]
    fn test_checkpoint_values_roundtrip_with_external_tool_use_id() {
        let original = CheckpointValues::new()
            .checkpoint_ts(1700000000)
            .kind("ai_agent")
            .file_path("src/lib.rs")
            .lines_added(50)
            .external_tool_use_id_null();

        let sparse = PosEncoded::to_sparse(&original);
        let restored = <CheckpointValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(restored.checkpoint_ts, Some(Some(1700000000)));
        assert_eq!(restored.kind, Some(Some("ai_agent".to_string())));
        assert_eq!(restored.file_path, Some(Some("src/lib.rs".to_string())));
        assert_eq!(restored.lines_added, Some(Some(50)));
        assert_eq!(restored.external_tool_use_id, Some(None)); // explicitly null
    }

    #[test]
    fn test_checkpoint_values_external_tool_use_id_not_set() {
        let mut sparse = SparseArray::new();
        sparse.insert("0".to_string(), Value::Number(1700000000.into()));
        sparse.insert("1".to_string(), Value::String("human".to_string()));
        // external_tool_use_id not included

        let values = <CheckpointValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(values.external_tool_use_id, None); // not set
    }

    #[test]
    fn test_checkpoint_values_with_edit_kind() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1704067200)
            .kind("ai_agent")
            .file_path("src/main.rs")
            .edit_kind("file_edit");

        assert_eq!(values.edit_kind, Some(Some("file_edit".to_string())));
    }

    #[test]
    fn test_checkpoint_values_edit_kind_null() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1704067200)
            .kind("ai_agent")
            .edit_kind_null();

        assert_eq!(values.edit_kind, Some(None));
    }

    #[test]
    fn test_checkpoint_values_with_recovery_metadata() {
        let values = CheckpointValues::new()
            .checkpoint_type("recovered_bash")
            .attribution_recovery_metadata(r#"{"solver":"bash_mtime"}"#);

        let sparse = PosEncoded::to_sparse(&values);
        assert_eq!(
            sparse.get("9"),
            Some(&Value::String("recovered_bash".to_string()))
        );
        assert_eq!(
            sparse.get("10"),
            Some(&Value::String(r#"{"solver":"bash_mtime"}"#.to_string()))
        );

        let restored = <CheckpointValues as PosEncoded>::from_sparse(&sparse);
        assert_eq!(
            restored.checkpoint_type,
            Some(Some("recovered_bash".to_string()))
        );
        assert_eq!(
            restored.attribution_recovery_metadata,
            Some(Some(r#"{"solver":"bash_mtime"}"#.to_string()))
        );
    }

    #[test]
    fn test_checkpoint_values_to_sparse_with_edit_kind() {
        let values = CheckpointValues::new()
            .checkpoint_ts(1700000000)
            .kind("ai_agent")
            .file_path("tests/test.rs")
            .edit_kind("bash");

        let sparse = PosEncoded::to_sparse(&values);

        assert_eq!(sparse.get("0"), Some(&Value::Number(1700000000.into())));
        assert_eq!(
            sparse.get("1"),
            Some(&Value::String("ai_agent".to_string()))
        );
        assert_eq!(sparse.get("8"), Some(&Value::String("bash".to_string())));
    }

    #[test]
    fn test_checkpoint_values_from_sparse_with_edit_kind() {
        let mut sparse = SparseArray::new();
        sparse.insert("0".to_string(), Value::Number(1704067200.into()));
        sparse.insert("1".to_string(), Value::String("ai_agent".to_string()));
        sparse.insert("2".to_string(), Value::String("lib.rs".to_string()));
        sparse.insert("8".to_string(), Value::String("file_edit".to_string()));

        let values = <CheckpointValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(values.checkpoint_ts, Some(Some(1704067200)));
        assert_eq!(values.kind, Some(Some("ai_agent".to_string())));
        assert_eq!(values.edit_kind, Some(Some("file_edit".to_string())));
    }

    #[test]
    fn test_checkpoint_values_roundtrip_with_edit_kind() {
        let original = CheckpointValues::new()
            .checkpoint_ts(1700000000)
            .kind("ai_agent")
            .file_path("src/lib.rs")
            .lines_added(50)
            .edit_kind("bash");

        let sparse = PosEncoded::to_sparse(&original);
        let restored = <CheckpointValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(restored.checkpoint_ts, Some(Some(1700000000)));
        assert_eq!(restored.kind, Some(Some("ai_agent".to_string())));
        assert_eq!(restored.file_path, Some(Some("src/lib.rs".to_string())));
        assert_eq!(restored.lines_added, Some(Some(50)));
        assert_eq!(restored.edit_kind, Some(Some("bash".to_string())));
    }

    #[test]
    fn test_checkpoint_values_edit_kind_not_set() {
        let mut sparse = SparseArray::new();
        sparse.insert("0".to_string(), Value::Number(1700000000.into()));
        sparse.insert("1".to_string(), Value::String("human".to_string()));

        let values = <CheckpointValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(values.edit_kind, None);
    }
}
