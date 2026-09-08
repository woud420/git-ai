use super::*;
use ciborium::value::Value;

#[test]
fn native_admission_model_independent_packet_and_state_vectors_match_the_closed_codec() {
    assert_eq!(vectors::RECORDS.len(), 36);
    assert_eq!(
        vectors::RECORDS
            .iter()
            .filter(|item| item.model_valid)
            .count(),
        18
    );
    for item in vectors::RECORDS {
        assert_eq!(
            checksum(item.raw),
            item.checksum,
            "{} fixture checksum",
            item.label
        );
        let accepted = if item.kind == "packet" {
            admission_codec::decode_packet(
                item.raw,
                vectors::SOURCE,
                item.admission_id,
                item.generation,
            )
            .is_ok()
        } else {
            admission_codec::decode_state(
                item.raw,
                vectors::SOURCE,
                item.admission_id,
                item.checksum,
            )
            .is_ok()
        };
        assert_eq!(accepted, item.model_valid, "{}", item.label);
    }
}

#[test]
fn native_admission_model_prepared_identity_matches_independent_byte_string_encoding() {
    for label in [
        "left",
        "merge",
        "return_left",
        "baseline_only",
        "two_heads",
        "root_closed",
        "large_bytes",
        "wrong_native",
        "wrong_native_order",
        "wrong_native_parent",
    ] {
        let input = Input::from(label);
        assert_eq!(
            input.bytes(),
            vector(label).raw,
            "independent runtime writer {label}"
        );
        assert_eq!(
            input.prepare().unwrap().admission_id(),
            vector(label).admission_id,
            "{label}"
        );
    }
    assert!(
        Input::from("large_bytes").operations[0]
            .operation_bytes
            .len()
            > 4096
    );
}

#[test]
fn native_admission_model_normalizes_sets_only_and_preserves_parent_and_operation_order() {
    let mut input = Input::from("two_heads");
    input.heads.reverse();
    assert_eq!(
        input.prepare().unwrap().admission_id(),
        vector("two_heads").admission_id
    );
    let unsorted = input.bytes();
    assert!(
        admission_codec::decode_packet(&unsorted, vectors::SOURCE, &checksum(&unsorted), 1)
            .is_err()
    );
    let mut expected = Input::from("two_heads");
    expected.generation = 1;
    expected.expected = expected.heads.clone();
    let canonical_id = expected.prepare().unwrap().admission_id().to_owned();
    expected.expected.reverse();
    assert_eq!(expected.prepare().unwrap().admission_id(), canonical_id);
    let bytes = expected.bytes();
    assert!(admission_codec::decode_packet(&bytes, vectors::SOURCE, &checksum(&bytes), 2).is_err());
    let state_vector = vector("two_heads_state");
    let mut state = value(state_vector.raw);
    field_mut(&mut state, "admitted_head_ids")
        .as_array_mut()
        .unwrap()
        .reverse();
    let bytes = encode(&state);
    assert!(
        admission_codec::decode_state(
            &bytes,
            vectors::SOURCE,
            state_vector.admission_id,
            &checksum(&bytes)
        )
        .is_err()
    );
    let input = Input::from("merge");
    let mut reversed = Input::from("merge");
    reversed.operations.last_mut().unwrap().parent_ids.reverse();
    assert_ne!(
        input.prepare().unwrap().admission_id(),
        reversed.prepare().unwrap().admission_id()
    );
    assert_eq!(
        Input::from("wrong_native_order")
            .prepare()
            .unwrap()
            .admission_id(),
        vector("wrong_native_order").admission_id
    );
}

