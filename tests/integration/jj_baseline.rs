use crate::debug_context::snapshot;
use crate::jj_evidence::support::{Fixture, first, left, merge, paired, right};
use crate::jj_evidence::vectors::*;
use crate::jj_operation::support::{bytes_field, scalar, unhex};
use crate::jj_view::vectors::{MINIMAL_HEX, MINIMAL_ID, RICH_HEX};
use git_ai::model::jj_observation::{JJ_OBSERVATION_READER_PROFILE, JjOperationEvidence};
use git_ai::operations::jj::baseline::{
    JjBaselineBoundary, JjBaselineError, MAX_JJ_BASELINE_HEADS, MAX_JJ_BASELINE_RAW_BYTES,
    PreparedCurrentStateBaseline, prepare_current_state_baseline,
};
use git_ai::operations::jj::evidence::JjEvidenceError;

#[path = "jj_baseline_limits.rs"]
mod limits;
#[path = "jj_baseline_real.rs"]
mod real;

fn heads(anchors: &[JjOperationEvidence]) -> Vec<String> {
    anchors
        .iter()
        .map(|anchor| anchor.operation_id.clone())
        .collect()
}

fn prepare<'a>(
    heads: &'a [String],
    anchors: &'a [JjOperationEvidence],
) -> Result<PreparedCurrentStateBaseline<'a>, JjBaselineError> {
    prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, heads, anchors)
}

fn rejected(heads: &[String], anchors: &[JjOperationEvidence], category: &str) -> JjBaselineError {
    let error = match prepare(heads, anchors) {
        Ok(_) => panic!("expected {category} rejection"),
        Err(error) => error,
    };
    let text = error.to_string().to_ascii_lowercase();
    assert!(text.contains(category), "expected {category}: {text}");
    error
}

#[test]
fn jj_baseline_unprovided_older_parents_are_an_explicit_boundary_without_journal_writes() {
    let fixture = Fixture::new();
    let journal = fixture.open();
    let anchors = vec![merge()];
    let heads = heads(&anchors);
    let before_status = journal.status(&fixture.source).unwrap();
    let before_pending = journal.pending(&fixture.source, 4).unwrap();
    let before_files = snapshot(fixture.repo.path());
    let before_home = snapshot(fixture.repo.test_home_path());

    let prepared = prepare(&heads, &anchors).unwrap();
    assert_eq!(prepared.boundary(), JjBaselineBoundary::CurrentState);
    assert_eq!(prepared.reader_profile(), JJ_OBSERVATION_READER_PROFILE);
    assert_eq!(prepared.captured_head_ids(), heads);
    assert!(std::ptr::eq(prepared.captured_head_ids(), heads.as_slice()));
    assert_eq!(prepared.anchors().len(), 1);
    let anchor = &prepared.anchors()[0];
    assert!(std::ptr::eq(anchor.evidence(), &anchors[0]));
    assert_eq!(anchor.operation().parent_ids, [LEFT_ID, RIGHT_ID]);
    assert_eq!(anchor.evidence().parent_ids, [LEFT_ID, RIGHT_ID]);
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 4).unwrap(), before_pending);
    assert_eq!(before_status.generation, 0);
    assert!(before_status.observed_heads.is_empty());
    assert!(before_status.applied_heads.is_empty());
    assert_eq!(before_status.pending_operations, 0);
    assert_eq!(snapshot(fixture.repo.path()), before_files);
    assert_eq!(snapshot(fixture.repo.test_home_path()), before_home);
}

#[test]
fn jj_baseline_preparation_does_not_promote_or_rewrite_existing_opaque_progress() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    journal
        .capture(&fixture.batch(FIRST_ID, vec![first()]))
        .unwrap();
    let before_status = journal.status(&fixture.source).unwrap();
    let before_pending = journal.pending(&fixture.source, 4).unwrap();
    let anchors = vec![merge()];
    let heads = heads(&anchors);
    prepare(&heads, &anchors).unwrap();
    let lookup = journal.lookup_observed(&fixture.source, &heads).unwrap();
    assert!(lookup.operations.is_empty());
    assert_eq!(lookup.status, before_status);
    assert_eq!(journal.pending(&fixture.source, 4).unwrap(), before_pending);
    assert_eq!(before_status.generation, 1);
    assert_eq!(before_status.observed_heads, [FIRST_ID]);
    assert!(before_status.applied_heads.is_empty());
}

#[test]
fn jj_baseline_exact_raw_head_set_preserves_input_order_and_redundant_heads() {
    let anchors = vec![left(), first(), right()];
    let heads = vec![RIGHT_ID.to_owned(), LEFT_ID.to_owned(), FIRST_ID.to_owned()];
    let prepared = prepare(&heads, &anchors).unwrap();
    assert!(std::ptr::eq(prepared.captured_head_ids(), heads.as_slice()));
    assert_eq!(prepared.captured_head_ids(), [RIGHT_ID, LEFT_ID, FIRST_ID]);
    assert_eq!(prepared.anchors().len(), anchors.len());
    for (proof, original) in prepared.anchors().iter().zip(&anchors) {
        let original: &JjOperationEvidence = original;
        assert!(std::ptr::eq::<JjOperationEvidence>(
            proof.evidence(),
            original
        ));
        assert_eq!(proof.operation().parent_ids, original.parent_ids);
    }
    let reversed = heads.iter().rev().cloned().collect::<Vec<_>>();
    assert_eq!(prepare(&reversed, &anchors).unwrap().anchors().len(), 3);
}

