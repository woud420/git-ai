use super::*;

#[test]
fn oversized_post_match_run_is_discarded_without_losing_the_exact_match() {
    let output = format!(
        "{}{}",
        matched_pair(1),
        leading_drops(MAX_PENDING_DROPPED_COMMITS + 1)
    );
    assert_eq!(
        parse_range_diff_output(&output),
        vec![("a".repeat(40), "b".repeat(40))]
    );
}

#[test]
fn post_match_run_at_the_bound_keeps_its_previous_destination() {
    let output = format!(
        "{}{}",
        matched_pair(1),
        leading_drops(MAX_PENDING_DROPPED_COMMITS)
    );
    let mappings = parse_range_diff_output(&output);
    assert_eq!(mappings.len(), MAX_PENDING_DROPPED_COMMITS + 1);
    assert!(
        mappings
            .iter()
            .all(|(_, destination)| destination == &"b".repeat(40))
    );
}

#[test]
fn exact_match_after_overflow_starts_a_fresh_drop_budget() {
    let output = format!(
        "{}{}2: {} = 2: {} later\n{}",
        matched_pair(1),
        leading_drops(MAX_PENDING_DROPPED_COMMITS + 1),
        "c".repeat(40),
        "d".repeat(40),
        leading_drops(1)
    );
    assert_eq!(
        parse_range_diff_output(&output),
        vec![
            ("a".repeat(40), "b".repeat(40)),
            ("c".repeat(40), "d".repeat(40)),
            (format!("{:040x}", 1), "d".repeat(40)),
        ]
    );
}
