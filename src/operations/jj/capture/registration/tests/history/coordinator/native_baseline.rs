use super::*;
use crate::operations::jj::ancestry::{
    JjAncestryInput, verify_ancestry_to_baseline, verify_ancestry_to_receipt,
};
use crate::operations::jj::baseline_persistence::verify_native_baseline_snapshot_ref;

#[test]
fn admission_borrowed_cutoff_matches_owned_ancestry_without_moving_raw_anchors() {
    let fixture = Fixture::new();
    let source = "a5".repeat(32);
    let owned = baseline(&fixture, &source);
    let path = fixture.ancestor.join(format!("history-{source}.sqlite"));
    let journal = JjObservationJournal::open_at_path(&path).unwrap();
    let snapshot = journal
        .read_native_baseline(&source, &mut reads())
        .unwrap()
        .unwrap();
    let op_ptr = snapshot.record.anchors[0].operation_bytes.as_ptr();
    let view_ptr = snapshot.record.anchors[0].view_bytes.as_ptr();
    let receipt = verify_native_baseline_snapshot_ref(&snapshot).unwrap();
    assert_eq!(&receipt, owned.receipt());
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    let heads = [records[1].operation_id.clone()];
    let evidence = [&records[1], &records[0]];
    let input = || JjAncestryInput {
        source_id: &source,
        reader_profile: JJ_OBSERVATION_READER_PROFILE,
        baseline_id: receipt.baseline_id(),
        expected_native_generation: receipt.generation(),
        head_ids: &heads,
        operations: &evidence,
    };
    let borrowed = verify_ancestry_to_receipt(&receipt, input()).unwrap();
    let original = verify_ancestry_to_baseline(&owned, input()).unwrap();
    assert_eq!(borrowed.baseline_receipt(), original.baseline_receipt());
    assert_eq!(borrowed.head_ids(), original.head_ids());
    assert_eq!(
        borrowed.reached_baseline_ids(),
        original.reached_baseline_ids()
    );
    assert_eq!(borrowed.reaches_root(), original.reaches_root());
    assert_eq!(borrowed.ordered_operations().len(), records.len());
    assert_eq!(original.ordered_operations().len(), records.len());
    for ((left, right), expected) in borrowed
        .ordered_operations()
        .iter()
        .zip(original.ordered_operations())
        .zip(&records)
    {
        assert_eq!(left.evidence(), right.evidence());
        assert!(std::ptr::eq(left.evidence(), expected));
    }
    assert_eq!(snapshot.record.anchors[0].operation_bytes.as_ptr(), op_ptr);
    assert_eq!(snapshot.record.anchors[0].view_bytes.as_ptr(), view_ptr);
    assert_eq!(snapshot.record.anchors, owned.anchors());
}

#[test]
fn admission_borrowed_cutoff_refuses_invalid_native_operation_and_view_bytes() {
    let fixture = Fixture::new();
    let source = "a6".repeat(32);
    let _owned = baseline(&fixture, &source);
    let path = fixture.ancestor.join(format!("history-{source}.sqlite"));
    let journal = JjObservationJournal::open_at_path(&path).unwrap();
    for corrupt_view in [false, true] {
        let mut snapshot = journal
            .read_native_baseline(&source, &mut reads())
            .unwrap()
            .unwrap();
        verify_native_baseline_snapshot_ref(&snapshot).unwrap();
        let record = &mut snapshot.record.anchors[0];
        let bytes = if corrupt_view {
            &mut record.view_bytes
        } else {
            &mut record.operation_bytes
        };
        bytes[0] = 0;
        let raw = bytes.clone();
        let pointer = bytes.as_ptr();
        assert!(verify_native_baseline_snapshot_ref(&snapshot).is_err());
        let record = &snapshot.record.anchors[0];
        let bytes = if corrupt_view {
            &record.view_bytes
        } else {
            &record.operation_bytes
        };
        assert_eq!(bytes, &raw);
        assert_eq!(bytes.as_ptr(), pointer);
    }
}
