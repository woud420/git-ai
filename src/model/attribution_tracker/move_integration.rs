//! Move-detection integration — phase 3.
//!
//! `should_skip_move_detection` guards the expensive move-detection pass
//! with file-size and operation-count heuristics.  `detect_moves` drives
//! the `move_detection` module and converts its line-level output into the
//! byte-range `MoveMapping` entries consumed by `transform_attributions`.

use crate::model::move_detection::{DeletedLine, InsertedLine, detect_moves};
use std::collections::HashMap;

use super::diff_engine::{MOVE_DETECTION_MAX_OPS, MOVE_DETECTION_MIN_FILE_BYTES};
use super::tokenizer::{LineMetadata, collect_line_metadata};
use super::tracker::AttributionTracker;
use super::types::{Deletion, Insertion, MoveMapping};

impl AttributionTracker {
    pub(super) fn should_skip_move_detection(
        &self,
        old_content: &str,
        new_content: &str,
        deletions: &[Deletion],
        insertions: &[Insertion],
    ) -> bool {
        if self.config.move_lines_threshold == 0 {
            return true;
        }
        if deletions.is_empty() || insertions.is_empty() {
            return true;
        }

        let max_file_bytes = old_content.len().max(new_content.len());
        let operation_count = deletions.len().saturating_add(insertions.len());
        if max_file_bytes < MOVE_DETECTION_MIN_FILE_BYTES
            && operation_count <= MOVE_DETECTION_MAX_OPS
        {
            return false;
        }

        let deleted_bytes: usize = deletions
            .iter()
            .map(|deletion| deletion.end.saturating_sub(deletion.start))
            .sum();
        let inserted_bytes: usize = insertions
            .iter()
            .map(|insertion| insertion.end.saturating_sub(insertion.start))
            .sum();
        let changed_bytes = deleted_bytes.saturating_add(inserted_bytes);

        operation_count > MOVE_DETECTION_MAX_OPS
            || (max_file_bytes >= MOVE_DETECTION_MIN_FILE_BYTES
                && changed_bytes >= max_file_bytes.saturating_mul(3) / 2)
    }

    /// Detect move operations between deletions and insertions
    pub(super) fn detect_moves(
        &self,
        old_content: &str,
        new_content: &str,
        deletions: &[Deletion],
        insertions: &[Insertion],
    ) -> Vec<MoveMapping> {
        let threshold = self.config.move_lines_threshold;
        if threshold == 0 || deletions.is_empty() || insertions.is_empty() {
            return Vec::new();
        }

        let old_lines = collect_line_metadata(old_content);
        let new_lines = collect_line_metadata(new_content);

        let old_line_map: HashMap<usize, LineMetadata> = old_lines
            .iter()
            .cloned()
            .map(|line| (line.number, line))
            .collect();
        let new_line_map: HashMap<usize, LineMetadata> = new_lines
            .iter()
            .cloned()
            .map(|line| (line.number, line))
            .collect();

        let mut inserted_lines: Vec<InsertedLine> = Vec::new();
        for (insertion_idx, insertion) in insertions.iter().enumerate() {
            for line in new_lines.iter() {
                if line.start < insertion.end && line.end > insertion.start {
                    inserted_lines.push(InsertedLine::new(
                        line.text.clone(),
                        line.number,
                        insertion_idx,
                    ));
                }
            }
        }

        let mut deleted_lines: Vec<DeletedLine> = Vec::new();
        for (deletion_idx, deletion) in deletions.iter().enumerate() {
            for line in old_lines.iter() {
                if line.start < deletion.end && line.end > deletion.start {
                    deleted_lines.push(DeletedLine::new(
                        line.text.clone(),
                        line.number,
                        deletion_idx,
                    ));
                }
            }
        }

        if inserted_lines.is_empty() || deleted_lines.is_empty() {
            return Vec::new();
        }

        let mut inserted_lines_slice = inserted_lines;
        let mut deleted_lines_slice = deleted_lines;
        let line_mappings = detect_moves(
            inserted_lines_slice.as_mut_slice(),
            deleted_lines_slice.as_mut_slice(),
            threshold,
        );

        let mut move_mappings = Vec::new();

        'mapping: for line_mapping in line_mappings {
            if line_mapping.deleted.is_empty() || line_mapping.inserted.is_empty() {
                continue;
            }
            if line_mapping.deleted.len() != line_mapping.inserted.len() {
                continue;
            }

            let deletion_idx = line_mapping.deleted[0].deletion_idx;
            if !line_mapping
                .deleted
                .iter()
                .all(|line| line.deletion_idx == deletion_idx)
            {
                continue;
            }

            let insertion_idx = line_mapping.inserted[0].insertion_idx;
            if !line_mapping
                .inserted
                .iter()
                .all(|line| line.insertion_idx == insertion_idx)
            {
                continue;
            }

            let deletion = match deletions.get(deletion_idx) {
                Some(value) => value,
                None => continue,
            };
            let insertion = match insertions.get(insertion_idx) {
                Some(value) => value,
                None => continue,
            };

            let mut source_start_opt: Option<usize> = None;
            let mut source_end_opt: Option<usize> = None;
            for deleted_line in &line_mapping.deleted {
                let meta = match old_line_map.get(&deleted_line.line_number) {
                    Some(meta) => meta,
                    None => continue 'mapping,
                };
                let start = meta.start.max(deletion.start);
                let end = meta.end.min(deletion.end);
                if start >= end {
                    continue 'mapping;
                }
                let rel_start = start - deletion.start;
                let rel_end = end - deletion.start;
                if source_start_opt.is_none() {
                    source_start_opt = Some(rel_start);
                }
                source_end_opt = Some(rel_end);
            }

            let mut target_start_opt: Option<usize> = None;
            let mut target_end_opt: Option<usize> = None;
            for inserted_line in &line_mapping.inserted {
                let meta = match new_line_map.get(&inserted_line.line_number) {
                    Some(meta) => meta,
                    None => continue 'mapping,
                };
                let start = meta.start.max(insertion.start);
                let end = meta.end.min(insertion.end);
                if start >= end {
                    continue 'mapping;
                }
                let rel_start = start - insertion.start;
                let rel_end = end - insertion.start;
                if target_start_opt.is_none() {
                    target_start_opt = Some(rel_start);
                }
                target_end_opt = Some(rel_end);
            }

            let (source_start, source_end) = match (source_start_opt, source_end_opt) {
                (Some(start), Some(end)) if start < end => (start, end),
                _ => continue,
            };
            let (target_start, target_end) = match (target_start_opt, target_end_opt) {
                (Some(start), Some(end)) if start < end => (start, end),
                _ => continue,
            };

            move_mappings.push(MoveMapping {
                deletion_idx,
                insertion_idx,
                source_range: (source_start, source_end),
                target_range: (target_start, target_end),
            });
        }

        move_mappings
    }
}
