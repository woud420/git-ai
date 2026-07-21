//! Internal diff-catalog types for the attribution tracker.
//!
//! `Deletion`, `Insertion`, and `MoveMapping` are internal diff-catalog types
//! shared across the diff engine and tracker. The public attribution value
//! types (`Attribution`, `LineAttribution`) live in [`crate::model::attribution`].

/// Represents a deletion operation from the diff
#[derive(Debug, Clone)]
pub(crate) struct Deletion {
    /// Start position in old content
    pub(crate) start: usize,
    /// End position in old content
    pub(crate) end: usize,
}

/// Represents an insertion operation from the diff
#[derive(Debug, Clone)]
pub(crate) struct Insertion {
    /// Start position in new content
    pub(crate) start: usize,
    /// End position in new content
    pub(crate) end: usize,
}

/// Information about a detected move operation
#[derive(Debug, Clone)]
pub(crate) struct MoveMapping {
    /// The deletion that was moved
    pub(crate) deletion_idx: usize,
    /// The insertion where it was moved to
    pub(crate) insertion_idx: usize,
    /// Range within the deletion text that maps to the insertion (start, end) exclusive bounds
    pub(crate) source_range: (usize, usize),
    /// Range within the insertion text where the deletion text lands (start, end) exclusive bounds
    pub(crate) target_range: (usize, usize),
}
