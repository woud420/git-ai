use super::*;

#[test]
fn jj_view_rejects_missing_or_duplicate_heads() {
    reject(&view(&[], &[]), "head");
    reject(&view(&[vec![0xaa; 20], vec![0xaa; 20]], &[]), "duplicate");
}

#[test]
fn jj_view_rejects_duplicate_keys_in_every_collection_before_normalization() {
    for entry in [
        bookmark(b"absent", None, &[]),
        bookmark(b"absent", Some(&absent()), &[]),
        named_target(6, b"tag", Some(&normal(0xaa))),
        named_target(3, b"gitref", Some(&normal(0xaa))),
        named_target(13, b"githead", Some(&normal(0xaa))),
        workspace(b"workspace", &[0xaa; 20]),
        remote_view(b"remote", &[], &[]),
    ] {
        reject(&base(&[entry.clone(), entry]), "duplicate");
    }
    let remote = remote_ref(b"ref", &[None], Some(0));
    for entry in [
        remote_view(b"r", &[remote.clone(), remote.clone()], &[]),
        remote_view(b"r", &[], &[remote.clone(), remote]),
        bookmark(
            b"b",
            None,
            &[
                legacy_remote(b"r", None, Some(0)),
                legacy_remote(b"r", None, Some(0)),
            ],
        ),
    ] {
        reject(&base(&[entry]), "duplicate");
    }
}

#[test]
fn jj_view_rejects_duplicate_singular_fields_at_every_nested_level() {
    for extra in [scalar(12, 1), bytes_field(2, b""), bytes_field(7, b"")] {
        let raw = if extra == scalar(12, 1) {
            base(&[extra])
        } else {
            base(&[extra.clone(), extra])
        };
        reject(&raw, "duplicate");
    }
    reject(
        &base(&[
            named_target(13, b"default", Some(&normal(0xaa))),
            bytes_field(9, &normal(0xaa)),
            bytes_field(9, &normal(0xaa)),
        ]),
        "duplicate",
    );
    let repeated_name = [bytes_field(1, b"a"), bytes_field(1, b"b")].concat();
    for tag in [3, 5, 6, 8, 11, 13] {
        reject(&base(&[bytes_field(tag, &repeated_name)]), "duplicate");
    }
    for tag in [5, 6, 13] {
        reject(
            &base(&[bytes_field(
                tag,
                &[
                    bytes_field(1, b"a"),
                    bytes_field(2, &normal(0xaa)),
                    bytes_field(2, &normal(0xbb)),
                ]
                .concat(),
            )]),
            "duplicate",
        );
    }
    for (tag, value_tag, value) in [(3, 3, normal(0xaa)), (8, 2, vec![0xaa; 20])] {
        reject(
            &base(&[bytes_field(
                tag,
                &[
                    bytes_field(1, b"a"),
                    bytes_field(value_tag, &value),
                    bytes_field(value_tag, &value),
                ]
                .concat(),
            )]),
            "duplicate",
        );
    }
    let repeated_target = [normal(0xaa), normal(0xbb)].concat();
    reject(
        &base(&[named_target(6, b"a", Some(&repeated_target))]),
        "duplicate",
    );
    let repeated_term = [bytes_field(1, &[0xaa; 20]), bytes_field(1, &[0xbb; 20])].concat();
    let target = bytes_field(3, &bytes_field(2, &repeated_term));
    reject(&base(&[named_target(6, b"a", Some(&target))]), "duplicate");
    let native_term = [bytes_field(1, b"t"), bytes_field(2, &repeated_term)].concat();
    reject(
        &base(&[remote_view(b"r", &[], &[native_term])]),
        "duplicate",
    );
    for raw in [
        [repeated_name, bytes_field(2, &[])].concat(),
        [remote_ref(b"t", &[None], Some(0)), scalar(3, 1)].concat(),
    ] {
        reject(&base(&[remote_view(b"r", &[], &[raw])]), "duplicate");
    }
    for raw in [
        [legacy_remote(b"r", None, Some(0)), scalar(3, 1)].concat(),
        [
            legacy_remote(b"r", Some(&normal(0xaa)), None),
            bytes_field(2, &normal(0xbb)),
        ]
        .concat(),
        [legacy_remote(b"r", None, None), bytes_field(1, b"other")].concat(),
    ] {
        reject(&base(&[bookmark(b"b", None, &[raw])]), "duplicate");
    }
}

