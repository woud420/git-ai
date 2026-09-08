use super::*;
use git_ai::model::jj_observation::{
    MAX_JJ_OBSERVATION_BATCH_BYTES, MAX_JJ_OBSERVATION_OPERATION_BYTES,
    MAX_JJ_OBSERVATION_OPERATIONS,
};
use git_ai::operations::jj::baseline::MAX_JJ_BASELINE_HEADS;
use git_ai::operations::jj::evidence::verify_evidence;

#[path = "jj_ancestry_bounds_support.rs"]
mod construction;
#[path = "jj_ancestry_vectors.rs"]
mod vectors;
use construction::*;
use vectors::*;

#[test]
fn jj_ancestry_exact_and_one_over_head_count_use_valid_native_graphs() {
    assert_eq!(MAX_JJ_BASELINE_HEADS, 32);
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    for count in [32, 33] {
        let records = chain(count);
        verify_each(&records);
        let references = records.iter().collect::<Vec<_>>();
        let heads = records
            .iter()
            .map(|record| record.operation_id.clone())
            .collect::<Vec<_>>();
        if count == 32 {
            let proof =
                checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
            assert_eq!(proof.ordered_operations().len(), count);
        } else {
            rejected(
                &fixture,
                &baseline,
                input(&baseline, &heads, &references),
                Some("limit"),
            );
        }
    }
}

#[test]
fn jj_ancestry_exact_256_node_depth_is_iterative_and_257_nodes_reject() {
    assert_eq!(MAX_JJ_OBSERVATION_OPERATIONS, 256);
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    for count in [256, 257] {
        let records = chain(count);
        verify_each(&records);
        let references = records.iter().rev().collect::<Vec<_>>();
        let heads = vec![records.last().unwrap().operation_id.clone()];
        if count == 256 {
            let proof =
                checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
            assert_eq!(ordered_ids(&proof), CHAIN_IDS[..count]);
            assert_eq!(proof.reached_baseline_ids(), [FIRST_ID]);
            assert!(!proof.reaches_root());
        } else {
            rejected(
                &fixture,
                &baseline,
                input(&baseline, &heads, &references),
                Some("limit"),
            );
        }
    }
}

#[test]
fn jj_ancestry_envelope_raw_limits_preflight_all_records_before_native_decode() {
    assert_eq!(MAX_JJ_OBSERVATION_OPERATION_BYTES, 1024 * 1024);
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let heads = vec![RIGHT_ID.to_owned()];
    let mut early_invalid = left();
    early_invalid.operation_bytes = vec![0];
    let mut too_large = right();
    too_large.operation_bytes =
        vec![0; MAX_JJ_OBSERVATION_OPERATION_BYTES + 1 - too_large.view_bytes.len()];
    let references = [&early_invalid, &too_large];
    rejected(
        &fixture,
        &baseline,
        input(&baseline, &heads, &references),
        Some("limit"),
    );

    too_large.operation_bytes.pop();
    assert_eq!(
        raw_size(&[too_large.clone()]),
        MAX_JJ_OBSERVATION_OPERATION_BYTES
    );
    let native_error = match verify_evidence(JJ_OBSERVATION_READER_PROFILE, &too_large) {
        Ok(_) => panic!("zero-filled fixture unexpectedly decoded"),
        Err(error) => error,
    };
    let references = [&too_large];
    let error = rejected(
        &fixture,
        &baseline,
        input(&baseline, &heads, &references),
        None,
    );
    assert!(
        error.to_string().contains(&native_error.to_string()),
        "exact envelope should reach malformed native wire: {error}"
    );
}

#[test]
fn jj_ancestry_exact_and_one_over_aggregate_raw_bytes_use_native_valid_chains() {
    assert_eq!(MAX_JJ_OBSERVATION_BATCH_BYTES, 8 * 1024 * 1024);
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    for over in [false, true] {
        let records = raw_chain(over);
        assert_eq!(records.len(), 256);
        assert_eq!(
            raw_size(&records),
            MAX_JJ_OBSERVATION_BATCH_BYTES + usize::from(over)
        );
        verify_each(&records);
        let references = records.iter().collect::<Vec<_>>();
        let heads = vec![records.last().unwrap().operation_id.clone()];
        if over {
            rejected(
                &fixture,
                &baseline,
                input(&baseline, &heads, &references),
                Some("limit"),
            );
        } else {
            let proof =
                checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();
            assert_eq!(ordered_ids(&proof), RAW_CHAIN_IDS);
        }
    }
}

#[test]
fn jj_ancestry_aggregate_predecessor_limit_counts_keys_and_repeated_edges() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    for over in [false, true] {
        let records = predecessor_records(over);
        let semantic = records
            .iter()
            .map(|record| {
                let proof = verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
                proof
                    .operation()
                    .commit_predecessors
                    .as_ref()
                    .unwrap()
                    .values()
                    .map(|edges| 1 + edges.len())
                    .sum::<usize>()
            })
            .sum::<usize>();
        assert_eq!(semantic, 4096 + usize::from(over));
        let references = records.iter().collect::<Vec<_>>();
        let heads = vec![records.last().unwrap().operation_id.clone()];
        if over {
            rejected(
                &fixture,
                &baseline,
                input(&baseline, &heads, &references),
                Some("limit"),
            );
        } else {
            assert_eq!(
                checked(&fixture, &baseline, input(&baseline, &heads, &references))
                    .unwrap()
                    .ordered_operations()
                    .len(),
                2
            );
        }
    }
}

#[test]
fn jj_ancestry_aggregate_view_limit_charges_each_shared_view_occurrence() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    for over in [false, true] {
        let records = view_records(over);
        let references_count = records
            .iter()
            .map(|record| {
                verify_evidence(JJ_OBSERVATION_READER_PROFILE, record)
                    .unwrap()
                    .view()
                    .commit_references
            })
            .sum::<usize>();
        assert_eq!(references_count, 4096 + usize::from(over));
        assert_eq!(records[0].view_id, records[1].view_id);
        let references = records.iter().collect::<Vec<_>>();
        let heads = vec![records.last().unwrap().operation_id.clone()];
        if over {
            rejected(
                &fixture,
                &baseline,
                input(&baseline, &heads, &references),
                Some("limit"),
            );
        } else {
            assert_eq!(
                checked(&fixture, &baseline, input(&baseline, &heads, &references))
                    .unwrap()
                    .ordered_operations()
                    .len(),
                2
            );
        }
    }
}
