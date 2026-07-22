//! Cube name constants, query field lists, and filter helpers for the
//! `sessions` command.

use serde_json::{Value, json};

pub(super) const SESSIONS_CUBE: &str = "public_v1_sessions";
pub(super) const EVENTS_CUBE: &str = "public_v1_normalized_events";
pub(super) const TOKEN_USAGE_CUBE: &str = "public_v1_token_usage";
pub(super) const SESSION_MODELS_CUBE: &str = "public_v1_session_models";
pub(super) const PR_SESSIONS_CUBE: &str = "public_v1_pr_sessions";

/// Fully-qualify a list of cube members as `<cube>.<member>`.
pub(super) fn qualify(cube: &str, members: &[&str]) -> Vec<String> {
    members.iter().map(|m| format!("{}.{}", cube, m)).collect()
}

pub(super) fn session_dimensions() -> Vec<String> {
    // session_start_time is a plain dimension (not a bare timeDimension) so that
    // `order` by it is honored — Cube ignores ordering on a timeDimension that
    // has no granularity. The `--since` dateRange still filters via timeDimensions.
    qualify(
        SESSIONS_CUBE,
        &[
            "session_id",
            "user_id",
            "agent",
            "repo_url",
            "parent_session_id",
            "child_session_count",
            "session_start_time",
            "session_end_time",
        ],
    )
}

pub(super) fn session_measures() -> Vec<String> {
    qualify(
        SESSIONS_CUBE,
        &[
            "total_generated_lines",
            "total_deleted_lines",
            "net_generated_lines",
            "total_generated_sloc",
            "total_committed_lines",
            "total_pr_opened_lines",
            "total_merged_lines",
            "total_production_lines",
            "total_checkpoints",
            "total_events",
            "total_usage_minutes",
        ],
    )
}

pub(super) fn event_dimensions() -> Vec<String> {
    qualify(
        EVENTS_CUBE,
        &[
            "event_kind",
            "tool",
            "tool_kind",
            "model",
            "target",
            "text",
            "summary",
            "tool_input",
            "tool_output",
            "event_time",
            "output_seq",
        ],
    )
}

/// Build Cube equals-filter objects from (member, optional value) pairs,
/// skipping any pair whose value is `None`. Returned as a `Vec` so callers can
/// merge these convenience filters with a raw `--filters` array before
/// serializing the combined `filters` clause.
pub(super) fn equals_filters(pairs: &[(String, Option<&str>)]) -> Vec<Value> {
    pairs
        .iter()
        .filter_map(|(member, value)| {
            value.map(|v| json!({ "member": member, "operator": "equals", "values": [v] }))
        })
        .collect()
}

/// Merge the convenience equals-filters with the caller's raw `--filters` JSON
/// array (the full `analyze query` escape hatch) into a single Cube `filters`
/// clause. Returns `None` when there are no filters at all. The raw array must
/// be a JSON array of filter objects; anything else is a user error.
pub(super) fn merge_filters(
    mut convenience: Vec<Value>,
    raw: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(raw) = raw {
        let parsed: Value =
            serde_json::from_str(raw).map_err(|e| format!("--filters is not valid JSON: {}", e))?;
        let extra = parsed
            .as_array()
            .ok_or("--filters must be a JSON array of filter objects")?;
        convenience.extend(extra.iter().cloned());
    }
    if convenience.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Value::Array(convenience).to_string()))
    }
}

/// Cube `equals` filter matching any of `session_ids` (IN semantics).
pub(super) fn id_filter(cube: &str, session_ids: &[String]) -> String {
    json!([{
        "member": format!("{}.session_id", cube),
        "operator": "equals",
        "values": session_ids,
    }])
    .to_string()
}
