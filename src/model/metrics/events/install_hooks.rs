use super::super::pos_encoded::{
    PosEncoded, PosField, sparse_get_string, sparse_set, string_to_json,
};
use super::super::types::{EventValues, MetricEventId, SparseArray};

/// Value positions for "install_hooks" event.
/// One event per tool attempted during install-hooks.
pub mod install_hooks_pos {
    pub const TOOL_ID: usize = 0; // String - tool id (e.g., "cursor", "vscode")
    pub const STATUS: usize = 1; // String - "not_found", "installed", "already_installed", "failed"
    pub const MESSAGE: usize = 2; // Option<String> - error message or warnings
}

/// Values for Event ID 3: install_hooks
///
/// Recorded for each tool during git-ai install-hooks command.
/// One event per tool attempted.
///
/// **Fields:**
/// | Position | Name | Type |
/// |----------|------|------|
/// | 0 | tool_id | String |
/// | 1 | status | String |
/// | 2 | message | `Option<String>` |
#[derive(Debug, Clone, Default)]
pub struct InstallHooksValues {
    pub tool_id: PosField<String>,
    pub status: PosField<String>,
    pub message: PosField<String>,
}

impl InstallHooksValues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tool_id(mut self, value: String) -> Self {
        self.tool_id = Some(Some(value));
        self
    }

    pub fn status(mut self, value: String) -> Self {
        self.status = Some(Some(value));
        self
    }

    pub fn message(mut self, value: String) -> Self {
        self.message = Some(Some(value));
        self
    }

    pub fn message_null(mut self) -> Self {
        self.message = Some(None);
        self
    }
}

impl PosEncoded for InstallHooksValues {
    fn to_sparse(&self) -> SparseArray {
        let mut map = SparseArray::new();

        sparse_set(
            &mut map,
            install_hooks_pos::TOOL_ID,
            string_to_json(&self.tool_id),
        );
        sparse_set(
            &mut map,
            install_hooks_pos::STATUS,
            string_to_json(&self.status),
        );
        sparse_set(
            &mut map,
            install_hooks_pos::MESSAGE,
            string_to_json(&self.message),
        );

        map
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        Self {
            tool_id: sparse_get_string(arr, install_hooks_pos::TOOL_ID),
            status: sparse_get_string(arr, install_hooks_pos::STATUS),
            message: sparse_get_string(arr, install_hooks_pos::MESSAGE),
        }
    }
}

impl EventValues for InstallHooksValues {
    fn event_id() -> MetricEventId {
        MetricEventId::InstallHooks
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
    fn test_install_hooks_values_builder() {
        let values = InstallHooksValues::new()
            .tool_id("cursor".to_string())
            .status("installed".to_string())
            .message("Successfully installed".to_string());

        assert_eq!(values.tool_id, Some(Some("cursor".to_string())));
        assert_eq!(values.status, Some(Some("installed".to_string())));
        assert_eq!(
            values.message,
            Some(Some("Successfully installed".to_string()))
        );
    }

    #[test]
    fn test_install_hooks_values_with_null_message() {
        let values = InstallHooksValues::new()
            .tool_id("vscode".to_string())
            .status("not_found".to_string())
            .message_null();

        assert_eq!(values.message, Some(None));
    }

    #[test]
    fn test_install_hooks_values_to_sparse() {
        let values = InstallHooksValues::new()
            .tool_id("copilot".to_string())
            .status("failed".to_string())
            .message("Error: permission denied".to_string());

        let sparse = PosEncoded::to_sparse(&values);

        assert_eq!(sparse.get("0"), Some(&Value::String("copilot".to_string())));
        assert_eq!(sparse.get("1"), Some(&Value::String("failed".to_string())));
        assert_eq!(
            sparse.get("2"),
            Some(&Value::String("Error: permission denied".to_string()))
        );
    }

    #[test]
    fn test_install_hooks_values_from_sparse() {
        let mut sparse = SparseArray::new();
        sparse.insert("0".to_string(), Value::String("windsurf".to_string()));
        sparse.insert(
            "1".to_string(),
            Value::String("already_installed".to_string()),
        );
        sparse.insert("2".to_string(), Value::Null);

        let values = <InstallHooksValues as PosEncoded>::from_sparse(&sparse);

        assert_eq!(values.tool_id, Some(Some("windsurf".to_string())));
        assert_eq!(values.status, Some(Some("already_installed".to_string())));
        assert_eq!(values.message, Some(None));
    }

    #[test]
    fn test_install_hooks_event_id() {
        assert_eq!(InstallHooksValues::event_id(), MetricEventId::InstallHooks);
        assert_eq!(InstallHooksValues::event_id() as u16, 3);
    }
}
