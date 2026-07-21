//! Diff computation — phases 1 and 2.
//!
//! Phase 1: `compute_diffs` — converts old/new content into a `ByteDiff`
//! sequence via a two-level pass (line-level, then token-level within hunks).
//!
//! Phase 2: `build_diff_catalog` — walks the `ByteDiff` list to extract
//! ordered `Deletion` and `Insertion` byte-range catalogs for the move-
//! detection and transform phases.

use crate::error::GitAiError;
use std::time::Instant;

use super::diff_engine::{
    DiffComputation, append_range_diffs, build_token_aligned_diffs, data_is_whitespace,
    hunk_line_bounds, line_range_to_byte_range, merge_ranges, should_use_line_aligned_hunk_diff,
};
use super::tokenizer::{LineMetadata, collect_line_metadata};
use super::tracker::AttributionTracker;
use super::types::{Deletion, Insertion};
use crate::model::imara_diff_utils::{ByteDiff, ByteDiffOp, DiffOp, capture_diff_slices};

impl AttributionTracker {
    pub(super) fn compute_diffs(
        &self,
        old_content: &str,
        new_content: &str,
        is_ai_checkpoint: bool,
    ) -> Result<DiffComputation, GitAiError> {
        let compute_start = Instant::now();
        let line_metadata_start = Instant::now();
        let old_lines = collect_line_metadata(old_content);
        let new_lines = collect_line_metadata(new_content);
        tracing::debug!(
            "[BENCHMARK] collect_line_metadata (old/new) took {:?}",
            line_metadata_start.elapsed()
        );

        let capture_start = Instant::now();
        let old_line_slices: Vec<&str> = old_lines
            .iter()
            .map(|line| &old_content[line.start..line.end])
            .collect();
        let new_line_slices: Vec<&str> = new_lines
            .iter()
            .map(|line| &new_content[line.start..line.end])
            .collect();

        let line_ops = capture_diff_slices(&old_line_slices, &new_line_slices);
        let line_ops_len = line_ops.len();
        tracing::debug!(
            "[BENCHMARK] capture_diff_slices produced {} ops in {:?}",
            line_ops_len,
            capture_start.elapsed()
        );

        let mut computation = DiffComputation::default();
        let mut pending_changed: Vec<DiffOp> = Vec::new();
        let process_start = Instant::now();

        for op in line_ops.into_iter() {
            if matches!(op, DiffOp::Equal { .. }) {
                if !pending_changed.is_empty() {
                    self.process_changed_hunk(
                        &pending_changed,
                        &old_lines,
                        &new_lines,
                        old_content,
                        new_content,
                        &mut computation,
                        is_ai_checkpoint,
                    )?;
                    pending_changed.clear();
                }

                self.push_equal_lines(op, &old_lines, old_content, &mut computation.diffs)?;
            } else {
                pending_changed.push(op);
            }
        }

        if !pending_changed.is_empty() {
            self.process_changed_hunk(
                &pending_changed,
                &old_lines,
                &new_lines,
                old_content,
                new_content,
                &mut computation,
                is_ai_checkpoint,
            )?;
        }

        computation.substantive_new_ranges = merge_ranges(computation.substantive_new_ranges);
        tracing::debug!(
            "[BENCHMARK] compute_diffs processed {} ops in {:?} (total {:?})",
            line_ops_len,
            process_start.elapsed(),
            compute_start.elapsed()
        );

        Ok(computation)
    }

    fn push_equal_lines(
        &self,
        op: DiffOp,
        old_lines: &[LineMetadata],
        old_content: &str,
        diffs: &mut Vec<ByteDiff>,
    ) -> Result<(), GitAiError> {
        if let DiffOp::Equal { old_index, len, .. } = op {
            if len == 0 {
                return Ok(());
            }

            let (start, end) =
                line_range_to_byte_range(old_lines, old_index, old_index + len, old_content.len());

            if start < end {
                diffs.push(ByteDiff::new(
                    ByteDiffOp::Equal,
                    &old_content.as_bytes()[start..end],
                ));
            }

            return Ok(());
        }

        Err(GitAiError::Generic(
            "Expected equal operation in push_equal_lines".to_string(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn process_changed_hunk(
        &self,
        ops: &[DiffOp],
        old_lines: &[LineMetadata],
        new_lines: &[LineMetadata],
        old_content: &str,
        new_content: &str,
        computation: &mut DiffComputation,
        is_ai_checkpoint: bool,
    ) -> Result<(), GitAiError> {
        if ops.is_empty() {
            return Ok(());
        }

        let (old_start_line, old_end_line) = hunk_line_bounds(ops, true);
        let (new_start_line, new_end_line) = hunk_line_bounds(ops, false);

        let (old_start, old_end) =
            line_range_to_byte_range(old_lines, old_start_line, old_end_line, old_content.len());
        let (new_start, new_end) =
            line_range_to_byte_range(new_lines, new_start_line, new_end_line, new_content.len());

        // For AI checkpoints, always use force_split so that all new bytes are
        // attributed to the AI author – including tokens that happen to match
        // pre-existing content (e.g. a `)` or a variable name that appeared in
        // the old file).  force_split emits Delete+Insert ops instead of Equal,
        // so transform_attributions never inherits old (human) attribution for
        // content rewritten by AI.
        //
        // For pure insertions (0→N) and pure deletions (N→0) force_split gives
        // the same result as token-aligned diffing (all Insert or all Delete),
        // so there is no regression for those cases.
        if is_ai_checkpoint {
            append_range_diffs(
                &mut computation.diffs,
                old_content,
                new_content,
                (old_start, old_end),
                (new_start, new_end),
                true,
            );
            return Ok(());
        }

        if should_use_line_aligned_hunk_diff(
            ops,
            old_end_line.saturating_sub(old_start_line),
            new_end_line.saturating_sub(new_start_line),
            old_end.saturating_sub(old_start),
            new_end.saturating_sub(new_start),
        ) {
            append_range_diffs(
                &mut computation.diffs,
                old_content,
                new_content,
                (old_start, old_end),
                (new_start, new_end),
                true,
            );
            if new_start < new_end
                && !data_is_whitespace(&new_content.as_bytes()[new_start..new_end])
            {
                computation
                    .substantive_new_ranges
                    .push((new_start, new_end));
            }
            return Ok(());
        }

        let (mut hunk_diffs, substantive_ranges) = build_token_aligned_diffs(
            old_content,
            new_content,
            (old_start, old_end),
            (new_start, new_end),
        );

        computation.diffs.append(&mut hunk_diffs);
        computation
            .substantive_new_ranges
            .extend(substantive_ranges);

        Ok(())
    }

    pub(super) fn build_diff_catalog(&self, diffs: &[ByteDiff]) -> (Vec<Deletion>, Vec<Insertion>) {
        let mut deletions = Vec::new();
        let mut insertions = Vec::new();

        let mut old_pos = 0;
        let mut new_pos = 0;

        for diff in diffs {
            let op = diff.op();
            match op {
                ByteDiffOp::Equal => {
                    let len = diff.data().len();
                    old_pos += len;
                    new_pos += len;
                }
                ByteDiffOp::Delete => {
                    let bytes = diff.data();
                    let len = bytes.len();
                    deletions.push(Deletion {
                        start: old_pos,
                        end: old_pos + len,
                    });
                    old_pos += len;
                }
                ByteDiffOp::Insert => {
                    let bytes = diff.data();
                    let len = bytes.len();
                    insertions.push(Insertion {
                        start: new_pos,
                        end: new_pos + len,
                    });
                    new_pos += len;
                }
            }
        }

        (deletions, insertions)
    }
}