#[test]
fn native_admission_model_structural_success_does_not_certify_native_bytes_or_envelope() {
    for label in ["wrong_native", "wrong_native_parent"] {
        let item = vector(label);
        let decoded = admission_codec::decode_packet(
            item.raw,
            vectors::SOURCE,
            item.admission_id,
            item.generation,
        )
        .unwrap();
        assert!(
            crate::operations::jj::evidence::verify_evidence(
                vectors::PROFILE,
                &decoded.record.operations[0]
            )
            .is_err()
        );
    }
    let wrong_order = vector("wrong_native_order");
    let decoded = admission_codec::decode_packet(
        wrong_order.raw,
        vectors::SOURCE,
        wrong_order.admission_id,
        2,
    )
    .unwrap();
    for record in &decoded.record.operations {
        crate::operations::jj::evidence::verify_evidence(vectors::PROFILE, record).unwrap();
    }
    assert_eq!(
        decoded.record.operations[0].operation_id,
        Input::from("merge").heads[0]
    );
}

#[test]
fn native_admission_model_encoded_limit_is_distinct_from_valid_native_and_raw_limits() {
    let fixture = Fixture::new();
    let baseline = crate::operations::jj::baseline_persistence::reopen_current_state_baseline(
        &fixture.journal(),
        vectors::SOURCE,
        &mut unlimited(),
    )
    .unwrap()
    .unwrap();
    for over in [false, true] {
        let input = encoded_boundary(over);
        let raw: usize = input
            .operations
            .iter()
            .map(|item| item.operation_bytes.len() + item.view_bytes.len())
            .sum();
        assert_eq!(raw, boundary::RAW_BYTES + usize::from(over));
        assert!(raw < 8 * 1024 * 1024);
        let refs: Vec<_> = input.operations.iter().collect();
        let proof = crate::operations::jj::ancestry::verify_ancestry_to_baseline(
            &baseline,
            crate::operations::jj::ancestry::JjAncestryInput {
                source_id: vectors::SOURCE,
                reader_profile: vectors::PROFILE,
                baseline_id: vectors::BASELINE,
                expected_native_generation: 1,
                head_ids: &input.heads,
                operations: &refs,
            },
        )
        .unwrap();
        assert_eq!(proof.ordered_operations().len(), 256);
        drop(proof);
        let bytes = input.bytes();
        assert_eq!(bytes.len(), 8 * 1024 * 1024 + usize::from(over));
        let expected = if over {
            boundary::OVER_ADMISSION_ID
        } else {
            boundary::EXACT_ADMISSION_ID
        };
        assert_eq!(checksum(&bytes), expected);
        let prepared = input.prepare();
        assert_eq!(prepared.is_ok(), !over);
        if let Ok(prepared) = prepared {
            assert_eq!(prepared.admission_id(), expected);
        }
        assert_eq!(
            admission_codec::decode_packet(&bytes, vectors::SOURCE, expected, 1).is_ok(),
            !over
        );
    }
}

#[test]
fn native_admission_model_preflight_rejects_invalid_counts_generations_and_scope() {
    let mutations: [fn(&mut Input); 11] = [
        |input: &mut Input| input.source.clear(),
        |input: &mut Input| input.profile.push('x'),
        |input: &mut Input| input.receipt.make_ascii_uppercase(),
        |input: &mut Input| input.baseline_generation = 2,
        |input: &mut Input| input.generation = i64::MAX as u64,
        |input: &mut Input| input.expected.clear(),
        |input: &mut Input| input.heads.clear(),
        |input: &mut Input| input.heads.push(input.heads[0].clone()),
        |input: &mut Input| input.operations[0].parent_ids.clear(),
        |input: &mut Input| input.operations[0].operation_bytes.clear(),
        |input: &mut Input| input.operations[0].view_bytes.clear(),
    ];
    for mutate in mutations {
        let mut input = Input::from("left");
        mutate(&mut input);
        assert!(input.prepare().is_err());
    }
    let mut max_generation = Input::from("left");
    max_generation.generation = i64::MAX as u64 - 1;
    assert!(max_generation.prepare().is_ok());
    for count in [32, 33] {
        let mut heads = Input::from("left");
        heads.heads = (1..=count).map(|index| format!("{index:0128x}")).collect();
        assert_eq!(heads.prepare().is_ok(), count == 32);
        let mut parents = Input::from("left");
        parents.operations[0].parent_ids =
            (1..=count).map(|index| format!("{index:0128x}")).collect();
        assert_eq!(parents.prepare().is_ok(), count == 32);
    }
}

