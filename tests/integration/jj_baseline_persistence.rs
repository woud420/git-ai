use crate::jj_evidence::support::{Fixture, first, left, merge, paired, right};
use crate::jj_evidence::vectors::*;
use crate::jj_operation::support::{bytes_field, scalar, unhex};
use crate::jj_view::vectors::{MINIMAL_HEX, MINIMAL_ID};
use git_ai::model::jj_observation::{JJ_OBSERVATION_READER_PROFILE, JjOperationEvidence};
use git_ai::model::repository::jj_observation_journal::{
    JjObservationJournal, JournalError, ReadBudget,
};
use git_ai::model::repository::sqlite::open_with_memory_limits;
use git_ai::operations::jj::baseline::{
    JjBaselineBoundary, JjBaselineError, prepare_current_state_baseline,
};
use git_ai::operations::jj::baseline_persistence::{
    BaselinePersistenceOutcome, BaselineReceipt, DurableCurrentStateBaseline,
    JjBaselinePersistenceError, persist_current_state_baseline, reopen_current_state_baseline,
};
use rusqlite::params;

#[path = "jj_baseline_persistence_faults.rs"]
mod faults;
#[path = "jj_baseline_persistence_limits.rs"]
mod limits;
#[path = "jj_baseline_persistence_real.rs"]
mod real;
#[path = "jj_baseline_persistence_support.rs"]
pub(super) mod support;
#[path = "jj_baseline_persistence_writers.rs"]
mod writers;

use support::*;

#[test]
fn jj_baseline_persistence_absent_source_has_no_baseline_and_consumes_no_payload() {
    let fixture = Fixture::new();
    let journal = fixture.open();
    let mut budget = ReadBudget::new(0);
    assert!(
        reopen_current_state_baseline(&journal, &fixture.source, &mut budget)
            .unwrap()
            .is_none()
    );
    assert_eq!(budget.consumed(), 0);
    assert_eq!(native_counts(&fixture), (0, 0));
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 0);
}

#[test]
fn jj_baseline_persistence_installs_reopens_exact_unknown_parent_and_unrecorded_evidence() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let anchors = vec![
        paired(
            DETACHED_ID,
            DETACHED_HEX,
            &[&"ee".repeat(64)],
            MINIMAL_ID,
            MINIMAL_HEX,
        ),
        paired(
            UNRECORDED_ID,
            UNRECORDED_HEX,
            &[&"00".repeat(64)],
            MINIMAL_ID,
            MINIMAL_HEX,
        ),
    ];
    let before_status = journal.status(&fixture.source).unwrap();
    let before_pending = journal.pending(&fixture.source, 8).unwrap();
    let receipt = installed(install(&mut journal, &fixture.source, &anchors).unwrap());
    assert_receipt(&receipt, &fixture.source, &heads(&anchors));
    assert_eq!(native_counts(&fixture), (1, 1));
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), before_pending);
    assert!(
        journal
            .lookup_observed(&fixture.source, &heads(&anchors))
            .unwrap()
            .operations
            .is_empty()
    );
    drop(journal);

    let journal = fixture.open();
    let mut budget = full_budget();
    let durable = reopen_current_state_baseline(&journal, &fixture.source, &mut budget)
        .unwrap()
        .unwrap();
    assert_same_receipt(durable.receipt(), &receipt);
    assert_eq!(durable.anchors(), canonical_anchors(&anchors));
    assert!(
        durable
            .anchors()
            .iter()
            .any(|anchor| anchor.parent_ids == ["ee".repeat(64)])
    );
    let prepared = prepare_current_state_baseline(
        durable.receipt().reader_profile(),
        durable.receipt().captured_head_ids(),
        durable.anchors(),
    )
    .unwrap();
    assert!(
        prepared
            .anchors()
            .iter()
            .any(|anchor| anchor.operation().commit_predecessors.is_none())
    );
    assert_eq!(budget.consumed(), stored_bytes(&fixture));
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), before_pending);
}

#[test]
fn jj_baseline_persistence_identical_generation_zero_retry_is_idempotent_after_reopen() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let anchors = [merge()];
    let receipt = installed(install(&mut journal, &fixture.source, &anchors).unwrap());
    let before = native_rows(&fixture);
    assert_same_receipt(
        &already(install(&mut journal, &fixture.source, &anchors).unwrap()),
        &receipt,
    );
    drop(journal);
    let mut journal = fixture.open();
    assert_same_receipt(
        &already(install(&mut journal, &fixture.source, &anchors).unwrap()),
        &receipt,
    );
    assert_eq!(native_rows(&fixture), before);
    assert_same_receipt(
        reopen(&journal, &fixture.source)
            .unwrap()
            .unwrap()
            .receipt(),
        &receipt,
    );
}

#[test]
fn jj_baseline_persistence_collection_reordering_has_one_canonical_receipt() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let anchors = [right(), first(), left()];
    let raw_heads = vec![LEFT_ID.to_owned(), RIGHT_ID.to_owned(), FIRST_ID.to_owned()];
    let prepared =
        prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, &raw_heads, &anchors)
            .unwrap();
    let receipt = installed(
        persist_current_state_baseline(&mut journal, &fixture.source, 0, &prepared).unwrap(),
    );
    let before = native_rows(&fixture);
    let reversed = [left(), first(), right()];
    assert_same_receipt(
        &already(install(&mut journal, &fixture.source, &reversed).unwrap()),
        &receipt,
    );
    assert_receipt(&receipt, &fixture.source, &raw_heads);
    assert_eq!(
        reopen(&journal, &fixture.source)
            .unwrap()
            .unwrap()
            .anchors(),
        canonical_anchors(&anchors)
    );
    assert_eq!(native_rows(&fixture), before);
}

