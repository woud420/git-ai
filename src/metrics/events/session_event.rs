use super::super::pos_encoded::PosEncoded;
use super::super::types::{EventValues, MetricEventId, SparseArray};

/// Value positions for "session_event" event.
pub mod session_event_pos {
    pub const RAW_JSON: usize = 0;
    pub const EXTERNAL_EVENT_ID: usize = 1;
    pub const EXTERNAL_PARENT_EVENT_ID: usize = 2;
    pub const EXTERNAL_TOOL_USE_ID: usize = 3;
}

/// Values for Event ID 5: session_event
///
/// Each event is the raw JSON from the agent's transcript file, stored at position 0.
/// Uses EventAttributes for session_id, trace_id, tool metadata.
#[derive(Debug, Clone, Default)]
pub struct SessionEventValues {
    pub raw_json: serde_json::Value,
    pub external_event_id: Option<String>,
    pub external_parent_event_id: Option<String>,
    pub external_tool_use_id: Option<String>,
}

impl SessionEventValues {
    pub fn new(raw_json: serde_json::Value) -> Self {
        Self {
            raw_json,
            external_event_id: None,
            external_parent_event_id: None,
            external_tool_use_id: None,
        }
    }

    pub fn with_ids(
        raw_json: serde_json::Value,
        external_event_id: Option<String>,
        external_parent_event_id: Option<String>,
        external_tool_use_id: Option<String>,
    ) -> Self {
        Self {
            raw_json,
            external_event_id,
            external_parent_event_id,
            external_tool_use_id,
        }
    }
}

impl PosEncoded for SessionEventValues {
    fn to_sparse(&self) -> SparseArray {
        let mut map = SparseArray::new();
        map.insert(
            session_event_pos::RAW_JSON.to_string(),
            self.raw_json.clone(),
        );
        if let Some(ref id) = self.external_event_id {
            map.insert(
                session_event_pos::EXTERNAL_EVENT_ID.to_string(),
                serde_json::Value::String(id.clone()),
            );
        }
        if let Some(ref id) = self.external_parent_event_id {
            map.insert(
                session_event_pos::EXTERNAL_PARENT_EVENT_ID.to_string(),
                serde_json::Value::String(id.clone()),
            );
        }
        if let Some(ref id) = self.external_tool_use_id {
            map.insert(
                session_event_pos::EXTERNAL_TOOL_USE_ID.to_string(),
                serde_json::Value::String(id.clone()),
            );
        }
        map
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        let raw_json = arr
            .get(&session_event_pos::RAW_JSON.to_string())
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let external_event_id = arr
            .get(&session_event_pos::EXTERNAL_EVENT_ID.to_string())
            .and_then(|v| v.as_str())
            .map(String::from);
        let external_parent_event_id = arr
            .get(&session_event_pos::EXTERNAL_PARENT_EVENT_ID.to_string())
            .and_then(|v| v.as_str())
            .map(String::from);
        let external_tool_use_id = arr
            .get(&session_event_pos::EXTERNAL_TOOL_USE_ID.to_string())
            .and_then(|v| v.as_str())
            .map(String::from);
        Self {
            raw_json,
            external_event_id,
            external_parent_event_id,
            external_tool_use_id,
        }
    }
}

impl EventValues for SessionEventValues {
    fn event_id() -> MetricEventId {
        MetricEventId::SessionEvent
    }

    fn to_sparse(&self) -> SparseArray {
        PosEncoded::to_sparse(self)
    }

    fn into_sparse(self) -> SparseArray {
        let mut map = SparseArray::new();
        map.insert(session_event_pos::RAW_JSON.to_string(), self.raw_json);
        if let Some(id) = self.external_event_id {
            map.insert(
                session_event_pos::EXTERNAL_EVENT_ID.to_string(),
                serde_json::Value::String(id),
            );
        }
        if let Some(id) = self.external_parent_event_id {
            map.insert(
                session_event_pos::EXTERNAL_PARENT_EVENT_ID.to_string(),
                serde_json::Value::String(id),
            );
        }
        if let Some(id) = self.external_tool_use_id {
            map.insert(
                session_event_pos::EXTERNAL_TOOL_USE_ID.to_string(),
                serde_json::Value::String(id),
            );
        }
        map
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        PosEncoded::from_sparse(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_event_values_new() {
        let raw = serde_json::json!({"type": "user", "uuid": "abc"});
        let values = SessionEventValues::new(raw.clone());
        assert_eq!(values.raw_json, raw);
        assert_eq!(values.external_event_id, None);
        assert_eq!(values.external_parent_event_id, None);
        assert_eq!(values.external_tool_use_id, None);
    }

    #[test]
    fn test_session_event_values_with_ids() {
        let raw = serde_json::json!({"type": "assistant"});
        let values = SessionEventValues::with_ids(
            raw.clone(),
            Some("evt-123".to_string()),
            Some("parent-456".to_string()),
            Some("toolu_789".to_string()),
        );

        assert_eq!(values.raw_json, raw);
        assert_eq!(values.external_event_id, Some("evt-123".to_string()));
        assert_eq!(
            values.external_parent_event_id,
            Some("parent-456".to_string())
        );
        assert_eq!(values.external_tool_use_id, Some("toolu_789".to_string()));
    }

    #[test]
    fn test_session_event_values_sparse_roundtrip_with_ids() {
        let raw = serde_json::json!({"type": "assistant", "data": 42});
        let values = SessionEventValues::with_ids(
            raw.clone(),
            Some("event-id".to_string()),
            Some("parent-id".to_string()),
            Some("tool-use-id".to_string()),
        );

        let sparse = PosEncoded::to_sparse(&values);
        assert_eq!(sparse.get("0"), Some(&raw));
        assert_eq!(
            sparse.get("1"),
            Some(&serde_json::Value::String("event-id".to_string()))
        );
        assert_eq!(
            sparse.get("2"),
            Some(&serde_json::Value::String("parent-id".to_string()))
        );
        assert_eq!(
            sparse.get("3"),
            Some(&serde_json::Value::String("tool-use-id".to_string()))
        );

        let restored = <SessionEventValues as PosEncoded>::from_sparse(&sparse);
        assert_eq!(restored.raw_json, raw);
        assert_eq!(restored.external_event_id, Some("event-id".to_string()));
        assert_eq!(
            restored.external_parent_event_id,
            Some("parent-id".to_string())
        );
        assert_eq!(
            restored.external_tool_use_id,
            Some("tool-use-id".to_string())
        );
    }

    #[test]
    fn test_session_event_values_sparse_none_ids_omitted() {
        let raw = serde_json::json!({"type": "user"});
        let values = SessionEventValues::new(raw.clone());

        let sparse = PosEncoded::to_sparse(&values);
        assert_eq!(sparse.get("0"), Some(&raw));
        assert_eq!(sparse.get("1"), None);
        assert_eq!(sparse.get("2"), None);
        assert_eq!(sparse.get("3"), None);
    }

    #[test]
    fn test_session_event_values_into_sparse_with_ids() {
        let raw = serde_json::json!({"msg": "hello"});
        let values = SessionEventValues::with_ids(
            raw.clone(),
            Some("eid".to_string()),
            None,
            Some("tid".to_string()),
        );

        let sparse = EventValues::into_sparse(values);
        assert_eq!(sparse.get("0"), Some(&raw));
        assert_eq!(
            sparse.get("1"),
            Some(&serde_json::Value::String("eid".to_string()))
        );
        assert_eq!(sparse.get("2"), None);
        assert_eq!(
            sparse.get("3"),
            Some(&serde_json::Value::String("tid".to_string()))
        );
    }
}
