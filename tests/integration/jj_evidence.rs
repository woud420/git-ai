use crate::debug_context::snapshot;
use crate::jj_operation::support::{bytes_field, scalar, unhex};
use crate::jj_view::vectors::{MINIMAL_HEX, MINIMAL_ID, RICH_HEX, RICH_ID};
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::model::jj_observation::{
    JJ_OBSERVATION_READER_PROFILE, JJ_OBSERVATION_SCHEMA_VERSION, JjObservationBatch,
    JjOperationEvidence, MAX_JJ_OBSERVATION_OPERATION_BYTES,
};
use git_ai::model::repository::jj_observation_journal::{CaptureOutcome, JjObservationJournal};
use git_ai::operations::jj::evidence::{JjEvidenceError, VerifiedJjEvidence, verify_evidence};

#[path = "jj_evidence_support.rs"]
pub(super) mod support;
#[path = "jj_evidence_vectors.rs"]
pub(super) mod vectors;

use support::*;
use vectors::*;

fn verify(evidence: &JjOperationEvidence) -> Result<VerifiedJjEvidence<'_>, JjEvidenceError> {
    verify_evidence(JJ_OBSERVATION_READER_PROFILE, evidence)
}

fn rejected(evidence: &JjOperationEvidence, category: &str) {
    let error = match verify(evidence) {
        Ok(_) => panic!("expected {category} rejection"),
        Err(error) => error,
    };
    let text = error.to_string().to_ascii_lowercase();
    assert!(text.contains(category), "expected {category}: {text}");
}

#[test]
fn jj_evidence_verifies_captured_operation_view_join_after_journal_reopen() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    assert_eq!(
        journal
            .capture(&fixture.batch(MERGE_ID, vec![merge(), right(), first(), left()]))
            .unwrap(),
        CaptureOutcome::Captured {
            inserted_operations: 4
        }
    );
    drop(journal);
    let journal = fixture.open();
    let known = journal
        .lookup_observed(&fixture.source, &[MERGE_ID.to_owned()])
        .unwrap();
    let before_status = known.status.clone();
    let before_pending = journal.pending(&fixture.source, 4).unwrap();
    let before_files = snapshot(fixture.repo.path());
    let raw = &known.operations[MERGE_ID];
    let proof = verify(raw).unwrap();
    assert!(std::ptr::eq(proof.evidence(), raw));
    assert_eq!(proof.evidence(), &merge());
    assert_eq!(proof.operation().operation_id, MERGE_ID);
    assert_eq!(proof.operation().parent_ids, [LEFT_ID, RIGHT_ID]);
    assert_eq!(proof.operation().view_id, RICH_ID);
    assert_eq!(
        proof.operation().workspace_name.as_deref(),
        Some("operation-workspace")
    );
    assert_eq!(proof.view().view_id, RICH_ID);
    assert_eq!(proof.view().head_ids, ["aa".repeat(20), "bb".repeat(20)]);
    assert_eq!(proof.view().commit_references, 15);
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 4).unwrap(), before_pending);
    assert!(before_status.applied_heads.is_empty());
    assert_eq!(snapshot(fixture.repo.path()), before_files);
}

#[test]
fn jj_evidence_rejects_opaque_false_parent_envelope_that_journal_can_store() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut false_parent = merge();
    false_parent.parent_ids = vec!["00".repeat(64)];
    journal
        .capture(&fixture.batch(MERGE_ID, vec![false_parent]))
        .unwrap();
    let known = journal
        .lookup_observed(&fixture.source, &[MERGE_ID.to_owned()])
        .unwrap();
    rejected(&known.operations[MERGE_ID], "parent");
    assert_eq!(journal.status(&fixture.source).unwrap(), known.status);
}

#[test]
fn jj_evidence_rejects_swapped_independently_valid_view_after_opaque_capture() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut swapped = first();
    swapped.view_id = RICH_ID.to_owned();
    swapped.view_bytes = unhex(RICH_HEX);
    journal
        .capture(&fixture.batch(FIRST_ID, vec![swapped]))
        .unwrap();
    let known = journal
        .lookup_observed(&fixture.source, &[FIRST_ID.to_owned()])
        .unwrap();
    rejected(&known.operations[FIRST_ID], "view");
    assert_eq!(journal.status(&fixture.source).unwrap(), known.status);
}

#[test]
fn jj_evidence_exact_parent_order_survives_journal_graph_normalization() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut reversed = merge();
    reversed.parent_ids.reverse();
    journal
        .capture(&fixture.batch(MERGE_ID, vec![reversed, left(), right(), first()]))
        .unwrap();
    let known = journal
        .lookup_observed(&fixture.source, &[MERGE_ID.to_owned()])
        .unwrap();
    assert_eq!(known.operations[MERGE_ID].parent_ids, [RIGHT_ID, LEFT_ID]);
    rejected(&known.operations[MERGE_ID], "parent");
    verify(&merge()).unwrap();
}

#[test]
fn jj_evidence_stored_checksum_does_not_replace_native_operation_hash() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut opaque = first();
    opaque.operation_bytes = unhex(LEFT_HEX);
    journal
        .capture(&fixture.batch(FIRST_ID, vec![opaque]))
        .unwrap();
    let known = journal
        .lookup_observed(&fixture.source, &[FIRST_ID.to_owned()])
        .unwrap();
    rejected(&known.operations[FIRST_ID], "hash");
}

