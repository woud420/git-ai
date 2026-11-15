//! `attribute_unattributed_ranges` against the per-char scan it replaced.

use super::tracker::AttributionTracker;
use crate::model::attribution::Attribution;

/// The original O(chars x attributions) implementation, kept as the oracle.
fn scan_every_char(
    content: &str,
    prev: &[Attribution],
    author: &str,
    ts: u128,
) -> Vec<Attribution> {
    let mut attributions = prev.to_vec();
    let mut range_start: Option<usize> = None;

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

/// Every byte range over the content, including zero-length, inverted,
/// mid-char and out-of-bounds ones.
fn all_byte_ranges(content: &str) -> Vec<(usize, usize)> {
    let limit = content.len() + 1;
    (0..=limit)
        .flat_map(|start| (0..=limit).map(move |end| (start, end)))
        .collect()
}

#[test]
fn matches_the_per_char_scan_for_every_pair_of_ranges() {
    let tracker = AttributionTracker::new();
    for content in ["", "abc", "é😀\nb", "a名 b\n"] {
        assert_eq!(
            tracker.attribute_unattributed_ranges(content, &[], "filler", 9),
            scan_every_char(content, &[], "filler", 9),
            "content {content:?}, no ranges"
        );

        let ranges = all_byte_ranges(content);
        for (i, &(a_start, a_end)) in ranges.iter().enumerate() {
            for &(b_start, b_end) in &ranges[i..] {
                let prev = [
                    Attribution::new(a_start, a_end, "a".into(), 1),
                    Attribution::new(b_start, b_end, "b".into(), 2),
                ];
                assert_eq!(
                    tracker.attribute_unattributed_ranges(content, &prev, "filler", 9),
                    scan_every_char(content, &prev, "filler", 9),
                    "content {content:?}, ranges {prev:?}"
                );
            }
        }
    }
}
