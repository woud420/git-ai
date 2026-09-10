use serde::{Deserialize, Serialize};
use std::fmt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod paths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JjObserverTarget {
    pub source_id: String,
    pub initialization_receipt_id: String,
    pub reader_profile: String,
    pub baseline_id: String,
    pub baseline_generation: u64,
    pub workspace_name: String,
    pub attachment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JjObserverError {
    pub code: String,
    pub message: String,
    pub persisted: bool,
}

impl JjObserverError {
    pub fn new(code: &str, message: impl fmt::Display) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_string().chars().take(1024).collect(),
            persisted: false,
        }
    }
}

impl fmt::Display for JjObserverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for JjObserverError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JjObserverSessionCursor {
    pub generation: u64,
    pub admitted_head_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JjObserverControlReply {
    pub schema_version: u16,
    pub backend: String,
    pub attribution_enabled: bool,
    pub action: String,
    pub disposition: String,
    pub revision: Option<u64>,
    pub desired_intent: Option<String>,
    pub target: Option<JjObserverTarget>,
    pub runtime: String,
    pub in_flight: bool,
    pub session_cursor: Option<JjObserverSessionCursor>,
    pub last_error: Option<JjObserverError>,
    pub error: Option<JjObserverError>,
}
