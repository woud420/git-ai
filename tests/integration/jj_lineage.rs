use crate::jj_evidence::{support::*, vectors::*};
use git_ai::model::jj_observation::JJ_OBSERVATION_READER_PROFILE as PROFILE;
use git_ai::operations::jj::lineage::verify_transition;

#[path = "jj_lineage_real.rs"]
mod real;

#[test]
fn jj_lineage_requires_the_exact_native_parent_set() {
    let child = merge();
    let left = left();
    let right = right();
    let first = first();
    for parents in [vec![], vec![&left], vec![&left, &first], vec![&left, &left]] {
        assert!(verify_transition(PROFILE, &child, &parents).is_err());
    }
    let proof = verify_transition(PROFILE, &child, &[&right, &left]).unwrap();
    assert_eq!(proof.operation().parent_ids, [LEFT_ID, RIGHT_ID]);
}

#[test]
fn jj_lineage_preserves_recorded_candidates_without_asserting_rewrite_provenance() {
    let child = merge();
    let left = left();
    let right = right();
    let proof = verify_transition(PROFILE, &child, &[&left, &right]).unwrap();
    assert_eq!(
        proof.recorded_predecessors().unwrap()[&"aa".repeat(20)],
        ["bb".repeat(20)]
    );
    assert!(!proof.workspaces().is_empty());
    assert!(
        proof
            .workspaces()
            .iter()
            .all(|movement| { movement.before == [None, None] && movement.after.is_some() })
    );
}

#[test]
fn jj_lineage_keeps_empty_and_unrecorded_predecessor_evidence_distinct() {
    let child = first();
    let proof = verify_transition(PROFILE, &child, &[]).unwrap();
    assert!(proof.recorded_predecessors().unwrap().is_empty());
    assert!(proof.workspaces().is_empty());
    let child = paired(
        UNRECORDED_ID,
        UNRECORDED_HEX,
        &[&"00".repeat(64)],
        crate::jj_view::vectors::MINIMAL_ID,
        crate::jj_view::vectors::MINIMAL_HEX,
    );
    assert!(
        verify_transition(PROFILE, &child, &[])
            .unwrap()
            .recorded_predecessors()
            .is_none()
    );
}

#[test]
fn jj_lineage_rejects_corrupt_parent_evidence_and_unknown_profiles() {
    let child = left();
    let mut parent = first();
    parent.view_bytes[0] ^= 1;
    assert!(verify_transition(PROFILE, &child, &[&parent]).is_err());
    assert!(verify_transition("unknown", &first(), &[]).is_err());
}

#[test]
fn jj_lineage_bounds_parent_count_and_raw_bytes_before_decoding() {
    let child = first();
    let parents = vec![&child; 33];
    let error = verify_transition(PROFILE, &child, &parents).err().unwrap();
    assert!(error.to_string().contains("parent count limit"));
    let mut oversized = first();
    oversized.operation_bytes = vec![0; 8 * 1024 * 1024 + 1];
    let error = verify_transition(PROFILE, &oversized, &[]).err().unwrap();
    assert!(error.to_string().contains("evidence byte limit"));
}
