pub(super) use crate::jj_operation::support::{bytes_field, numbered_id, scalar, unhex, varint};

pub fn term(value: Option<&[u8]>) -> Vec<u8> {
    value.map(|value| bytes_field(1, value)).unwrap_or_default()
}

pub fn target(values: &[Option<Vec<u8>>]) -> Vec<u8> {
    let removes: Vec<_> = values
        .iter()
        .skip(1)
        .step_by(2)
        .flat_map(|value| bytes_field(1, &term(value.as_deref())))
        .collect();
    let adds: Vec<_> = values
        .iter()
        .step_by(2)
        .flat_map(|value| bytes_field(2, &term(value.as_deref())))
        .collect();
    bytes_field(3, &[removes, adds].concat())
}

pub fn normal(value: u8) -> Vec<u8> {
    target(&[Some(vec![value; 20])])
}

pub fn absent() -> Vec<u8> {
    target(&[None])
}

pub fn view(heads: &[Vec<u8>], entries: &[Vec<u8>]) -> Vec<u8> {
    let mut fields: Vec<_> = heads.iter().map(|head| bytes_field(1, head)).collect();
    fields.extend_from_slice(entries);
    fields.push(scalar(12, 1));
    fields.concat()
}

pub fn base(entries: &[Vec<u8>]) -> Vec<u8> {
    view(&[vec![0xaa; 20]], entries)
}

pub fn named_target(tag: u64, name: &[u8], value: Option<&[u8]>) -> Vec<u8> {
    let mut raw = bytes_field(1, name);
    if let Some(value) = value {
        raw.extend(bytes_field(if tag == 3 { 3 } else { 2 }, value));
    }
    bytes_field(tag, &raw)
}

pub fn workspace(name: &[u8], commit: &[u8]) -> Vec<u8> {
    bytes_field(8, &[bytes_field(1, name), bytes_field(2, commit)].concat())
}

pub fn legacy_remote(name: &[u8], value: Option<&[u8]>, state: Option<u64>) -> Vec<u8> {
    let mut raw = bytes_field(1, name);
    if let Some(value) = value {
        raw.extend(bytes_field(2, value));
    }
    if let Some(state) = state {
        raw.extend(scalar(3, state));
    }
    raw
}

pub fn bookmark(name: &[u8], local: Option<&[u8]>, remote: &[Vec<u8>]) -> Vec<u8> {
    let mut raw = bytes_field(1, name);
    if let Some(local) = local {
        raw.extend(bytes_field(2, local));
    }
    for entry in remote {
        raw.extend(bytes_field(3, entry));
    }
    bytes_field(5, &raw)
}

pub fn remote_ref(name: &[u8], values: &[Option<Vec<u8>>], state: Option<u64>) -> Vec<u8> {
    let mut raw = bytes_field(1, name);
    for value in values {
        raw.extend(bytes_field(2, &term(value.as_deref())));
    }
    if let Some(state) = state {
        raw.extend(scalar(3, state));
    }
    raw
}

pub fn remote_view(name: &[u8], bookmarks: &[Vec<u8>], tags: &[Vec<u8>]) -> Vec<u8> {
    let mut raw = bytes_field(1, name);
    for bookmark in bookmarks {
        raw.extend(bytes_field(2, bookmark));
    }
    for tag in tags {
        raw.extend(bytes_field(3, tag));
    }
    bytes_field(11, &raw)
}

pub fn mirrored_remote(name: &[u8], remote: &[u8], values: &[Option<Vec<u8>>]) -> Vec<Vec<u8>> {
    vec![
        bookmark(
            name,
            None,
            &[legacy_remote(remote, Some(&target(values)), Some(1))],
        ),
        remote_view(remote, &[remote_ref(name, values, Some(1))], &[]),
    ]
}

pub fn rich_entries() -> Vec<Vec<u8>> {
    let conflict = vec![Some(vec![0xaa; 20]), None, Some(vec![0xbb; 20])];
    vec![
        bookmark(
            b"conflict",
            Some(&target(&conflict)),
            &[legacy_remote(b"origin", Some(&target(&conflict)), Some(1))],
        ),
        bookmark(b"deleted", Some(&absent()), &[]),
        bookmark("local-工".as_bytes(), Some(&normal(0xcc)), &[]),
        bookmark(
            b"remoteonly",
            None,
            &[legacy_remote(b"origin", Some(&normal(0xcc)), Some(0))],
        ),
        named_target(6, b"absent", Some(&absent())),
        named_target(6, b"v1", Some(&normal(0xaa))),
        remote_view(
            b"origin",
            &[
                remote_ref(b"conflict", &conflict, Some(1)),
                remote_ref(b"remoteonly", &[Some(vec![0xcc; 20])], Some(0)),
            ],
            &[
                remote_ref(b"release", &[Some(vec![0xbb; 20])], Some(1)),
                remote_ref(b"tombstone", &[None], Some(0)),
            ],
        ),
        remote_view(
            b"tagonly",
            &[],
            &[remote_ref(b"t", &[Some(vec![0xcc; 20])], Some(1))],
        ),
        named_target(3, b"refs/heads/main", Some(&normal(0xaa))),
        named_target(3, b"refs/tags/gone", Some(&absent())),
        named_target(13, b"default", Some(&normal(0xaa))),
        named_target(13, "workspace-工".as_bytes(), Some(&absent())),
        workspace(b"default", &[0xbb; 20]),
        workspace("workspace-工".as_bytes(), &[0xcc; 20]),
        bytes_field(9, &normal(0xaa)),
    ]
}

pub fn rich() -> Vec<u8> {
    view(&[vec![0xbb; 20], vec![0xaa; 20]], &rich_entries())
}
