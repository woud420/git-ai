use super::super::pos_encoded::PosEncoded;
use super::super::types::{EventValues, MetricEventId, SparseArray};

/// Values for Event ID 2: agent_usage
///
/// Recorded on every AI checkpoint to track agent usage.
/// Uses attributes (prompt_id, tool, model) rather than event-specific values.
#[derive(Debug, Clone, Default)]
pub struct AgentUsageValues {}

impl AgentUsageValues {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PosEncoded for AgentUsageValues {
    fn to_sparse(&self) -> SparseArray {
        SparseArray::new()
    }

    fn from_sparse(_arr: &SparseArray) -> Self {
        Self::default()
    }
}

impl EventValues for AgentUsageValues {
    fn event_id() -> MetricEventId {
        MetricEventId::AgentUsage
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

    #[test]
    fn test_agent_usage_values() {
        let values = AgentUsageValues::new();
        assert_eq!(AgentUsageValues::event_id(), MetricEventId::AgentUsage);
        assert_eq!(AgentUsageValues::event_id() as u16, 2);

        // Should produce empty sparse array
        let sparse = PosEncoded::to_sparse(&values);
        assert!(sparse.is_empty());
    }

    #[test]
    fn test_agent_usage_values_roundtrip() {
        let original = AgentUsageValues::new();
        let sparse = PosEncoded::to_sparse(&original);
        let restored = <AgentUsageValues as PosEncoded>::from_sparse(&sparse);

        // Both should be empty
        assert!(PosEncoded::to_sparse(&restored).is_empty());
    }
}
