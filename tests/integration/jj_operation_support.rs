pub fn varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 128 {
        bytes.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}

pub fn bytes_field(tag: u64, bytes: &[u8]) -> Vec<u8> {
    [
        varint(tag << 3 | 2),
        varint(bytes.len() as u64),
        bytes.to_vec(),
    ]
    .concat()
}

pub fn scalar(tag: u64, value: u64) -> Vec<u8> {
    [varint(tag << 3), varint(value)].concat()
}

pub fn operation(
    metadata: Option<&[u8]>,
    parents: &[Vec<u8>],
    predecessors: &[Vec<u8>],
    stores: Option<u64>,
) -> Vec<u8> {
    let mut out = bytes_field(1, &[0x11; 64]);
    for parent in parents {
        out.extend(bytes_field(2, parent));
    }
    if let Some(metadata) = metadata {
        out.extend(bytes_field(3, metadata));
    }
    for predecessor in predecessors {
        out.extend(bytes_field(4, predecessor));
    }
    if let Some(stores) = stores {
        out.extend(scalar(5, stores));
    }
    out
}

pub fn base(metadata: Option<&[u8]>) -> Vec<u8> {
    operation(metadata, &[vec![0x22; 64]], &[], None)
}

pub fn predecessor(commit: &[u8], predecessors: &[Vec<u8>]) -> Vec<u8> {
    let mut out = bytes_field(1, commit);
    for predecessor in predecessors {
        out.extend(bytes_field(2, predecessor));
    }
    out
}

pub fn attribute(key: &[u8], value: &[u8]) -> Vec<u8> {
    bytes_field(6, &[bytes_field(1, key), bytes_field(2, value)].concat())
}

pub fn numbered_id(index: usize, length: usize) -> Vec<u8> {
    let mut out = vec![0; length];
    out[..8].copy_from_slice(&(index as u64).to_le_bytes());
    out
}

pub fn unhex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

pub fn rich_metadata_fields() -> Vec<Vec<u8>> {
    vec![
        bytes_field(
            1,
            &[scalar(1, -123456789_i64 as u64), scalar(2, -300_i64 as u64)].concat(),
        ),
        bytes_field(2, &[scalar(1, 123456800), scalar(2, 330)].concat()),
        bytes_field(3, b"synthetic change"),
        bytes_field(4, b"fixture.invalid"),
        bytes_field(5, b"fixture-user"),
        attribute(b"zeta", b"last"),
        attribute(b"alpha", b"first"),
        scalar(7, 1),
        bytes_field(8, "main-工".as_bytes()),
    ]
}

pub fn rich_metadata() -> Vec<u8> {
    rich_metadata_fields().concat()
}

pub fn rich_predecessors() -> Vec<Vec<u8>> {
    vec![
        predecessor(&[0xdd; 20], &[]),
        predecessor(&[0xaa; 20], &[vec![0xbb; 20], vec![0xcc; 20]]),
    ]
}
