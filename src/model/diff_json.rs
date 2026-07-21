//! JSON DTO for per-file diff output (`git-ai diff --json`).
//!
//! This is a pure serialization shape shared between the diff command
//! (which produces it) and the API layer (which converts it to
//! [`crate::model::api_types::ApiFileRecord`]). It lives in `model` so the
//! domain does not have to reach up into `operations::commands::diff`.

use crate::model::authorship_log::LineRange;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;

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
