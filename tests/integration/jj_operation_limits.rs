use super::*;

#[test]
fn jj_operation_limits_are_explicit_and_match_qualified_profile() {
    assert_eq!(MAX_OPERATION_BYTES, 2 * 1024 * 1024);
    assert_eq!(MAX_OPERATION_PARENTS, 32);
    assert_eq!(MAX_OPERATION_PREDECESSOR_REFERENCES, 4096);
    assert_eq!(MAX_OPERATION_ATTRIBUTES, 256);
    assert_eq!(MAX_OPERATION_METADATA_BYTES, 64 * 1024);
}

#[test]
fn jj_operation_rejects_oversized_input_before_parsing() {
    reject(&vec![0; MAX_OPERATION_BYTES + 1], "limit");
}

#[test]
fn jj_operation_parent_limit_is_inclusive() {
    let mut parents: Vec<_> = (0..MAX_OPERATION_PARENTS)
        .map(|i| numbered_id(i, 64))
        .collect();
    let raw = operation(None, &parents, &[], None);
    let decoded = decode_operation(JJ_OBSERVATION_READER_PROFILE, PARENTS_LIMIT_ID, &raw).unwrap();
    assert_eq!(decoded.parent_ids.len(), MAX_OPERATION_PARENTS);
    parents.push(numbered_id(MAX_OPERATION_PARENTS, 64));
    reject(&operation(None, &parents, &[], None), "limit");
}

#[test]
fn jj_operation_predecessor_entry_limit_is_inclusive() {
    let mut entries: Vec<_> = (0..MAX_OPERATION_PREDECESSOR_REFERENCES)
        .map(|i| predecessor(&numbered_id(i, 20), &[]))
        .collect();
    let raw = operation(None, &[vec![0x22; 64]], &entries, Some(1));
    let decoded = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        PREDECESSOR_ENTRIES_LIMIT_ID,
        &raw,
    )
    .unwrap();
    assert_eq!(
        decoded.commit_predecessors.unwrap().len(),
        MAX_OPERATION_PREDECESSOR_REFERENCES
    );
    entries.push(predecessor(
        &numbered_id(MAX_OPERATION_PREDECESSOR_REFERENCES, 20),
        &[],
    ));
    reject(
        &operation(None, &[vec![0x22; 64]], &entries, Some(1)),
        "limit",
    );
}

#[test]
fn jj_operation_predecessor_budget_includes_map_key_and_every_edge() {
    let mut edges: Vec<_> = (0..MAX_OPERATION_PREDECESSOR_REFERENCES - 1)
        .map(|i| numbered_id(i, 20))
        .collect();
    let raw = operation(
        None,
        &[vec![0x22; 64]],
        &[predecessor(&[0xaa; 20], &edges)],
        Some(1),
    );
    let decoded = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        PREDECESSOR_EDGES_LIMIT_ID,
        &raw,
    )
    .unwrap();
    assert_eq!(
        decoded.commit_predecessors.unwrap()[&"aa".repeat(20)].len(),
        MAX_OPERATION_PREDECESSOR_REFERENCES - 1
    );
    edges.push(numbered_id(MAX_OPERATION_PREDECESSOR_REFERENCES, 20));
    reject(
        &operation(
            None,
            &[vec![0x22; 64]],
            &[predecessor(&[0xaa; 20], &edges)],
            Some(1),
        ),
        "limit",
    );
}

#[test]
fn jj_operation_predecessor_budget_is_aggregate_across_map_entries() {
    let half: Vec<_> = (0..MAX_OPERATION_PREDECESSOR_REFERENCES / 2)
        .map(|i| numbered_id(i, 20))
        .collect();
    let entries = [
        predecessor(&[0xaa; 20], &half),
        predecessor(&[0xbb; 20], &half),
    ];
    reject(
        &operation(None, &[vec![0x22; 64]], &entries, Some(1)),
        "limit",
    );
}

#[test]
fn jj_operation_attribute_limit_is_inclusive() {
    let mut metadata: Vec<u8> = (0..MAX_OPERATION_ATTRIBUTES)
        .flat_map(|i| attribute(format!("key-{i:03}").as_bytes(), b"value"))
        .collect();
    decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        ATTRIBUTES_LIMIT_ID,
        &base(Some(&metadata)),
    )
    .unwrap();
    metadata.extend(attribute(b"extra", b"value"));
    reject(&base(Some(&metadata)), "limit");
}

#[test]
fn jj_operation_metadata_byte_limit_is_inclusive() {
    let raw = base(Some(&bytes_field(
        3,
        &vec![b'x'; MAX_OPERATION_METADATA_BYTES],
    )));
    decode_operation(JJ_OBSERVATION_READER_PROFILE, METADATA_BYTES_LIMIT_ID, &raw).unwrap();
    let raw = base(Some(&bytes_field(
        3,
        &vec![b'x'; MAX_OPERATION_METADATA_BYTES + 1],
    )));
    reject(&raw, "limit");
}

#[test]
fn jj_operation_metadata_byte_limit_counts_every_string_and_attribute() {
    let description = bytes_field(3, &vec![b'x'; MAX_OPERATION_METADATA_BYTES]);
    for extra in [
        bytes_field(4, b"x"),
        bytes_field(5, b"x"),
        bytes_field(8, b"x"),
        attribute(b"x", b""),
        attribute(b"", b"x"),
    ] {
        reject(&base(Some(&[description.clone(), extra].concat())), "limit");
    }
    let metadata = [
        bytes_field(3, &vec![b'x'; MAX_OPERATION_METADATA_BYTES - 1]),
        bytes_field(8, "é".as_bytes()),
    ]
    .concat();
    reject(&base(Some(&metadata)), "limit");
}
