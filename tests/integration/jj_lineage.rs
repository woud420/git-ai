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
fn jj_lineage_preserves_distinct_parent_values_and_added_removed_workspaces() {
    mod ordered {
        include!("fixtures/jj-lineage/vectors.rs");
    }
    use git_ai::operations::jj::lineage::JjWorkspaceTransition;
    use ordered::*;
    let root = "00".repeat(64);
    let left = paired(LEFT_ID, LEFT_HEX, &[&root], LEFT_VIEW_ID, LEFT_VIEW_HEX);
    let right = paired(RIGHT_ID, RIGHT_HEX, &[&root], RIGHT_VIEW_ID, RIGHT_VIEW_HEX);
    let current = paired(
        MERGE_ID,
        MERGE_HEX,
        &[RIGHT_ID, LEFT_ID],
        MERGE_VIEW_ID,
        MERGE_VIEW_HEX,
    );
    // Supply the opposite order: only the immutable operation determines order.
    let proof = verify_transition(PROFILE, &current, &[&left, &right]).unwrap();
    let expected = [
        ("added", vec![None, None], Some("66".repeat(20))),
        (
            "default",
            vec![Some("22".repeat(20)), Some("11".repeat(20))],
            Some("33".repeat(20)),
        ),
        ("removed", vec![None, Some("44".repeat(20))], None),
        ("right-only", vec![Some("55".repeat(20)), None], None),
    ]
    .map(|(name, before, after)| JjWorkspaceTransition {
        workspace_name: name.to_owned(),
        before,
        after,
    });
    assert_eq!(proof.workspaces(), expected);
    assert!(proof.recorded_predecessors().unwrap().is_empty());
    let root_transition = verify_transition(PROFILE, &left, &[]).unwrap();
    assert!(
        root_transition
            .workspaces()
            .iter()
            .all(|transition| transition.before == [None])
    );
    let mut forged = current.clone();
    forged.parent_ids.reverse();
    assert!(verify_transition(PROFILE, &forged, &[&left, &right]).is_err());
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
