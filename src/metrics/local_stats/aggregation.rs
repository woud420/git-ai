//! Per-event aggregation helpers: committed, checkpoint, session.

use crate::metrics::attrs::attr_pos;
use crate::metrics::events::{checkpoint_pos, committed_pos};
use crate::metrics::pos_encoded::{
    sparse_get_string, sparse_get_u32, sparse_get_vec_string, sparse_get_vec_u32,
};
use crate::metrics::types::MetricEvent;
use chrono::NaiveDate;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Per-commit contribution returned by `aggregate_committed` for bucketing.
pub(super) struct CommitContribution {
    pub(super) ai_lines: u32,
    pub(super) human_lines: u32,
    pub(super) diff_added: u32,
}

pub(super) fn aggregate_committed(
    event: &MetricEvent,
    total_commits: &mut u32,
    total_ai_lines: &mut u32,
    total_human_lines: &mut u32,
    total_diff_added: &mut u32,
    commit_tool_counts: &mut HashMap<String, u32>,
    committed_ai_by_plain_tool: &mut HashMap<String, u32>,
) -> CommitContribution {
    let human = sparse_get_u32(&event.values, committed_pos::HUMAN_ADDITIONS)
        .flatten()
        .unwrap_or(0);
    let diff_added = sparse_get_u32(&event.values, committed_pos::GIT_DIFF_ADDED_LINES)
        .flatten()
        .unwrap_or(0);
    let ai_vecs = sparse_get_vec_u32(&event.values, committed_pos::AI_ADDITIONS)
        .flatten()
        .unwrap_or_default();
    let total_ai = ai_vecs.first().copied().unwrap_or(0);

    // Always accumulate human lines and total diff additions regardless of
    // whether the commit has AI lines (coverage spans all committed code).
    *total_human_lines += human;
    *total_diff_added += diff_added;

    let contribution = CommitContribution {
        ai_lines: total_ai,
        human_lines: human,
        diff_added,
    };

    // Only count the commit toward the AI-commits total when AI was involved.
    // Human-only commits still contribute to human_lines and diff_added above.
    if total_ai == 0 {
        return contribution;
    }

    *total_commits += 1;
    *total_ai_lines += total_ai;

    // Per-tool breakdown: index 0 = "all" aggregate, 1+ = per tool::model.
    // Parse pairs once and use them for both the display label map and the
    // plain-tool map used for acceptance rate — no second parse needed.
    let pairs = sparse_get_vec_string(&event.values, committed_pos::TOOL_MODEL_PAIRS)
        .flatten()
        .unwrap_or_default();
    for (i, pair) in pairs.iter().enumerate().skip(1) {
        let ai_for_tool = ai_vecs.get(i).copied().unwrap_or(0);
        if ai_for_tool > 0 {
            *commit_tool_counts
                .entry(format_tool_model(pair))
                .or_insert(0) += ai_for_tool;
            let plain_tool = pair.split_once("::").map(|(t, _)| t).unwrap_or(pair);
            *committed_ai_by_plain_tool
                .entry(plain_tool.to_string())
                .or_insert(0) += ai_for_tool;
        }
    }

    contribution
}

/// Format a "tool::model" pair into a readable "tool · model" label,
/// trimming a redundant tool prefix from the model (e.g. "claude::claude-sonnet-4-6"
/// becomes "claude · sonnet-4-6").
fn format_tool_model(pair: &str) -> String {
    match pair.split_once("::") {
        Some((tool, model)) if !model.is_empty() => {
            let prefix = format!("{tool}-");
            let model = model.strip_prefix(&prefix).unwrap_or(model);
            format!("{tool} · {model}")
        }
        _ => pair.to_string(),
    }
}

pub(super) fn aggregate_checkpoint(
    event: &MetricEvent,
    total_checkpoints: &mut u32,
    ai_lines_added: &mut u32,
    human_lines_added: &mut u32,
    files_edited: &mut HashSet<String>,
    checkpoint_ai_by_tool: &mut HashMap<String, u32>,
) {
    *total_checkpoints += 1;

    let kind = sparse_get_string(&event.values, checkpoint_pos::KIND)
        .flatten()
        .unwrap_or_default();
    let file_path = sparse_get_string(&event.values, checkpoint_pos::FILE_PATH)
        .flatten()
        .unwrap_or_default();
    let lines_added = sparse_get_u32(&event.values, checkpoint_pos::LINES_ADDED)
        .flatten()
        .unwrap_or(0);

    if !file_path.is_empty() {
        files_edited.insert(file_path);
    }

    match kind.as_str() {
        "ai_agent" | "ai_tab" => {
            *ai_lines_added += lines_added;
            if lines_added > 0 {
                let tool = sparse_get_string(&event.attrs, attr_pos::TOOL)
                    .flatten()
                    .unwrap_or_else(|| "unknown".to_string());
                *checkpoint_ai_by_tool.entry(tool).or_insert(0) += lines_added;
            }
        }
        "known_human" => *human_lines_added += lines_added,
        _ => {}
    }
}

pub(super) fn aggregate_session(
    event: &MetricEvent,
    session_ids: &mut HashSet<String>,
    session_tool_counts: &mut HashMap<String, u32>,
) {
    let session_id = sparse_get_string(&event.attrs, attr_pos::SESSION_ID).flatten();
    let tool = sparse_get_string(&event.attrs, attr_pos::TOOL)
        .flatten()
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(sid) = session_id
        && session_ids.insert(sid)
    {
        *session_tool_counts.entry(tool).or_insert(0) += 1;
    }
}

/// Compute (longest, current) consecutive-active-day streaks from a sorted set of
/// active days. A run extends only when consecutive days differ by exactly one
/// calendar day (DST-proof `NaiveDate` arithmetic). The current streak is the
/// trailing run, counted only when it reaches today or yesterday (one-day grace).
pub(super) fn compute_streaks(days: &BTreeMap<NaiveDate, u32>, today: NaiveDate) -> (u32, u32) {
    let mut longest = 0u32;
    let mut run = 0u32;
    let mut prev: Option<NaiveDate> = None;
    for &d in days.keys() {
        run = match prev {
            Some(p) if (d - p).num_days() == 1 => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        prev = Some(d);
    }
    let last = match prev {
        Some(p) => p,
        None => return (0, 0),
    };
    let yesterday = today.pred_opt().unwrap_or(today);
    let current = if last == today || last == yesterday {
        run
    } else {
        0
    };
    (longest, current)
}
