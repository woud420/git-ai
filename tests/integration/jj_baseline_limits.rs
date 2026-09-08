use super::*;
use git_ai::model::jj_observation::MAX_JJ_OBSERVATION_OPERATION_BYTES;

fn opaque_anchors(count: usize, bytes_each: usize) -> Vec<JjOperationEvidence> {
    (1..=count)
        .map(|index| {
            let mut anchor = first();
            anchor.operation_id = format!("{index:0128x}");
            anchor.operation_bytes = vec![0; bytes_each];
            anchor.view_bytes.clear();
            anchor
        })
        .collect()
}

#[test]
fn jj_baseline_head_cap_is_32_and_precedes_native_decoding() {
    assert_eq!(MAX_JJ_BASELINE_HEADS, 32);
    let anchors = opaque_anchors(MAX_JJ_BASELINE_HEADS + 1, 1);
    rejected(&heads(&anchors), &anchors, "head");
    let anchors = &anchors[..MAX_JJ_BASELINE_HEADS];
    let error = rejected(&heads(anchors), anchors, "operation");
    assert!(!error.to_string().contains("limit"));
}

#[test]
fn jj_baseline_anchor_count_is_bounded_before_native_decoding_or_set_allocation() {
    let anchors = opaque_anchors(MAX_JJ_BASELINE_HEADS + 1, 1);
    let error = rejected(&[anchors[0].operation_id.clone()], &anchors, "anchor");
    assert!(error.to_string().contains("limit"));
}

#[test]
fn jj_baseline_combined_envelope_limit_precedes_native_decoding() {
    let mut anchors = opaque_anchors(2, 1);
    anchors[1].operation_bytes = vec![0; MAX_JJ_OBSERVATION_OPERATION_BYTES / 2];
    anchors[1].view_bytes = vec![0; MAX_JJ_OBSERVATION_OPERATION_BYTES / 2 + 1];
    rejected(&heads(&anchors), &anchors, "byte limit");
}

#[test]
fn jj_baseline_aggregate_raw_limit_precedes_native_decoding() {
    assert_eq!(MAX_JJ_BASELINE_RAW_BYTES, 8 * 1024 * 1024);
    let mut anchors = opaque_anchors(8, MAX_JJ_OBSERVATION_OPERATION_BYTES);
    let mut extra = opaque_anchors(1, 1).pop().unwrap();
    extra.operation_id = format!("{:0128x}", 9);
    anchors.push(extra);
    assert_eq!(
        anchors
            .iter()
            .map(|a| a.operation_bytes.len() + a.view_bytes.len())
            .sum::<usize>(),
        MAX_JJ_BASELINE_RAW_BYTES + 1
    );
    let error = rejected(&heads(&anchors), &anchors, "byte limit");
    assert!(!error.to_string().contains("operation evidence"));
}

#[test]
fn jj_baseline_exact_raw_limit_reaches_native_validation_without_serialization_claim() {
    let anchors = opaque_anchors(8, MAX_JJ_OBSERVATION_OPERATION_BYTES);
    assert_eq!(
        anchors
            .iter()
            .map(|a| a.operation_bytes.len() + a.view_bytes.len())
            .sum::<usize>(),
        MAX_JJ_BASELINE_RAW_BYTES
    );
    let error = rejected(&heads(&anchors), &anchors, "operation");
    assert!(!error.to_string().contains("byte limit"));
}
