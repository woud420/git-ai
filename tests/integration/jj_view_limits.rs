use super::*;

#[test]
fn jj_view_limits_are_explicit_and_independent() {
    assert_eq!(MAX_VIEW_BYTES, 2 * 1024 * 1024);
    assert_eq!(MAX_VIEW_COMMIT_REFERENCES, 4096);
    assert_eq!(MAX_VIEW_NAME_BYTES, 64 * 1024);
    assert_eq!(MAX_VIEW_WIRE_ENTRIES, 8192);
}

#[test]
fn jj_view_raw_limit_is_inclusive_and_checked_before_parsing() {
    reject(&vec![0; MAX_VIEW_BYTES], "field");
    reject(&vec![0; MAX_VIEW_BYTES + 1], "limit");
}

#[test]
fn jj_view_reference_limit_counts_all_heads() {
    let mut heads: Vec<_> = (0..MAX_VIEW_COMMIT_REFERENCES)
        .map(|i| numbered_id(i, 20))
        .collect();
    let decoded = decode(HEADS_LIMIT_ID, &view(&heads, &[])).unwrap();
    assert_eq!(decoded.head_ids.len(), MAX_VIEW_COMMIT_REFERENCES);
    assert_eq!(decoded.commit_references, MAX_VIEW_COMMIT_REFERENCES);
    heads.push(numbered_id(MAX_VIEW_COMMIT_REFERENCES, 20));
    reject(&view(&heads, &[]), "limit");
}

#[test]
fn jj_view_reference_limit_counts_repeated_terms_without_simplification() {
    let repeated = target(&vec![Some(vec![0xaa; 20]); MAX_VIEW_COMMIT_REFERENCES - 1]);
    let raw = base(&[named_target(6, b"t", Some(&repeated))]);
    let decoded = decode(REPEATED_REFERENCES_LIMIT_ID, &raw).unwrap();
    assert_eq!(decoded.commit_references, MAX_VIEW_COMMIT_REFERENCES);
    let raw = base(&[
        named_target(6, b"t", Some(&repeated)),
        workspace(b"w", &[0xaa; 20]),
    ]);
    reject(&raw, "limit");
}

#[test]
fn jj_view_reference_budget_is_aggregate_across_each_retained_collection() {
    let heads: Vec<_> = (0..MAX_VIEW_COMMIT_REFERENCES)
        .map(|i| numbered_id(i, 20))
        .collect();
    for entries in [
        vec![bookmark(b"b", Some(&normal(0xaa)), &[])],
        vec![named_target(6, b"t", Some(&normal(0xaa)))],
        vec![named_target(3, b"g", Some(&normal(0xaa)))],
        vec![named_target(13, b"w", Some(&normal(0xaa)))],
        vec![workspace(b"w", &[0xaa; 20])],
        vec![remote_view(
            b"r",
            &[],
            &[remote_ref(b"t", &[Some(vec![0xaa; 20])], Some(0))],
        )],
        mirrored_remote(b"b", b"r", &[Some(vec![0xaa; 20])]),
    ] {
        reject(&view(&heads, &entries), "limit");
    }
}

#[test]
fn jj_view_name_byte_limit_is_inclusive_and_counts_utf8_bytes() {
    let name = vec![b'x'; MAX_VIEW_NAME_BYTES];
    decode(
        NAME_BYTES_LIMIT_ID,
        &base(&[named_target(6, &name, Some(&absent()))]),
    )
    .unwrap();
    let mut unicode = vec![b'x'; MAX_VIEW_NAME_BYTES - 2];
    unicode.extend("é".as_bytes());
    decode(
        UNICODE_NAME_BYTES_LIMIT_ID,
        &base(&[named_target(6, &unicode, Some(&absent()))]),
    )
    .unwrap();
    unicode.insert(0, b'x');
    reject(
        &base(&[named_target(6, &unicode, Some(&absent()))]),
        "limit",
    );
    reject(
        &base(&[named_target(
            6,
            &vec![b'x'; MAX_VIEW_NAME_BYTES + 1],
            Some(&absent()),
        )]),
        "limit",
    );
}

#[test]
fn jj_view_name_budget_counts_every_on_wire_name_position() {
    let large = named_target(6, &vec![b'x'; MAX_VIEW_NAME_BYTES], Some(&absent()));
    for extra in [
        bookmark(b"b", None, &[]),
        named_target(6, b"t", None),
        named_target(3, b"g", Some(&absent())),
        named_target(13, b"w", None),
        workspace(b"w", &[0xaa; 20]),
        remote_view(b"r", &[], &[]),
        remote_view(b"", &[], &[remote_ref(b"t", &[None], Some(0))]),
        bookmark(b"", None, &[legacy_remote(b"r", None, None)]),
    ] {
        reject(&base(&[large.clone(), extra]), "limit");
    }
}

#[test]
fn jj_view_name_budget_charges_current_and_legacy_mirror_occurrences() {
    let name = vec![b'x'; (MAX_VIEW_NAME_BYTES - 2) / 2];
    let entries = vec![
        bookmark(
            &name,
            None,
            &[legacy_remote(b"r", Some(&absent()), Some(0))],
        ),
        remote_view(b"r", &[remote_ref(&name, &[None], Some(0))], &[]),
    ];
    let decoded = decode(MIRROR_NAMES_LIMIT_ID, &base(&entries)).unwrap();
    assert_eq!(decoded.commit_references, 1);
    let mut excess = entries;
    excess.push(named_target(6, b"x", None));
    reject(&base(&excess), "limit");
}

#[test]
fn jj_view_wire_budget_counts_absent_terms_before_semantic_normalization() {
    let terms = target(&vec![None; MAX_VIEW_WIRE_ENTRIES - 3]);
    // Tag entry + terms + bookmark entry + its explicit None term = 8192.
    let mut entries = vec![
        named_target(6, b"t", Some(&terms)),
        bookmark(b"empty", Some(&absent()), &[]),
    ];
    let decoded = decode(WIRE_ENTRIES_LIMIT_ID, &base(&entries)).unwrap();
    assert_eq!(decoded.commit_references, 1);
    entries.push(bookmark(b"extra", None, &[]));
    reject(&base(&entries), "limit");
}

#[test]
fn jj_view_wire_budget_counts_absent_bookmark_entries_before_filtering() {
    let mut entries: Vec<_> = (0..MAX_VIEW_WIRE_ENTRIES)
        .map(|index| bookmark(format!("b{index:04}").as_bytes(), None, &[]))
        .collect();
    let decoded = decode(MINIMAL_ID, &base(&entries)).unwrap();
    assert_eq!(decoded.commit_references, 1);
    entries.push(bookmark(b"extra", None, &[]));
    reject(&base(&entries), "limit");
}

#[test]
fn jj_view_wire_budget_includes_mirrors_but_reference_budget_excludes_them() {
    let values = vec![Some(vec![0xaa; 20]); 4093];
    let mut entries = mirrored_remote(b"b", b"r", &values);
    entries.push(bookmark(b"empty", Some(&absent()), &[]));
    let decoded = decode(MIRROR_WIRE_LIMIT_ID, &base(&entries)).unwrap();
    assert_eq!(decoded.commit_references, 4094);
    entries.push(bookmark(b"extra", None, &[]));
    reject(&base(&entries), "limit");
}
