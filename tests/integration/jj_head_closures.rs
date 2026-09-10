use super::*;
use git_ai::operations::jj::ancestry::JjHeadClosure;

pub(crate) fn summary(values: &[JjHeadClosure]) -> Vec<(String, Vec<String>, bool)> {
    values
        .iter()
        .map(|value| {
            (
                value.head_id().to_owned(),
                value.reached_baseline_ids().to_vec(),
                value.reaches_root(),
            )
        })
        .collect()
}

pub(crate) fn assert_closures(actual: &[JjHeadClosure], expected: &[(&str, &[&str], bool)]) {
    let mut expected: Vec<_> = expected
        .iter()
        .map(|(head, anchors, root)| {
            let mut anchors: Vec<_> = anchors.iter().map(|id| (*id).to_owned()).collect();
            anchors.sort();
            ((*head).to_owned(), anchors, *root)
        })
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(summary(actual), expected);
    assert_eq!(actual, actual.to_vec().as_slice());
}

#[test]
fn jj_head_closures_distinguish_unrelated_anchors_and_native_root() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first(), merge()]);
    let records = [left(), native::rich_parent(), unrecorded()];
    let heads = records
        .iter()
        .map(|record| record.operation_id.clone())
        .collect::<Vec<_>>();
    let references = records.iter().collect::<Vec<_>>();
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert_closures(
        proof.head_closures(),
        &[
            (LEFT_ID, &[FIRST_ID], false),
            (native::RICH_PARENT_ID, &[MERGE_ID], false),
            (UNRECORDED_ID, &[], true),
        ],
    );
    let mut aggregate = vec![FIRST_ID.to_owned(), MERGE_ID.to_owned()];
    aggregate.sort();
    assert_eq!(proof.reached_baseline_ids(), aggregate);
    assert!(proof.reaches_root());
    assert_eq!(proof.ordered_operations().len(), 3);
}

#[test]
fn jj_head_closures_shared_diamond_and_redundant_heads_keep_each_full_closure() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let records = [merge(), right(), left()];
    let references = records.iter().collect::<Vec<_>>();
    let heads = vec![MERGE_ID.to_owned(), RIGHT_ID.to_owned(), LEFT_ID.to_owned()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert_closures(
        proof.head_closures(),
        &[
            (MERGE_ID, &[FIRST_ID], false),
            (RIGHT_ID, &[FIRST_ID], false),
            (LEFT_ID, &[FIRST_ID], false),
        ],
    );
    assert_eq!(ordered_ids(&proof), [LEFT_ID, RIGHT_ID, MERGE_ID]);
    assert_eq!(proof.reached_baseline_ids(), [FIRST_ID]);
    assert!(!proof.reaches_root());
}

#[test]
fn jj_head_closures_mixed_head_does_not_give_its_anchor_to_root_only_head() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[left()]);
    let records = [merge(), right(), first()];
    let references = records.iter().collect::<Vec<_>>();
    let heads = vec![MERGE_ID.to_owned(), RIGHT_ID.to_owned()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert_closures(
        proof.head_closures(),
        &[(MERGE_ID, &[LEFT_ID], true), (RIGHT_ID, &[], true)],
    );
    assert_eq!(ordered_ids(&proof), [FIRST_ID, RIGHT_ID, MERGE_ID]);
    assert_eq!(proof.reached_baseline_ids(), [LEFT_ID]);
    assert!(proof.reaches_root());
}

#[test]
fn jj_head_closures_terminal_heads_stop_before_overlapping_baseline_parents() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first(), left()]);
    let heads = vec![LEFT_ID.to_owned(), FIRST_ID.to_owned()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &[])).unwrap();
    assert_closures(
        proof.head_closures(),
        &[(LEFT_ID, &[LEFT_ID], false), (FIRST_ID, &[FIRST_ID], false)],
    );
    assert!(proof.ordered_operations().is_empty());
    assert!(!proof.reaches_root());
}

#[test]
fn jj_head_closures_sort_metadata_without_changing_input_order_or_raw_borrows() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[left(), right()]);
    let records = [native::rich_parent(), merge()];
    let references = records.iter().collect::<Vec<_>>();
    let heads = vec![
        native::RICH_PARENT_ID.to_owned(),
        RIGHT_ID.to_owned(),
        MERGE_ID.to_owned(),
    ];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert_closures(
        proof.head_closures(),
        &[
            (native::RICH_PARENT_ID, &[LEFT_ID, RIGHT_ID], false),
            (MERGE_ID, &[LEFT_ID, RIGHT_ID], false),
            (RIGHT_ID, &[RIGHT_ID], false),
        ],
    );
    let reversed_heads = heads.iter().rev().cloned().collect::<Vec<_>>();
    let reversed_records = references.iter().rev().copied().collect::<Vec<_>>();
    let reversed = checked(
        &fixture,
        &baseline,
        input(&baseline, &reversed_heads, &reversed_records),
    )
    .unwrap();
    assert_eq!(proof.head_closures(), reversed.head_closures());
    assert_eq!(ordered_ids(&proof), ordered_ids(&reversed));
    assert!(std::ptr::eq(proof.head_ids(), heads.as_slice()));
    assert!(std::ptr::eq(reversed.head_ids(), reversed_heads.as_slice()));
    for verified in [proof.ordered_operations(), reversed.ordered_operations()] {
        for operation in verified {
            let original = records
                .iter()
                .find(|record| record.operation_id == operation.operation().operation_id)
                .unwrap();
            assert!(std::ptr::eq(operation.evidence(), original));
            assert_eq!(
                operation.evidence().operation_bytes.as_ptr(),
                original.operation_bytes.as_ptr()
            );
            assert_eq!(
                operation.evidence().view_bytes.as_ptr(),
                original.view_bytes.as_ptr()
            );
        }
    }
}
