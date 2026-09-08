use crate::debug_context::snapshot;
use crate::jj_baseline_persistence::support as baseline_support;
use crate::jj_evidence::support::{Fixture, first, left, merge, paired, right};
use crate::jj_evidence::vectors::*;
use crate::jj_operation::support::{bytes_field, scalar, unhex};
use crate::jj_view::vectors::{MINIMAL_HEX, MINIMAL_ID};
use git_ai::model::jj_observation::{JJ_OBSERVATION_READER_PROFILE, JjOperationEvidence};
use git_ai::operations::jj::ancestry::{
    JjAncestryError, JjAncestryInput, VerifiedJjAncestry, verify_ancestry_to_baseline,
};
use git_ai::operations::jj::baseline_persistence::DurableCurrentStateBaseline;

#[path = "jj_ancestry_bounds.rs"]
mod bounds;
#[path = "jj_ancestry_errors.rs"]
mod errors;
#[path = "jj_ancestry_opaque.rs"]
mod opaque;
#[path = "jj_ancestry_real.rs"]
mod real;
#[path = "jj_ancestry_support.rs"]
mod support;
use support::*;

#[test]
fn jj_ancestry_diamond_is_parent_first_deterministic_and_borrows_original_evidence() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let records = [merge(), right(), left()];
    let references: Vec<_> = records.iter().collect();
    let heads = vec![MERGE_ID.to_owned(), RIGHT_ID.to_owned()];
    let journal = fixture.open();
    let before_status = journal.status(&fixture.source).unwrap();
    let before_pending = journal.pending(&fixture.source, 8).unwrap();
    let before_native = baseline_support::native_rows(&fixture);
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert_eq!(ordered_ids(&proof), [LEFT_ID, RIGHT_ID, MERGE_ID]);
    assert_eq!(proof.reached_baseline_ids(), [FIRST_ID]);
    assert!(!proof.reaches_root());
    assert!(std::ptr::eq(proof.baseline_receipt(), baseline.receipt()));
    assert!(std::ptr::eq(proof.head_ids(), heads.as_slice()));
    for operation in proof.ordered_operations() {
        let original = records
            .iter()
            .find(|record| record.operation_id == operation.operation().operation_id)
            .unwrap();
        assert!(std::ptr::eq(operation.evidence(), original));
        assert_eq!(
            operation.evidence().operation_bytes,
            original.operation_bytes
        );
        assert_eq!(operation.evidence().view_bytes, original.view_bytes);
    }
    assert_eq!(
        proof.ordered_operations()[2].operation().parent_ids,
        [LEFT_ID, RIGHT_ID]
    );
    let reversed_heads = heads.iter().rev().cloned().collect::<Vec<_>>();
    let reversed_records = references.iter().rev().copied().collect::<Vec<_>>();
    let reversed = checked(
        &fixture,
        &baseline,
        input(&baseline, &reversed_heads, &reversed_records),
    )
    .unwrap();
    assert_eq!(ordered_ids(&reversed), ordered_ids(&proof));
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), before_pending);
    assert_eq!(baseline_support::native_rows(&fixture), before_native);
}

#[test]
fn jj_ancestry_late_branch_does_not_stop_at_a_parent_named_by_the_baseline() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[left()]);
    assert_eq!(baseline.anchors()[0].parent_ids, [FIRST_ID]);
    let records = [merge(), right(), first()];
    let heads = vec![MERGE_ID.to_owned()];
    let incomplete = [&records[0], &records[1]];
    rejected(
        &fixture,
        &baseline,
        input(&baseline, &heads, &incomplete),
        None,
    );
    let complete = records.iter().collect::<Vec<_>>();
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &complete)).unwrap();
    assert_eq!(ordered_ids(&proof), [FIRST_ID, RIGHT_ID, MERGE_ID]);
    assert_eq!(proof.reached_baseline_ids(), [LEFT_ID]);
    assert!(proof.reaches_root());
}

#[test]
fn jj_ancestry_root_only_closure_reports_no_reached_baseline_or_newness_claim() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[left()]);
    let records = [right(), first()];
    let references = records.iter().collect::<Vec<_>>();
    let heads = vec![RIGHT_ID.to_owned()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert_eq!(ordered_ids(&proof), [FIRST_ID, RIGHT_ID]);
    assert!(proof.reached_baseline_ids().is_empty());
    assert!(proof.reaches_root());
    assert_eq!(proof.baseline_receipt().generation(), 1);
    assert_eq!(proof.baseline_receipt().source_id(), fixture.source);
}

#[test]
fn jj_ancestry_exact_terminal_heads_need_no_records_or_all_redundant_cutoffs() {
    for anchors in [vec![left()], vec![first(), left()]] {
        let fixture = Fixture::new();
        let baseline = durable(&fixture, &anchors);
        let heads = vec![LEFT_ID.to_owned()];
        let proof = checked(&fixture, &baseline, input(&baseline, &heads, &[])).unwrap();
        assert!(proof.ordered_operations().is_empty());
        assert_eq!(proof.reached_baseline_ids(), [LEFT_ID]);
        assert!(!proof.reaches_root());
    }
}

#[test]
fn jj_ancestry_sorted_roots_and_reached_cutoffs_do_not_mutate_borrowed_head_order() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[right(), left()]);
    let heads = vec![RIGHT_ID.to_owned(), LEFT_ID.to_owned()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &[])).unwrap();
    assert_eq!(proof.head_ids(), [RIGHT_ID, LEFT_ID]);
    assert!(std::ptr::eq(proof.head_ids(), heads.as_slice()));
    assert_eq!(proof.reached_baseline_ids(), [LEFT_ID, RIGHT_ID]);
    assert!(proof.ordered_operations().is_empty());
    assert!(!proof.reaches_root());
}

#[test]
fn jj_ancestry_retains_unrecorded_predecessor_fact_without_applying_semantic_policy() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let record = unrecorded();
    let references = [&record];
    let heads = vec![UNRECORDED_ID.to_owned()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert_eq!(
        proof.ordered_operations()[0]
            .operation()
            .commit_predecessors,
        None
    );
    assert!(proof.reaches_root());
    assert!(proof.reached_baseline_ids().is_empty());
}

#[test]
fn jj_ancestry_preserves_semantically_equivalent_raw_wire_encoding() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let mut record = left();
    let prefix = bytes_field(1, &unhex(MINIMAL_ID));
    assert!(record.operation_bytes.starts_with(&prefix));
    record.operation_bytes = [record.operation_bytes[prefix.len()..].to_vec(), prefix].concat();
    record.view_bytes = [scalar(12, 1), bytes_field(1, &[0xaa; 20])].concat();
    let references = [&record];
    let heads = vec![LEFT_ID.to_owned()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert!(std::ptr::eq(
        proof.ordered_operations()[0].evidence(),
        &record
    ));
    assert_ne!(record.operation_bytes, left().operation_bytes);
    assert_ne!(record.view_bytes, left().view_bytes);
}