#[test]
fn jj_baseline_requires_native_hashes_for_every_anchor_operation_and_view() {
    for wrong_view in [false, true] {
        let mut anchor = first();
        if wrong_view {
            anchor.view_bytes = unhex(RICH_HEX);
        } else {
            anchor.operation_bytes = unhex(LEFT_HEX);
        }
        let error = rejected(&heads(&[anchor.clone()]), &[anchor], "hash");
        let source = std::error::Error::source(&error).unwrap();
        assert!(source.downcast_ref::<JjEvidenceError>().is_some());
    }
}

#[test]
fn jj_baseline_requires_exact_ordered_parent_envelope_and_view_join() {
    let mut reversed = merge();
    reversed.parent_ids.reverse();
    rejected(&heads(&[reversed.clone()]), &[reversed], "parent");
    let mut false_root = merge();
    false_root.parent_ids = vec!["00".repeat(64)];
    rejected(&heads(&[false_root.clone()]), &[false_root], "parent");
    let mut swapped = merge();
    swapped.view_id = MINIMAL_ID.to_owned();
    swapped.view_bytes = unhex(MINIMAL_HEX);
    rejected(&heads(&[swapped.clone()]), &[swapped], "view");
}

#[test]
fn jj_baseline_keeps_unrecorded_predecessors_distinct_from_recorded_empty() {
    let anchors = vec![
        first(),
        paired(
            UNRECORDED_ID,
            UNRECORDED_HEX,
            &[&"00".repeat(64)],
            MINIMAL_ID,
            MINIMAL_HEX,
        ),
    ];
    let heads = heads(&anchors);
    let prepared = prepare(&heads, &anchors).unwrap();
    assert!(
        prepared.anchors()[0]
            .operation()
            .commit_predecessors
            .as_ref()
            .unwrap()
            .is_empty()
    );
    assert_eq!(prepared.anchors()[1].operation().commit_predecessors, None);
    assert_eq!(prepared.boundary(), JjBaselineBoundary::CurrentState);
}

#[test]
fn jj_baseline_borrows_original_semantically_equivalent_wire_encoding() {
    let mut anchor = first();
    let prefix = bytes_field(1, &unhex(MINIMAL_ID));
    assert!(anchor.operation_bytes.starts_with(&prefix));
    anchor.operation_bytes = [anchor.operation_bytes[prefix.len()..].to_vec(), prefix].concat();
    anchor.view_bytes = [scalar(12, 1), bytes_field(1, &[0xaa; 20])].concat();
    let anchors = [anchor];
    let heads = heads(&anchors);
    let prepared = prepare(&heads, &anchors).unwrap();
    let proof = &prepared.anchors()[0];
    assert!(std::ptr::eq(proof.evidence(), &anchors[0]));
    assert_eq!(proof.operation().operation_id, FIRST_ID);
    assert_eq!(proof.view().view_id, MINIMAL_ID);
    assert_ne!(proof.evidence().operation_bytes, unhex(FIRST_HEX));
    assert_ne!(proof.evidence().view_bytes, unhex(MINIMAL_HEX));
}

#[test]
fn jj_baseline_profile_is_explicit_and_errors_implement_standard_error() {
    let anchors = [first()];
    let heads = heads(&anchors);
    for profile in [
        "",
        "jj-simple-op-store/0.45.0",
        "jj-simple-op-store/0.45.1 ",
    ] {
        let error = match prepare_current_state_baseline(profile, &heads, &anchors) {
            Ok(_) => panic!("unsupported profile accepted"),
            Err(error) => error,
        };
        let standard: &dyn std::error::Error = &error;
        assert!(standard.to_string().contains("profile"));
        assert!(standard.source().is_some());
    }
}

#[test]
fn jj_baseline_rejects_empty_duplicate_and_malformed_head_identities() {
    rejected(&[], &[], "head");
    let anchors = [first(), first()];
    rejected(
        &[FIRST_ID.to_owned(), FIRST_ID.to_owned()],
        &anchors,
        "duplicate",
    );
    for id in ["abc".to_owned(), FIRST_ID.to_uppercase(), "g".repeat(128)] {
        rejected(&[id], &[first()], "identity");
    }
    rejected(&["00".repeat(64)], &[first()], "root");
}

#[test]
fn jj_baseline_rejects_missing_detached_extra_and_duplicate_anchors() {
    rejected(&[FIRST_ID.to_owned()], &[], "anchor");
    rejected(&[FIRST_ID.to_owned()], &[first(), left()], "anchor");
    rejected(&[FIRST_ID.to_owned()], &[left()], "anchor");
    rejected(
        &[FIRST_ID.to_owned(), LEFT_ID.to_owned()],
        &[first(), first()],
        "duplicate",
    );
}

#[test]
fn jj_baseline_checks_all_structural_envelopes_before_any_native_decode() {
    let mut malformed_wire = first();
    malformed_wire.operation_bytes = vec![0];
    let mut invalid_envelope = left();
    invalid_envelope.parent_ids = vec![FIRST_ID.to_owned(); 2];
    let anchors = [malformed_wire, invalid_envelope];
    rejected(&heads(&anchors), &anchors, "duplicate");
}