#[test]
fn jj_evidence_requires_native_hashes_for_both_supplied_byte_strings() {
    let mut wrong_operation = first();
    wrong_operation.operation_bytes = unhex(LEFT_HEX);
    rejected(&wrong_operation, "hash");
    let mut wrong_view = first();
    wrong_view.view_bytes = unhex(RICH_HEX);
    rejected(&wrong_view, "hash");
    let mut wrong_identity = merge();
    wrong_identity.operation_id = LEFT_ID.to_owned();
    rejected(&wrong_identity, "hash");
}

#[test]
fn jj_evidence_rejects_corrupted_wire_bytes_without_returning_partial_proof() {
    for bytes in [vec![], vec![0], vec![0x80; 11]] {
        let mut evidence = first();
        evidence.operation_bytes = bytes.clone();
        rejected(&evidence, "operation");
        let mut evidence = first();
        evidence.view_bytes = bytes;
        rejected(&evidence, "view");
    }
}

#[test]
fn jj_evidence_preserves_semantically_equivalent_wire_bytes_by_borrowing_input() {
    let mut evidence = first();
    let prefix = bytes_field(1, &unhex(MINIMAL_ID));
    assert!(evidence.operation_bytes.starts_with(&prefix));
    evidence.operation_bytes = [evidence.operation_bytes[prefix.len()..].to_vec(), prefix].concat();
    evidence.view_bytes = [scalar(12, 1), bytes_field(1, &[0xaa; 20])].concat();
    let proof = verify(&evidence).unwrap();
    assert!(std::ptr::eq(proof.evidence(), &evidence));
    assert_eq!(proof.operation().operation_id, FIRST_ID);
    assert_eq!(proof.view().view_id, MINIMAL_ID);
    assert_ne!(proof.evidence().operation_bytes, unhex(FIRST_HEX));
    assert_ne!(proof.evidence().view_bytes, unhex(MINIMAL_HEX));
}

#[test]
fn jj_evidence_validates_profile_and_exposes_standard_error_trait() {
    let evidence = first();
    for profile in [
        "",
        "jj-simple-op-store/0.45.0",
        "jj-simple-op-store/0.45.1 ",
    ] {
        let error = match verify_evidence(profile, &evidence) {
            Ok(_) => panic!("unsupported profile accepted"),
            Err(error) => error,
        };
        let standard: &dyn std::error::Error = &error;
        assert!(standard.to_string().contains("profile"));
    }
}

#[test]
fn jj_evidence_reuses_envelope_identity_parent_and_root_validation() {
    for id in ["abc".to_owned(), FIRST_ID.to_uppercase(), "g".repeat(128)] {
        let mut evidence = first();
        evidence.operation_id = id.clone();
        rejected(&evidence, "identity");
        let mut evidence = first();
        evidence.view_id = id.clone();
        rejected(&evidence, "identity");
        let mut evidence = first();
        evidence.parent_ids = vec![id];
        rejected(&evidence, "identity");
    }
    let mut evidence = first();
    evidence.operation_id = "00".repeat(64);
    rejected(&evidence, "root");
    let mut evidence = first();
    evidence.parent_ids.clear();
    rejected(&evidence, "parent");
    let mut evidence = first();
    evidence.parent_ids = vec!["00".repeat(64); 2];
    rejected(&evidence, "duplicate");
}

#[test]
fn jj_evidence_applies_combined_envelope_byte_limit_before_native_decoding() {
    let limit = MAX_JJ_OBSERVATION_OPERATION_BYTES;
    let mut single = first();
    single.operation_bytes = vec![0; limit + 1];
    single.view_bytes.clear();
    rejected(&single, "byte limit");
    let mut combined = first();
    combined.operation_bytes = vec![0; limit / 2];
    combined.view_bytes = vec![0; limit / 2 + 1];
    rejected(&combined, "byte limit");
    let mut exact = first();
    exact.view_bytes = vec![0; limit - exact.operation_bytes.len()];
    let error = match verify(&exact) {
        Ok(_) => panic!("malformed view accepted"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("view"));
    assert!(!error.contains("byte limit"));
}

#[test]
fn jj_evidence_single_join_does_not_require_ancestry_repository_or_admission() {
    let fixture = Fixture::new();
    let journal = fixture.open();
    let evidence = paired(
        DETACHED_ID,
        DETACHED_HEX,
        &[&"ee".repeat(64)],
        MINIMAL_ID,
        MINIMAL_HEX,
    );
    let proof = verify(&evidence).unwrap();
    assert_eq!(proof.operation().parent_ids, ["ee".repeat(64)]);
    assert!(
        journal
            .lookup_observed(&fixture.source, &[DETACHED_ID.to_owned()])
            .unwrap()
            .operations
            .is_empty()
    );
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 0);
    assert!(status.observed_heads.is_empty());
    assert!(status.applied_heads.is_empty());
}

#[test]
fn jj_evidence_preserves_unrecorded_predecessors_as_a_later_reader_gate() {
    let unrecorded = paired(
        UNRECORDED_ID,
        UNRECORDED_HEX,
        &[&"00".repeat(64)],
        MINIMAL_ID,
        MINIMAL_HEX,
    );
    assert_eq!(
        verify(&unrecorded).unwrap().operation().commit_predecessors,
        None
    );
    assert!(
        verify(&first())
            .unwrap()
            .operation()
            .commit_predecessors
            .as_ref()
            .unwrap()
            .is_empty()
    );
}
