use crate::model::authorship_log::{LineRange, PromptRecord};
use crate::model::diff_json::FileDiffJson;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// File record for API - converts LineRange annotations to API format
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiFileRecord {
    /// Maps prompt_hash to line numbers/ranges
    /// Example: { "prompt_abc123": [[1, 5], 10] } means lines 1-5 and line 10 attributed to prompt_abc123
    pub annotations: HashMap<String, Vec<serde_json::Value>>,
    /// Git diff output
    pub diff: String,
    /// Original file content before changes
    #[serde(rename = "base_content")]
    pub base_content: String,
}

impl From<&FileDiffJson> for ApiFileRecord {
    fn from(file_diff: &FileDiffJson) -> Self {
        let annotations: HashMap<String, Vec<serde_json::Value>> = file_diff
            .annotations
            .iter()
            .map(|(key, ranges)| {
                let json_ranges: Vec<serde_json::Value> = ranges
                    .iter()
                    .map(|range| match range {
                        LineRange::Single(line) => serde_json::Value::Number((*line as u64).into()),
                        LineRange::Range(start, end) => serde_json::Value::Array(vec![
                            serde_json::Value::Number((*start as u64).into()),
                            serde_json::Value::Number((*end as u64).into()),
                        ]),
                    })
                    .collect();
                (key.clone(), json_ranges)
            })
            .collect();

        Self {
            annotations,
            diff: file_diff.diff.clone(),
            base_content: file_diff.base_content.clone(),
        }
    }
}

/// Bundle data containing prompts and optional files
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleData {
    /// REQUIRED: At least one prompt
    pub prompts: HashMap<String, PromptRecord>,
    /// OPTIONAL: File diffs and annotations
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub files: HashMap<String, ApiFileRecord>,
}

/// Request body for creating a bundle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateBundleRequest {
    /// Bundle title (min 1 character)
    pub title: String,
    /// Bundle data containing prompts and optional files
    pub data: BundleData,
    // TODO PR Metadata if linked to PR
}

/// Success response from bundle creation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateBundleResponse {
    pub success: bool,
    pub id: String,
    pub url: String,
}

/// Error response from API
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiErrorResponse {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Single CAS object for upload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CasObject {
    pub content: serde_json::Value,
    pub hash: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

/// Request body for CAS upload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CasUploadRequest {
    pub objects: Vec<CasObject>,
}

/// Result for a single CAS object upload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CasUploadResult {
    pub hash: String,
    pub status: String, // "ok" or "error"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response from CAS upload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CasUploadResponse {
    pub results: Vec<CasUploadResult>,
    pub success_count: usize,
    pub failure_count: usize,
}

/// Wrapper for messages stored in CAS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CasMessagesObject {
    pub messages: Vec<crate::model::transcript::Message>,
}

/// A single authorship note entry (commit SHA + content).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteEntry {
    pub commit_sha: String,
    pub content: String,
}

/// Request body for uploading notes to the HTTP backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotesUploadRequest {
    pub entries: Vec<NoteEntry>,
}

/// Response from a notes upload request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotesUploadResponse {
    pub success_count: usize,
    pub failure_count: usize,
}

/// Response from a notes read request — maps commit_sha → note content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotesReadResponse {
    pub notes: std::collections::HashMap<String, String>,
}

/// Single result from CA prompt store batch read
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CAPromptStoreReadResult {
    pub hash: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response from CA prompt store batch read
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CAPromptStoreReadResponse {
    pub results: Vec<CAPromptStoreReadResult>,
    pub success_count: usize,
    pub failure_count: usize,
}

/// Daemon diagnostics upload protocol version.
pub const DAEMON_LOGS_UPLOAD_VERSION: u8 = 1;

/// Kind of daemon diagnostic event accepted by `/worker/logs/upload`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonLogKind {
    Log,
    Heartbeat,
}

/// Log level accepted by `/worker/logs/upload`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DaemonLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Primitive daemon log field value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DaemonLogFieldValue {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null,
}

impl From<String> for DaemonLogFieldValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for DaemonLogFieldValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<u64> for DaemonLogFieldValue {
    fn from(value: u64) -> Self {
        Self::Number(serde_json::Number::from(value))
    }
}

impl From<i64> for DaemonLogFieldValue {
    fn from(value: i64) -> Self {
        Self::Number(serde_json::Number::from(value))
    }
}

impl From<bool> for DaemonLogFieldValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// Single daemon diagnostic event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonLogEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub kind: DaemonLogKind,
    pub timestamp: String,
    pub level: DaemonLogLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, DaemonLogFieldValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ai_version: Option<String>,
}

/// Request body for uploading daemon diagnostics to the HTTP backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonLogsUploadRequest {
    pub version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ai_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    pub events: Vec<DaemonLogEvent>,
}

/// Error entry returned from daemon log upload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonLogsUploadError {
    pub index: Option<usize>,
    pub error: String,
}

/// Response from daemon log upload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonLogsUploadResponse {
    pub accepted: usize,
    pub dropped: usize,
    pub enqueued: bool,
    #[serde(default)]
    pub errors: Vec<DaemonLogsUploadError>,
}

#[path = "api_types_tests.rs"]
#[cfg(test)]
mod tests;
