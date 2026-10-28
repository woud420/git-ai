use super::*;
use git_ai::operations::jj::ancestry::{ClosedJjFrontier, verify_ancestry_segment};

#[test]
fn jj_segments_compose_more_than_256_operations_without_moving_original_cutoff() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[native::merge()]);
    let chain = native::chain(257);
    let first_heads = vec![chain[127].operation_id.clone()];
    let first_refs: Vec<_> = chain[..128].iter().collect();
    let proof = verify_ancestry_to_baseline(&baseline, input(&baseline, &first_heads, &first_refs))
        .unwrap();
    let frontier = ClosedJjFrontier::from_proof(&proof);
    let heads = vec![chain[256].operation_id.clone()];
    let refs: Vec<_> = chain[128..].iter().collect();
    let segment = verify_ancestry_segment(&frontier, input(&baseline, &heads, &refs)).unwrap();
    assert_eq!(segment.ordered_operations().len(), 129);
    assert_eq!(segment.reached_baseline_ids(), [native::MERGE_ID]);
    assert_eq!(segment.baseline_receipt(), baseline.receipt());
    assert_eq!(
        ClosedJjFrontier::from_proof(&segment).head_closures(),
        segment.head_closures()
    );
}

#[test]
fn jj_segments_late_branch_must_supply_its_own_ancestry() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let heads = vec![LEFT_ID.to_owned()];
    let record = left();
    let records = [&record];
    let proof = verify_ancestry_to_baseline(&baseline, input(&baseline, &heads, &records)).unwrap();
    let frontier = ClosedJjFrontier::from_proof(&proof);
    let heads = vec![MERGE_ID.to_owned()];
    let merge = merge();
    assert!(verify_ancestry_segment(&frontier, input(&baseline, &heads, &[&merge])).is_err());
    let right = right();
    let records = [&merge, &right];
    let proof = verify_ancestry_segment(&frontier, input(&baseline, &heads, &records)).unwrap();
    assert_eq!(ordered_ids(&proof), [RIGHT_ID, MERGE_ID]);
    assert_eq!(proof.reached_baseline_ids(), [FIRST_ID]);
}

#[test]
fn jj_segments_reject_resupplied_frontier_and_wrong_scope() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let left = left();
    let heads = vec![LEFT_ID.to_owned()];
    let records = [&left];
    let proof = verify_ancestry_to_baseline(&baseline, input(&baseline, &heads, &records)).unwrap();
    let frontier = ClosedJjFrontier::from_proof(&proof);
    assert!(verify_ancestry_segment(&frontier, input(&baseline, &heads, &records)).is_err());
    let mut wrong = input(&baseline, &heads, &[]);
    wrong.source_id = "wrong source";
    assert!(verify_ancestry_segment(&frontier, wrong).is_err());
    assert!(
        verify_ancestry_segment(&frontier, input(&baseline, &heads, &[]))
            .unwrap()
            .ordered_operations()
            .is_empty()
    );
}

#[test]
fn jj_segments_keep_each_heads_original_anchor_and_root_provenance_separate() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[left()]);
    let initial_heads = vec![LEFT_ID.to_owned(), RIGHT_ID.to_owned()];
    let first = first();
    let right = right();
    let initial = [&first, &right];
    let proof =
        verify_ancestry_to_baseline(&baseline, input(&baseline, &initial_heads, &initial)).unwrap();
    let frontier = ClosedJjFrontier::from_proof(&proof);
    let merge = merge();
    let heads = vec![LEFT_ID.to_owned(), MERGE_ID.to_owned()];
    let records = [&merge];
    let proof = verify_ancestry_segment(&frontier, input(&baseline, &heads, &records)).unwrap();
    for closure in proof.head_closures() {
        assert_eq!(closure.reached_baseline_ids(), [LEFT_ID]);
        assert_eq!(closure.reaches_root(), closure.head_id() == MERGE_ID);
    }
}
