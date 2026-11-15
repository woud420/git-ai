//! Which chars of a file's text a set of byte ranges touches.
//!
//! A char counts as covered when a range touches any of its bytes, so every
//! boundary produced here is a char boundary of the content.

use super::diff_engine::merge_ranges;
use crate::model::attribution::{ceil_char_boundary, floor_char_boundary};

/// Sorted, disjoint byte ranges of the chars that `ranges` touch.
pub(crate) fn covered_char_ranges(
    content: &str,
    ranges: impl IntoIterator<Item = (usize, usize)>,
) -> Vec<(usize, usize)> {
    let widened = ranges.into_iter().map(|(start, end)| {
        (
            floor_char_boundary(content, start),
            ceil_char_boundary(content, end),
        )
    });
    merge_ranges(widened.collect())
}

/// The parts of `start..end` that the sorted, disjoint `covered` ranges leave out.
pub(crate) fn uncovered_ranges(
    covered: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Vec<(usize, usize)> {
    let mut gaps = Vec::new();
    let mut gap_start = start;
    let first = covered.partition_point(|&(_, covered_end)| covered_end <= start);
    for &(covered_start, covered_end) in &covered[first..] {
        if covered_start >= end {
            break;
        }
        if gap_start < covered_start {
            gaps.push((gap_start, covered_start));
        }
        gap_start = covered_end;
    }
    if gap_start < end {
        gaps.push((gap_start, end));
    }
    gaps
}
