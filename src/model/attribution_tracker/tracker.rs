//! Attribution tracker state machine — struct and orchestration.
//!
//! `AttributionTracker` owns the four-phase pipeline that lives in
//! `diff_pipeline.rs`, `move_integration.rs`, and `transform.rs`.
//! This file holds the public constructors, the top-level
//! `update_attributions*` orchestrators, and `merge_attributions`.

use crate::error::GitAiError;
use std::cmp::Ordering;

use crate::model::attribution::Attribution;

/// Configuration for the attribution tracker
pub struct AttributionConfig {
    pub(super) move_lines_threshold: usize,
}

impl Default for AttributionConfig {
    fn default() -> Self {
        AttributionConfig {
            move_lines_threshold: 3,
        }
    }
}

/// Main attribution tracker
pub struct AttributionTracker {
    pub(super) config: AttributionConfig,
}

impl AttributionTracker {
    /// Create a new attribution tracker with default configuration
    pub fn new() -> Self {
        AttributionTracker {
            config: AttributionConfig::default(),
        }
    }

    /// Create a new attribution tracker with custom configuration
    #[allow(dead_code)]
    pub fn with_config(config: AttributionConfig) -> Self {
        AttributionTracker { config }
    }

    /// Attribute all unattributed ranges to the given author
    pub fn attribute_unattributed_ranges(
        &self,
        content: &str,
        prev_attributions: &[Attribution],
        author: &str,
        ts: u128,
    ) -> Vec<Attribution> {
        let mut attributions = prev_attributions.to_vec();
        let mut range_start: Option<usize> = None;

        // Find all unattributed character ranges on UTF-8 boundaries.
        for (idx, ch) in content.char_indices() {
            let end = idx + ch.len_utf8();
            let covered = attributions.iter().any(|a| a.overlaps(idx, end));

            if covered {
                if let Some(start) = range_start.take()
                    && start < idx
                {
                    attributions.push(Attribution::new(start, idx, author.to_string(), ts));
                }
            } else if range_start.is_none() {
                range_start = Some(idx);
            }
        }

        if let Some(start) = range_start.take()
            && start < content.len()
        {
            attributions.push(Attribution::new(
                start,
                content.len(),
                author.to_string(),
                ts,
            ));
        }

        attributions
    }

    /// Update attributions from old content to new content
    ///
    /// # Arguments
    /// * `old_content` - The previous version of the file
    /// * `new_content` - The new version of the file
    /// * `old_attributions` - Attributions from the previous version
    /// * `current_author` - Author ID to use for new changes
    ///
    /// # Returns
    /// A vector of updated attributions for the new content
    pub fn update_attributions(
        &self,
        old_content: &str,
        new_content: &str,
        old_attributions: &[Attribution],
        current_author: &str,
        ts: u128,
    ) -> Result<Vec<Attribution>, GitAiError> {
        self.update_attributions_for_checkpoint(
            old_content,
            new_content,
            old_attributions,
            current_author,
            ts,
            false,
        )
    }

    pub fn update_attributions_for_checkpoint(
        &self,
        old_content: &str,
        new_content: &str,
        old_attributions: &[Attribution],
        current_author: &str,
        ts: u128,
        is_ai_checkpoint: bool,
    ) -> Result<Vec<Attribution>, GitAiError> {
        // Cursor-based scans in transform_attributions assume sorted ranges.
        // Normalize once at the boundary so callers can pass ranges in any order.
        let sorted_old_storage = (!is_attribution_list_sorted(old_attributions))
            .then(|| sort_attributions_for_transform(old_attributions));
        let old_attributions = sorted_old_storage.as_deref().unwrap_or(old_attributions);

        // Phase 1: Compute diff
        let diff_result = self.compute_diffs(old_content, new_content, is_ai_checkpoint)?;

        // Phase 2: Build deletion and insertion catalogs
        let (deletions, insertions) = self.build_diff_catalog(&diff_result.diffs);

        // Phase 3: Detect move operations
        let move_mappings = if is_ai_checkpoint {
            // AI formatting/refactor checkpoints should attribute rewritten regions to AI
            // instead of preserving original ownership through move detection.
            Vec::new()
        } else if self.should_skip_move_detection(old_content, new_content, &deletions, &insertions)
        {
            Vec::new()
        } else {
            self.detect_moves(old_content, new_content, &deletions, &insertions)
        };

        // Phase 4: Transform attributions through the diff
        let new_attributions = self.transform_attributions(
            &diff_result.diffs,
            old_attributions,
            current_author,
            &insertions,
            &move_mappings,
            ts,
            &diff_result.substantive_new_ranges,
            is_ai_checkpoint,
        );

        // Phase 5: Merge and clean up
        Ok(self.merge_attributions(new_attributions))
    }

    /// Merge and clean up attributions
    pub(super) fn merge_attributions(
        &self,
        mut attributions: Vec<Attribution>,
    ) -> Vec<Attribution> {
        if attributions.is_empty() {
            return attributions;
        }

        // Sort by position first, then stable author/timestamp metadata.
        attributions.sort_by(|a, b| {
            a.start
                .cmp(&b.start)
                .then_with(|| a.end.cmp(&b.end))
                .then_with(|| a.author_id.cmp(&b.author_id))
                .then_with(|| a.ts.cmp(&b.ts))
        });

        // Remove exact duplicates
        attributions.dedup();

        // Coalesce adjacent/overlapping ranges with identical attribution metadata.
        // This keeps attribution vectors compact during long rewrite chains.
        let mut merged: Vec<Attribution> = Vec::with_capacity(attributions.len());
        for attr in attributions {
            if let Some(last) = merged.last_mut()
                && last.author_id == attr.author_id
                && last.ts == attr.ts
                && last.start < last.end
                && attr.start < attr.end
                && attr.start <= last.end
            {
                last.end = last.end.max(attr.end);
                continue;
            }
            merged.push(attr);
        }

        merged
    }
}

impl Default for AttributionTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn compare_attribution_order(a: &Attribution, b: &Attribution) -> Ordering {
    a.start
        .cmp(&b.start)
        .then_with(|| a.end.cmp(&b.end))
        .then_with(|| a.author_id.cmp(&b.author_id))
        .then_with(|| a.ts.cmp(&b.ts))
}

pub(super) fn is_attribution_list_sorted(attributions: &[Attribution]) -> bool {
    attributions
        .windows(2)
        .all(|pair| compare_attribution_order(&pair[0], &pair[1]) != Ordering::Greater)
}

fn sort_attributions_for_transform(attributions: &[Attribution]) -> Vec<Attribution> {
    let mut sorted = attributions.to_vec();
    sorted.sort_by(compare_attribution_order);
    sorted
}
