//! JSON DTO for per-file diff output (`git-ai diff --json`).
//!
//! This is a pure serialization shape shared between the diff command
//! (which produces it) and the API layer (which converts it to
//! [`crate::model::api_types::ApiFileRecord`]). It lives in `model` so the
//! domain does not have to reach up into `operations::commands::diff`.

use crate::model::authorship_log::{HumanRecord, LineRange, PromptRecord, SessionRecord};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Per-file diff information in JSON output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiffJson {
    /// Annotations mapping prompt hash to line ranges
    /// Line ranges are serialized as JSON tuples: [start, end] or single number
    #[serde(serialize_with = "serialize_annotations")]
    pub annotations: BTreeMap<String, Vec<LineRange>>,
    /// The unified diff for this file
    pub diff: String,
    /// The base content of the file (before changes)
    pub base_content: String,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub struct DiffLineKey {
    pub file: String,
    pub line: u32,
    pub side: LineSide,
}

/// JSON output format for `git-ai diff --json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffJson {
    /// Per-file diff information with annotations
    pub files: BTreeMap<String, FileDiffJson>,
    /// Prompt records keyed by prompt hash (old-format, bare 16-char hex)
    pub prompts: BTreeMap<String, PromptRecord>,
    /// Session records keyed by full attestation hash (s_xxx::t_yyy)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sessions: BTreeMap<String, SessionRecord>,
    /// Human records keyed by human hash (h_-prefixed)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub humans: BTreeMap<String, HumanRecord>,
    /// Per-hunk records for machine consumption
    #[serde(default)]
    pub hunks: Vec<DiffJsonHunk>,
    /// Commit metadata for all commits referenced by hunks
    #[serde(default)]
    pub commits: BTreeMap<String, DiffCommitMetadata>,
    /// Optional commit stats for single-commit diffs (`--json --include-stats`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_stats: Option<DiffCommitStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DiffToolModelStats {
    #[serde(default)]
    pub ai_lines_added: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DiffCommitStats {
    #[serde(default)]
    pub ai_lines_added: u32,
    #[serde(default)]
    pub human_lines_added: u32,
    #[serde(default)]
    pub unknown_lines_added: u32,
    #[serde(default)]
    pub git_lines_added: u32,
    #[serde(default)]
    pub git_lines_deleted: u32,
    #[serde(default)]
    pub tool_model_breakdown: BTreeMap<String, DiffToolModelStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffJsonHunk {
    pub commit_sha: String,
    pub content_hash: String,
    pub hunk_kind: String, // "addition" | "deletion"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_commit_sha: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffCommitMetadata {
    pub authored_time: String,
    pub msg: String,
    pub full_msg: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorship_note: Option<String>,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum LineSide {
    Old, // For deleted lines
    New, // For added lines
}

#[derive(Debug, Clone)]
pub enum Attribution {
    Ai(String),    // Tool name: "cursor", "claude", etc.
    Human(String), // Username
    NoData,        // No authorship data available
}

#[derive(Debug)]
pub struct DiffBuildArtifacts {
    pub attributions: HashMap<DiffLineKey, Attribution>,
    pub annotations_by_file: BTreeMap<String, BTreeMap<String, Vec<LineRange>>>,
    pub prompts: BTreeMap<String, PromptRecord>,
    pub sessions: BTreeMap<String, SessionRecord>,
    pub humans: BTreeMap<String, HumanRecord>,
    pub json_hunks: Vec<DiffJsonHunk>,
    pub commits: BTreeMap<String, DiffCommitMetadata>,
    pub included_files: HashSet<String>,
}

fn serialize_annotations<S>(
    annotations: &BTreeMap<String, Vec<LineRange>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(annotations.len()))?;
    for (key, ranges) in annotations {
        let json_ranges: Vec<serde_json::Value> = ranges
            .iter()
            .map(|range| match range {
                LineRange::Single(line) => serde_json::Value::Number((*line).into()),
                LineRange::Range(start, end) => serde_json::Value::Array(vec![
                    serde_json::Value::Number((*start).into()),
                    serde_json::Value::Number((*end).into()),
                ]),
            })
            .collect();
        map.serialize_entry(key, &json_ranges)?;
    }
    map.end()
}
