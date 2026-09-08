//! Comprehensive tests for src/authorship/attribution_tracker.rs
//!
//! This test module covers critical functionality in attribution_tracker.rs (2,573 LOC)
//! which is the core diff-based attribution tracking module that underpins AI authorship tracking.
//!
//! Test coverage areas:
//! 1. Basic line attribution (AI vs human edits)
//! 2. Move detection across files and within files
//! 3. Whitespace-only changes
//! 4. Mixed AI/human edits on same lines
//! 5. Large file performance
//! 6. Unicode and special character handling
//! 7. Diff algorithm edge cases
//! 8. Character-level attribution tracking
//! 9. Attribution preservation through renames
//! 10. Multi-file attribution scenarios

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::attribution_tracker::{
    Attribution, AttributionConfig, AttributionTracker, INITIAL_ATTRIBUTION_TS, LineAttribution,
};

mod edit_content;
mod gap_filling;
mod large_edits;
mod moves_and_context;
mod ranges;
mod repository_operations;
mod whitespace_and_encoding;
