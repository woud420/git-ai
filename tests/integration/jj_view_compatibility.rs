use super::*;

#[test]
fn jj_view_accepts_current_mirrors_and_missing_default_git_head_mirror() {
    assert_eq!(RICH_ID, RICH_NO_HEAD_MIRROR_ID);
    assert_eq!(
        decode(RICH_ID, &unhex(RICH_HEX)).unwrap(),
        decode(RICH_NO_HEAD_MIRROR_ID, &unhex(RICH_NO_HEAD_MIRROR_HEX)).unwrap()
    );
    decode(
        MINIMAL_ID,
        &base(&[bytes_field(2, b""), bytes_field(7, b"")]),
    )
    .unwrap();
    decode(ABSENT_GITHEAD_ID, &unhex(ABSENT_GITHEAD_HEX)).unwrap();
    decode(
        ABSENT_GITHEAD_ID,
        &base(&[named_target(13, b"default", None)]),
    )
    .unwrap();
}

#[test]
fn jj_view_rejects_inconsistent_current_compatibility_mirrors() {
    let mut entries = rich_entries();
    entries[14] = bytes_field(9, &normal(0xbb));
    reject(&view(&[vec![0xbb; 20], vec![0xaa; 20]], &entries), "mirror");
    reject(&base(&[bytes_field(9, &normal(0xaa))]), "head");
    for legacy in [
        legacy_remote(b"r", Some(&normal(0xbb)), Some(1)),
        legacy_remote(b"r", Some(&normal(0xaa)), Some(0)),
        legacy_remote(b"other", Some(&normal(0xaa)), Some(1)),
    ] {
        reject(
            &base(&[
                bookmark(b"b", None, &[legacy]),
                remote_view(
                    b"r",
                    &[remote_ref(b"b", &[Some(vec![0xaa; 20])], Some(1))],
                    &[],
                ),
            ]),
            "mirror",
        );
    }
    reject(
        &base(&[remote_view(
            b"r",
            &[remote_ref(b"b", &[Some(vec![0xaa; 20])], Some(1))],
            &[],
        )]),
        "mirror",
    );
}

#[test]
fn jj_view_rejects_mirror_term_reordering_even_when_current_domain_is_unchanged() {
    let mut entries = rich_entries();
    entries[0] = bookmark(
        b"conflict",
        Some(&target(&[Some(vec![0xaa; 20]), None, Some(vec![0xbb; 20])])),
        &[legacy_remote(
            b"origin",
            Some(&target(&[Some(vec![0xbb; 20]), None, Some(vec![0xaa; 20])])),
            Some(1),
        )],
    );
    reject(&view(&[vec![0xbb; 20], vec![0xaa; 20]], &entries), "mirror");
}

#[test]
fn jj_view_rejects_legacy_data_requiring_migration_or_fallback() {
    for old in [bytes_field(2, &[0xaa; 20]), bytes_field(7, &[0xaa; 20])] {
        reject(&base(&[old]), "legacy");
    }
    for flag in [None, Some(0)] {
        let mut raw = bytes_field(1, &[0xaa; 20]);
        if let Some(flag) = flag {
            raw.extend(scalar(12, flag));
        }
        reject(&raw, "legacy");
    }
    let old_ref = bytes_field(
        3,
        &[bytes_field(1, b"old"), bytes_field(2, &[0xaa; 20])].concat(),
    );
    reject(&base(&[old_ref]), "legacy");
    let both_ref = bytes_field(
        3,
        &[
            bytes_field(1, b"old"),
            bytes_field(2, &[0xaa; 20]),
            bytes_field(3, &normal(0xaa)),
        ]
        .concat(),
    );
    reject(&base(&[both_ref]), "legacy");
    reject(&base(&[named_target(3, b"missing", None)]), "legacy");
    reject(
        &base(&[bookmark(
            b"b",
            None,
            &[legacy_remote(b"r", Some(&normal(0xaa)), Some(1))],
        )]),
        "legacy",
    );
    for old_target in [
        bytes_field(1, &[0xaa; 20]),
        bytes_field(2, &bytes_field(2, &[0xaa; 20])),
    ] {
        reject(
            &base(&[named_target(6, b"old", Some(&old_target))]),
            "legacy",
        );
    }
}

#[test]
fn jj_view_applies_absence_normalization_only_to_local_bookmarks() {
    for raw in [
        bookmark(b"deleted", None, &[]),
        bookmark(b"deleted", Some(&absent()), &[]),
    ] {
        decode(MINIMAL_ID, &base(&[raw])).unwrap();
    }
    assert_ne!(ABSENT_LOCAL_CONFLICT_ID, MINIMAL_ID);
    let conflict = decode(ABSENT_LOCAL_CONFLICT_ID, &unhex(ABSENT_LOCAL_CONFLICT_HEX)).unwrap();
    assert_eq!(conflict.commit_references, 1);
    for (id, hex) in [
        (ABSENT_TAG_ID, ABSENT_TAG_HEX),
        (ABSENT_GITREF_ID, ABSENT_GITREF_HEX),
        (ABSENT_GITHEAD_ID, ABSENT_GITHEAD_HEX),
        (ABSENT_REMOTE_ID, ABSENT_REMOTE_HEX),
    ] {
        assert_ne!(id, MINIMAL_ID);
        assert_eq!(decode(id, &unhex(hex)).unwrap().commit_references, 1);
    }
    decode(ABSENT_TAG_ID, &base(&[named_target(6, b"deleted", None)])).unwrap();
}

#[test]
fn jj_view_optional_and_nonoptional_remote_state_defaults_are_new() {
    let native = remote_ref(b"t", &[Some(vec![0xaa; 20])], None);
    decode(REMOTE_NEW_ID, &base(&[remote_view(b"r", &[], &[native])])).unwrap();
    let raw = base(&[
        bookmark(
            b"b",
            None,
            &[legacy_remote(b"r", Some(&normal(0xaa)), None)],
        ),
        remote_view(
            b"r",
            &[remote_ref(b"b", &[Some(vec![0xaa; 20])], None)],
            &[],
        ),
    ]);
    assert_eq!(
        decode(REMOTE_BOOKMARK_NEW_ID, &raw)
            .unwrap()
            .commit_references,
        2
    );
    decode(REMOTE_BOOKMARK_NEW_ID, &unhex(REMOTE_BOOKMARK_NEW_HEX)).unwrap();
}