#[test]
fn jj_view_rejects_unknown_fields_and_wrong_wire_types_at_each_level() {
    for bad in [
        bytes_field(4, b""),
        bytes_field(10, b""),
        bytes_field(14, b""),
        scalar(1, 0),
        bytes_field(12, b""),
    ] {
        reject(&base(&[bad]), "field");
    }
    for tag in [3, 5, 6, 8, 11, 13] {
        reject(&base(&[bytes_field(tag, &bytes_field(99, b""))]), "field");
        reject(&base(&[bytes_field(tag, &scalar(1, 0))]), "field");
    }
    for bad in [bytes_field(99, b""), scalar(3, 0)] {
        reject(&base(&[named_target(6, b"t", Some(&bad))]), "field");
    }
    let bad_conflict = bytes_field(3, &bytes_field(99, b""));
    reject(
        &base(&[named_target(6, b"t", Some(&bad_conflict))]),
        "field",
    );
    let bad_term = bytes_field(99, b"");
    let bad_target = bytes_field(3, &bytes_field(2, &bad_term));
    reject(&base(&[named_target(6, b"t", Some(&bad_target))]), "field");
    for bad in [bytes_field(99, b""), scalar(1, 0)] {
        reject(
            &base(&[remote_view(b"r", &[], std::slice::from_ref(&bad))]),
            "field",
        );
        reject(&base(&[bookmark(b"b", None, &[bad])]), "field");
    }
    let native = [bytes_field(1, b"t"), bytes_field(2, &bad_term)].concat();
    reject(&base(&[remote_view(b"r", &[], &[native])]), "field");
}

#[test]
fn jj_view_rejects_invalid_utf8_in_every_name_position() {
    for entry in [
        bookmark(&[0xff], None, &[]),
        named_target(6, &[0xff], Some(&absent())),
        named_target(3, &[0xff], Some(&absent())),
        named_target(13, &[0xff], Some(&absent())),
        workspace(&[0xff], &[0xaa; 20]),
        remote_view(&[0xff], &[], &[]),
        remote_view(b"r", &[], &[remote_ref(&[0xff], &[None], Some(0))]),
        bookmark(b"b", None, &[legacy_remote(&[0xff], None, None)]),
    ] {
        reject(&base(&[entry]), "utf");
    }
}

#[test]
fn jj_view_requires_sha1_commit_ids_in_every_reference_position() {
    for length in [0, 19, 21, 32, 64] {
        let id = vec![0xaa; length];
        reject(&view(std::slice::from_ref(&id), &[]), "identity");
        reject(&base(&[workspace(b"w", &id)]), "identity");
        reject(
            &base(&[named_target(6, b"t", Some(&target(&[Some(id.clone())])))]),
            "identity",
        );
        reject(
            &base(&[remote_view(
                b"r",
                &[],
                &[remote_ref(b"t", &[Some(id)], Some(0))],
            )]),
            "identity",
        );
    }
}

#[test]
fn jj_view_rejects_invalid_conflict_arity_and_empty_native_terms() {
    for (removes, adds) in [(0, 0), (1, 1), (0, 2), (2, 1)] {
        let conflict = [
            (0..removes)
                .flat_map(|_| bytes_field(1, &term(None)))
                .collect::<Vec<_>>(),
            (0..adds)
                .flat_map(|_| bytes_field(2, &term(None)))
                .collect::<Vec<_>>(),
        ]
        .concat();
        reject(
            &base(&[named_target(6, b"t", Some(&bytes_field(3, &conflict)))]),
            "arity",
        );
    }
    for count in [0, 2, 4] {
        reject(
            &base(&[remote_view(
                b"r",
                &[],
                &[remote_ref(b"t", &vec![None; count], Some(0))],
            )]),
            "term",
        );
    }
}

#[test]
fn jj_view_rejects_invalid_booleans_and_noncanonical_remote_enums() {
    reject(
        &[bytes_field(1, &[0xaa; 20]), scalar(12, 2)].concat(),
        "bool",
    );
    for state in [2, u64::MAX, 1_u64 << 32, (1_u64 << 32) + 1] {
        reject(
            &base(&[remote_view(
                b"r",
                &[],
                &[remote_ref(b"t", &[None], Some(state))],
            )]),
            "state",
        );
        reject(
            &base(&[bookmark(
                b"b",
                None,
                &[legacy_remote(b"r", None, Some(state))],
            )]),
            "state",
        );
    }
}

#[test]
fn jj_view_rejects_truncated_and_overflowing_wire_encodings_without_panicking() {
    for raw in [
        vec![0x80],
        vec![0x80; 11],
        vec![0x0a, 0xff],
        vec![
            0x0a, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02,
        ],
        vec![0x0a, 0x14, 0xaa],
        vec![0x60, 0x80],
    ] {
        assert!(decode(RICH_ID, &raw).is_err());
    }
    let valid = unhex(RICH_HEX);
    for length in 0..valid.len() {
        assert!(
            decode(RICH_ID, &valid[..length]).is_err(),
            "accepted prefix {length}"
        );
    }
    reject(&base(&[vec![0]]), "field");
    let raw = [varint(1 << 3 | 2), varint(u64::MAX)].concat();
    reject(&raw, "truncated");
}
