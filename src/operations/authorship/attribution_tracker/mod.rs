//! Attribution tracking through file changes
//!
//! This library maintains attribution ranges as files are edited, preserving
//! authorship information even through moves, edits, and whitespace changes.

mod diff_engine;
mod diff_pipeline;
mod line_attribution;
mod move_integration;
mod tokenizer;
mod tracker;
mod transform;

#[cfg(test)]
mod tests_tracker;

// `Attribution` / `LineAttribution` are pure value types owned by `model`.
// Re-exported here so the tracker keeps a single public attribution surface.
pub use crate::model::attribution::{Attribution, LineAttribution};
pub use line_attribution::{
    attributions_to_line_attributions, attributions_to_line_attributions_for_checkpoint,
    line_attributions_to_attributions,
};
pub use tracker::{AttributionConfig, AttributionTracker};

mod types;

pub const INITIAL_ATTRIBUTION_TS: u128 = 42;