#[test]
fn jj_baseline_persistence_equivalent_native_wire_bytes_are_a_different_request() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let before = native_rows(&fixture);
    let mut alternate = first();
    let prefix = bytes_field(1, &unhex(MINIMAL_ID));
    assert!(alternate.operation_bytes.starts_with(&prefix));
    alternate.operation_bytes =
        [alternate.operation_bytes[prefix.len()..].to_vec(), prefix].concat();
    alternate.view_bytes = [scalar(12, 1), bytes_field(1, &[0xaa; 20])].concat();
    let anchors = [alternate];
    prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, &heads(&anchors), &anchors)
        .unwrap();
    rejected(install(&mut journal, &fixture.source, &anchors), "conflict");
    assert_eq!(native_rows(&fixture), before);
    assert_eq!(
        reopen(&journal, &fixture.source)
            .unwrap()
            .unwrap()
            .anchors(),
        [first()]
    );
}

#[test]
fn jj_baseline_persistence_source_namespaces_install_independently() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let other = "02".repeat(32);
    let a = installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let b = installed(install(&mut journal, &other, &[first()]).unwrap());
    assert_ne!(a.baseline_id(), b.baseline_id());
    assert_receipt(&a, &fixture.source, &[FIRST_ID.to_owned()]);
    assert_receipt(&b, &other, &[FIRST_ID.to_owned()]);
    assert_same_receipt(
        reopen(&journal, &fixture.source)
            .unwrap()
            .unwrap()
            .receipt(),
        &a,
    );
    assert_same_receipt(reopen(&journal, &other).unwrap().unwrap().receipt(), &b);
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 0);
    assert_eq!(journal.status(&other).unwrap().generation, 0);
}

#[test]
fn jj_baseline_persistence_opaque_same_identity_cannot_shadow_native_evidence() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut opaque = first();
    opaque.operation_bytes = unhex(LEFT_HEX);
    journal
        .capture(&fixture.batch(FIRST_ID, vec![opaque.clone()]))
        .unwrap();
    let before_status = journal.status(&fixture.source).unwrap();
    let before_pending = journal.pending(&fixture.source, 8).unwrap();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    assert_eq!(
        reopen(&journal, &fixture.source)
            .unwrap()
            .unwrap()
            .anchors(),
        [first()]
    );
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), before_pending);
    assert_eq!(
        journal
            .lookup_observed(&fixture.source, &[FIRST_ID.to_owned()])
            .unwrap()
            .operations[FIRST_ID],
        opaque
    );
}

#[test]
fn jj_baseline_persistence_opaque_progress_advances_without_moving_native_epoch() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let receipt = installed(install(&mut journal, &fixture.source, &[merge()]).unwrap());
    let before = native_rows(&fixture);
    journal
        .capture(&fixture.batch(FIRST_ID, vec![first()]))
        .unwrap();
    let mut next = fixture.batch(LEFT_ID, vec![left()]);
    next.expected_generation = 1;
    next.expected_observed_heads = vec![FIRST_ID.to_owned()];
    journal.capture(&next).unwrap();
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 2);
    assert_same_receipt(
        reopen(&journal, &fixture.source)
            .unwrap()
            .unwrap()
            .receipt(),
        &receipt,
    );
    assert_eq!(native_rows(&fixture), before);
    assert_same_receipt(
        &already(install(&mut journal, &fixture.source, &[merge()]).unwrap()),
        &receipt,
    );
}

#[test]
fn jj_baseline_persistence_invalid_source_or_nonzero_expected_generation_writes_nothing() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let anchors = [first()];
    let heads = heads(&anchors);
    let prepared =
        prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, &heads, &anchors).unwrap();
    for source in ["", "abc", &"AA".repeat(32), &"g".repeat(64)] {
        rejected(
            persist_current_state_baseline(&mut journal, source, 0, &prepared),
            "source",
        );
        rejected(reopen(&journal, source), "source");
    }
    for generation in [1, u64::MAX] {
        rejected(
            persist_current_state_baseline(&mut journal, &fixture.source, generation, &prepared),
            "generation",
        );
    }
    assert_eq!(native_counts(&fixture), (0, 0));
    installed(install(&mut journal, &fixture.source, &anchors).unwrap());
    let before = native_rows(&fixture);
    rejected(
        persist_current_state_baseline(&mut journal, &fixture.source, 1, &prepared),
        "generation",
    );
    assert_eq!(native_rows(&fixture), before);
}

#[test]
fn jj_baseline_persistence_changed_cutoff_cannot_replace_first_baseline() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let receipt = installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let before = native_rows(&fixture);
    rejected(
        install(&mut journal, &fixture.source, &[left()]),
        "conflict",
    );
    assert_same_receipt(
        reopen(&journal, &fixture.source)
            .unwrap()
            .unwrap()
            .receipt(),
        &receipt,
    );
    assert_eq!(native_rows(&fixture), before);
}
