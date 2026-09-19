use crate::error::GitAiError;
use crate::model::attribution_tracker::{LineAttribution, attributions_to_line_attributions};
use crate::model::imara_diff_utils::{DiffOp, capture_diff_slices, normalize_line_endings};
use crate::model::working_log::{InitialAttributions, WorkingLogEntry};
use crate::operations::git::repo_storage::PersistedWorkingLog;
use std::collections::{BTreeSet, HashSet};

pub(crate) fn historical_attributions(
    working_log: &PersistedWorkingLog,
    initial: &InitialAttributions,
    path: &str,
    committed: &str,
    changed: &[u32],
    entries: &[&WorkingLogEntry],
) -> Result<Vec<LineAttribution>, GitAiError> {
    let mut unresolved: BTreeSet<_> = changed.iter().copied().collect();
    let mut restored = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries.iter().rev() {
        if unresolved.is_empty() {
            break;
        }
        if entry.blob_sha.is_empty() || !seen.insert(&entry.blob_sha) {
            continue;
        }
        let content = working_log.get_file_version(&entry.blob_sha)?;
        let attrs = if entry.line_attributions.is_empty() {
            attributions_to_line_attributions(&entry.attributions, &content)
        } else {
            entry.line_attributions.clone()
        };
        restore_matching_lines(&content, committed, &attrs, &mut unresolved, &mut restored);
    }
    if !unresolved.is_empty()
        && let Some(blob) = initial.file_blobs.get(path)
        && let Some(attrs) = initial.files.get(path)
    {
        let content = working_log.get_file_version(blob)?;
        restore_matching_lines(&content, committed, attrs, &mut unresolved, &mut restored);
    }
    Ok(restored)
}

pub(crate) fn restore_matching_lines(
    checkpoint: &str,
    committed: &str,
    attrs: &[LineAttribution],
    unresolved: &mut BTreeSet<u32>,
    restored: &mut Vec<LineAttribution>,
) {
    let checkpoint = normalize_line_endings(checkpoint);
    let committed = normalize_line_endings(committed);
    // An EOF newline change does not replace the text carrying attribution.
    let old_lines: Vec<_> = checkpoint.lines().collect();
    let new_lines: Vec<_> = committed.lines().collect();
    for op in capture_diff_slices(&old_lines, &new_lines) {
        let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        else {
            continue;
        };
        for attr in attrs {
            let start = attr.start_line.max(old_index as u32 + 1);
            let end = attr.end_line.min((old_index + len) as u32);
            if start > end {
                continue;
            }
            let target_start = start - old_index as u32 + new_index as u32;
            let target_end = end - old_index as u32 + new_index as u32;
            let lines: Vec<_> = unresolved
                .range(target_start..=target_end)
                .copied()
                .collect();
            for line in lines {
                unresolved.remove(&line);
                restored.push(LineAttribution {
                    start_line: line,
                    end_line: line,
                    author_id: attr.author_id.clone(),
                    overrode: attr.overrode.clone(),
                });
            }
        }
    }
}
