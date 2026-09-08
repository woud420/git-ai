use super::*;
use git_ai::model::repository::sqlite::open_with_memory_limits;

#[test]
fn jj_ancestry_opaque_lookup_ids_are_candidates_not_terminals_or_native_generations() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[left()]);
    let mut journal = fixture.open();
    journal
        .capture(&fixture.batch(FIRST_ID, vec![first()]))
        .unwrap();
    let mut next = fixture.batch(RIGHT_ID, vec![right()]);
    next.expected_generation = 1;
    next.expected_observed_heads = vec![FIRST_ID.to_owned()];
    journal.capture(&next).unwrap();
    let lookup = journal
        .lookup_observed(&fixture.source, &[RIGHT_ID.to_owned(), FIRST_ID.to_owned()])
        .unwrap();
    assert_eq!(lookup.status.generation, 2);
    assert_eq!(baseline.receipt().generation(), 1);
    let heads = vec![RIGHT_ID.to_owned()];
    let incomplete = [&lookup.operations[RIGHT_ID]];
    rejected(
        &fixture,
        &baseline,
        input(&baseline, &heads, &incomplete),
        Some("missing"),
    );
    let complete = [&lookup.operations[RIGHT_ID], &lookup.operations[FIRST_ID]];
    let before_pending = journal.pending(&fixture.source, 8).unwrap();
    let before_native = baseline_support::native_rows(&fixture);
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &complete)).unwrap();
    assert_eq!(ordered_ids(&proof), [FIRST_ID, RIGHT_ID]);
    assert!(proof.reached_baseline_ids().is_empty());
    assert!(proof.reaches_root());
    assert!(std::ptr::eq(
        proof.ordered_operations()[0].evidence(),
        &lookup.operations[FIRST_ID]
    ));
    let mut wrong_generation = input(&baseline, &heads, &complete);
    wrong_generation.expected_native_generation = lookup.status.generation;
    rejected(&fixture, &baseline, wrong_generation, Some("generation"));
    assert_eq!(journal.status(&fixture.source).unwrap(), lookup.status);
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), before_pending);
    assert_eq!(baseline_support::native_rows(&fixture), before_native);
}

#[test]
fn jj_ancestry_storage_checksum_valid_opaque_bytes_still_need_native_identity() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let mut journal = fixture.open();
    let mut false_identity = first();
    false_identity.operation_id = LEFT_ID.to_owned();
    journal
        .capture(&fixture.batch(LEFT_ID, vec![false_identity.clone()]))
        .unwrap();
    let heads = vec![LEFT_ID.to_owned()];
    let lookup = journal.lookup_observed(&fixture.source, &heads).unwrap();
    assert_eq!(lookup.operations[LEFT_ID], false_identity);
    let references = [&lookup.operations[LEFT_ID]];
    let before = journal.pending(&fixture.source, 8).unwrap();
    rejected(
        &fixture,
        &baseline,
        input(&baseline, &heads, &references),
        Some("hash"),
    );
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), before);
    assert_eq!(journal.status(&fixture.source).unwrap(), lookup.status);
}

#[test]
fn jj_ancestry_borrows_verified_baseline_snapshot_without_reopening_changed_storage() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let mut journal = fixture.open();
    journal
        .capture(&fixture.batch(FIRST_ID, vec![first()]))
        .unwrap();
    let connection = open_with_memory_limits(&fixture.path).unwrap();
    connection
        .execute(
            "UPDATE jj_native_baselines SET checksum = ?1 WHERE source_id = ?2",
            rusqlite::params!["00".repeat(32), fixture.source],
        )
        .unwrap();
    assert!(baseline_support::reopen(&journal, &fixture.source).is_err());
    assert!(
        journal
            .lookup_observed(&fixture.source, &[FIRST_ID.to_owned()])
            .unwrap()
            .operations
            .contains_key(FIRST_ID)
    );
    let heads = vec![LEFT_ID.to_owned()];
    let record = left();
    let references = [&record];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
    assert!(std::ptr::eq(proof.baseline_receipt(), baseline.receipt()));
    assert_eq!(proof.reached_baseline_ids(), [FIRST_ID]);
    assert_eq!(proof.baseline_receipt().generation(), 1);
    assert!(baseline_support::reopen(&journal, &fixture.source).is_err());
}