#[test]
fn native_admission_model_count_and_raw_caps_apply_before_native_semantics() {
    let mut input = Input::from("left");
    let view_bytes = input.operations[0].view_bytes.len();
    input.operations[0].operation_bytes = vec![0xa5; 1024 * 1024 - view_bytes];
    assert!(input.prepare().is_ok());
    input.operations[0].operation_bytes.push(0xa5);
    assert!(input.prepare().is_err());
    let mut count = Input::from("left");
    count.operations = (1..=256)
        .map(|index| {
            let mut item = Input::from("left").operations.pop().unwrap();
            item.operation_id = format!("{index:0128x}");
            item
        })
        .collect();
    assert!(count.prepare().is_ok());
    let mut extra = Input::from("left").operations.pop().unwrap();
    extra.operation_id = format!("{:0128x}", 257);
    count.operations.push(extra);
    assert!(count.prepare().is_err());
}

#[test]
fn native_admission_model_state_is_closed_canonical_and_exactly_scoped() {
    let item = vector("left_state");
    for (name, replacement) in [
        ("state_version", Value::Integer(2.into())),
        ("domain", Value::Text("wrong".to_owned())),
        ("source_id", Value::Text("02".repeat(32))),
        ("baseline_generation", Value::Integer(2.into())),
        ("generation", Value::Integer(0.into())),
        ("generation", Value::Integer((i64::MAX as u64 + 1).into())),
        ("admitted_head_ids", Value::Array(vec![])),
    ] {
        let mut state = value(item.raw);
        *field_mut(&mut state, name) = replacement;
        let bytes = encode(&state);
        assert!(
            admission_codec::decode_state(
                &bytes,
                vectors::SOURCE,
                item.admission_id,
                &checksum(&bytes)
            )
            .is_err(),
            "{name}"
        );
    }
    let mut state = value(item.raw);
    state.as_map_mut().unwrap().reverse();
    let bytes = encode(&state);
    assert!(
        admission_codec::decode_state(
            &bytes,
            vectors::SOURCE,
            item.admission_id,
            &checksum(&bytes)
        )
        .is_err()
    );
    assert!(
        admission_codec::decode_state(item.raw, vectors::SOURCE, &"cd".repeat(32), item.checksum)
            .is_err()
    );
    assert!(
        admission_codec::decode_state(
            item.raw,
            vectors::SOURCE,
            item.admission_id,
            &"cd".repeat(32)
        )
        .is_err()
    );
    let item = vector("left_state");
    let mut state = value(item.raw);
    *field_mut(&mut state, "generation") = Value::Integer((i64::MAX as u64).into());
    let bytes = encode(&state);
    assert!(
        admission_codec::decode_state(
            &bytes,
            vectors::SOURCE,
            item.admission_id,
            &checksum(&bytes)
        )
        .is_ok()
    );
}

#[test]
fn native_admission_model_both_raw_fields_require_nonempty_cbor_byte_strings() {
    for key in ["operation_bytes", "view_bytes"] {
        for representation in [0, 1, 2] {
            let mut packet = value(vector("left").raw);
            let record = &mut field_mut(&mut packet, "operations").as_array_mut().unwrap()[0];
            let raw = field(record, key).as_bytes().unwrap();
            let replacement = match representation {
                0 => Value::Array(
                    raw.iter()
                        .map(|byte| Value::Integer((*byte).into()))
                        .collect(),
                ),
                1 => Value::Text("opaque".to_owned()),
                _ => Value::Bytes(Vec::new()),
            };
            *field_mut(record, key) = replacement;
            let bytes = encode(&packet);
            assert!(
                admission_codec::decode_packet(&bytes, vectors::SOURCE, &checksum(&bytes), 1)
                    .is_err(),
                "{key}:{representation}"
            );
        }
    }
}
