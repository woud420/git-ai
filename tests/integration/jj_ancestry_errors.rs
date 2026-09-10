use super::*;

#[test]
fn jj_ancestry_rejects_declared_scope_mismatch_before_decoding_evidence() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let mut malformed = left();
    malformed.operation_bytes = vec![0];
    let references = [&malformed];
    let heads = vec![LEFT_ID.to_owned()];
    let wrong_source = "ab".repeat(32);
    let wrong_baseline = "cd".repeat(32);
    for case in 0..7 {
        let mut request = input(&baseline, &heads, &references);
        let category = match case {
            0 => {
                request.source_id = &wrong_source;
                "source"
            }
            1 => {
                request.reader_profile = "jj-simple-op-store/0.45.0";
                "profile"
            }
            2 => {
                request.baseline_id = &wrong_baseline;
                "baseline"
            }
            3 => {
                request.expected_native_generation = 0;
                "generation"
            }
            4 => {
                request.expected_native_generation = 2;
                "generation"
            }
            5 => {
                request.expected_native_generation = u64::MAX;
                "generation"
            }
            6 => {
                request.source_id = "";
                "source"
            }
            _ => unreachable!(),
        };
        rejected(&fixture, &baseline, request, Some(category));
    }
}

#[test]
fn jj_ancestry_rejects_empty_duplicate_root_and_malformed_head_ids() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let record = left();
    let references = [&record];
    for heads in [
        vec![],
        vec![LEFT_ID.to_owned(), LEFT_ID.to_owned()],
        vec!["00".repeat(64)],
        vec![LEFT_ID.to_uppercase()],
        vec!["g".repeat(128)],
        vec!["abc".to_owned()],
    ] {
        rejected(
            &fixture,
            &baseline,
            input(&baseline, &heads, &references),
            None,
        );
    }
}

#[test]
fn jj_ancestry_rejects_duplicate_records_before_native_decode_or_map_selection() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let heads = vec![LEFT_ID.to_owned()];
    for conflicting in [false, true] {
        let first_record = left();
        let mut second = first_record.clone();
        if conflicting {
            second.operation_bytes = vec![0];
        }
        let references = [&first_record, &second];
        rejected(
            &fixture,
            &baseline,
            input(&baseline, &heads, &references),
            Some("duplicate"),
        );
    }
}

#[test]
fn jj_ancestry_rejects_resupplied_boundary_evidence_even_when_hash_or_bytes_differ() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[left()]);
    let heads = vec![LEFT_ID.to_owned()];
    for corrupt in [false, true] {
        let mut record = left();
        if corrupt {
            record.operation_bytes = vec![0];
        }
        let references = [&record];
        rejected(
            &fixture,
            &baseline,
            input(&baseline, &heads, &references),
            Some("boundary"),
        );
    }
}

#[test]
fn jj_ancestry_missing_head_and_every_missing_merge_parent_prevent_partial_proofs() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let records = [left(), right(), merge()];
    let heads = vec![MERGE_ID.to_owned()];
    for excluded in 0..3 {
        let references = records
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != excluded)
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        rejected(
            &fixture,
            &baseline,
            input(&baseline, &heads, &references),
            Some("missing"),
        );
    }
    let all = records.iter().collect::<Vec<_>>();
    assert_eq!(
        checked(&fixture, &baseline, input(&baseline, &heads, &all))
            .unwrap()
            .ordered_operations()
            .len(),
        3
    );
}

#[test]
fn jj_ancestry_rejects_detached_inputs_including_records_below_a_stopped_anchor() {
    for below_cutoff in [false, true] {
        let fixture = Fixture::new();
        let baseline = durable(&fixture, &[if below_cutoff { left() } else { first() }]);
        let records = if below_cutoff {
            vec![first()]
        } else {
            vec![left(), right()]
        };
        let references = records.iter().collect::<Vec<_>>();
        let heads = vec![LEFT_ID.to_owned()];
        rejected(
            &fixture,
            &baseline,
            input(&baseline, &heads, &references),
            Some("detached"),
        );
    }
}

#[test]
fn jj_ancestry_reverifies_native_hashes_view_join_and_exact_parent_order() {
    let fixture = Fixture::new();
    let baseline = durable(&fixture, &[first()]);
    let heads = vec![MERGE_ID.to_owned()];
    for case in 0..6 {
        let mut records = [left(), right(), merge()];
        match case {
            0 => records[2].operation_bytes = vec![0],
            1 => records[2].operation_bytes = first().operation_bytes,
            2 => records[2].view_bytes = vec![0],
            3 => records[2].view_bytes = first().view_bytes,
            4 => records[2].parent_ids.reverse(),
            5 => {
                records[2].view_id = MINIMAL_ID.to_owned();
                records[2].view_bytes = first().view_bytes;
            }
            _ => unreachable!(),
        }
        let references = records.iter().collect::<Vec<_>>();
        rejected(
            &fixture,
            &baseline,
            input(&baseline, &heads, &references),
            None,
        );
    }
}
