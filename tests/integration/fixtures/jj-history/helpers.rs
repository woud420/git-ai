use git_ai::model::jj_observation::JjOperationEvidence;

mod vectors {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/integration/fixtures/jj-history/vectors.rs"
    ));
}
use vectors::*;
pub use vectors::{
    CHAIN_IDS, CONVERGED_MERGE_ID, FIRST_ID, LATE_BRANCH_ID, LEFT_ID, MERGE_ID, MERGE_PAIR_BYTES,
    MIXED_MERGE_ID, RAW_CHAIN_IDS, RICH_CHILD_ID, RICH_PARENT_ID, RIGHT_ID,
};

pub fn first() -> JjOperationEvidence {
    stored(FIRST_ID, FIRST_HEX, &[&"00".repeat(64)], false)
}

pub fn left() -> JjOperationEvidence {
    stored(LEFT_ID, LEFT_HEX, &[FIRST_ID], false)
}

pub fn right() -> JjOperationEvidence {
    stored(RIGHT_ID, RIGHT_HEX, &[FIRST_ID], false)
}

pub fn merge() -> JjOperationEvidence {
    stored(MERGE_ID, MERGE_HEX, &[LEFT_ID, RIGHT_ID], true)
}

pub fn rich_parent() -> JjOperationEvidence {
    stored(RICH_PARENT_ID, RICH_PARENT_HEX, &[MERGE_ID], true)
}

pub fn rich_child() -> JjOperationEvidence {
    stored(RICH_CHILD_ID, RICH_CHILD_HEX, &[RICH_PARENT_ID], true)
}

pub fn late_branch() -> JjOperationEvidence {
    stored(LATE_BRANCH_ID, LATE_BRANCH_HEX, &[LEFT_ID], true)
}

pub fn mixed_merge() -> JjOperationEvidence {
    stored(
        MIXED_MERGE_ID,
        MIXED_MERGE_HEX,
        &[MERGE_ID, LATE_BRANCH_ID],
        true,
    )
}

pub fn converged_merge() -> JjOperationEvidence {
    stored(
        CONVERGED_MERGE_ID,
        CONVERGED_MERGE_HEX,
        &[RICH_PARENT_ID, RICH_CHILD_ID],
        true,
    )
}

fn stored(id: &str, hex: &str, parents: &[&str], rich: bool) -> JjOperationEvidence {
    let (view_id, view_hex) = if rich {
        (RICH_VIEW_ID, RICH_VIEW_HEX)
    } else {
        (MINIMAL_VIEW_ID, MINIMAL_VIEW_HEX)
    };
    JjOperationEvidence {
        operation_id: id.to_owned(),
        parent_ids: parents.iter().map(|id| (*id).to_owned()).collect(),
        view_id: view_id.to_owned(),
        operation_bytes: unhex(hex),
        view_bytes: unhex(view_hex),
    }
}

pub fn chain(count: usize) -> Vec<JjOperationEvidence> {
    assert!(count <= CHAIN_IDS.len());
    CHAIN_IDS[..count]
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let parent = if index == 0 {
                MERGE_ID
            } else {
                CHAIN_IDS[index - 1]
            };
            record(
                id,
                parent,
                b"chain",
                MINIMAL_VIEW_ID,
                unhex(MINIMAL_VIEW_HEX),
                None,
            )
        })
        .collect()
}

pub fn raw_chain(over: bool) -> Vec<JjOperationEvidence> {
    RAW_CHAIN_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let last_over = over && index == RAW_CHAIN_IDS.len() - 1;
            let id = if last_over { RAW_OVER_ID } else { *id };
            let parent = if index == 0 {
                MERGE_ID
            } else {
                RAW_CHAIN_IDS[index - 1]
            };
            record(
                id,
                parent,
                &vec![b'x'; RAW_DESCRIPTION_BYTES + usize::from(last_over)],
                MINIMAL_VIEW_ID,
                unhex(MINIMAL_VIEW_HEX),
                None,
            )
        })
        .collect()
}

// With sampled heads [MERGE, tail], 255 nonterminals plus the freshly read
// baseline head use exactly 256 union slots. The final pair supplies the bytes
// left after 254 ordinary 32KiB pairs and the independently sized baseline pair.
pub fn raw_with_baseline(over: bool) -> Vec<JjOperationEvidence> {
    let mut records = RAW_CHAIN_IDS[..254]
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let parent = if index == 0 {
                MERGE_ID
            } else {
                RAW_CHAIN_IDS[index - 1]
            };
            record(
                id,
                parent,
                &vec![b'x'; RAW_DESCRIPTION_BYTES],
                MINIMAL_VIEW_ID,
                unhex(MINIMAL_VIEW_HEX),
                None,
            )
        })
        .collect::<Vec<_>>();
    records.push(record(
        if over {
            RAW_BASELINE_OVER_ID
        } else {
            RAW_BASELINE_TAIL_ID
        },
        RAW_CHAIN_IDS[253],
        &vec![b'x'; RAW_BASELINE_DESCRIPTION_BYTES + usize::from(over)],
        MINIMAL_VIEW_ID,
        unhex(MINIMAL_VIEW_HEX),
        None,
    ));
    records
}

pub fn predecessor_records(over: bool) -> Vec<JjOperationEvidence> {
    vec![
        record(
            PREDECESSOR_LEFT_ID,
            MERGE_ID,
            b"pred-left",
            MINIMAL_VIEW_ID,
            unhex(MINIMAL_VIEW_HEX),
            Some(2047),
        ),
        record(
            if over {
                PREDECESSOR_OVER_ID
            } else {
                PREDECESSOR_RIGHT_ID
            },
            PREDECESSOR_LEFT_ID,
            b"pred-right",
            MINIMAL_VIEW_ID,
            unhex(MINIMAL_VIEW_HEX),
            Some(2047 + usize::from(over)),
        ),
    ]
}

pub fn view_records(over: bool) -> Vec<JjOperationEvidence> {
    let mut view = Vec::new();
    for index in 0u64..2048 {
        let mut id = [0u8; 20];
        id[..8].copy_from_slice(&index.to_le_bytes());
        view.extend(field(1, &id));
    }
    view.extend(scalar(12, 1));
    let mut records = vec![
        record(
            VIEW_LEFT_ID,
            MERGE_ID,
            b"view-left",
            SHARED_VIEW_ID,
            view.clone(),
            None,
        ),
        record(
            VIEW_RIGHT_ID,
            VIEW_LEFT_ID,
            b"view-right",
            SHARED_VIEW_ID,
            view,
            None,
        ),
    ];
    if over {
        records.push(record(
            VIEW_OVER_ID,
            VIEW_RIGHT_ID,
            b"view-over",
            MINIMAL_VIEW_ID,
            unhex(MINIMAL_VIEW_HEX),
            None,
        ));
    }
    records
}

pub fn raw_size(records: &[JjOperationEvidence]) -> usize {
    records
        .iter()
        .map(|r| r.operation_bytes.len() + r.view_bytes.len())
        .sum::<usize>()
}

fn record(
    id: &str,
    parent: &str,
    description: &[u8],
    view_id: &str,
    view_bytes: Vec<u8>,
    predecessor_edges: Option<usize>,
) -> JjOperationEvidence {
    let timestamp = [scalar(1, 0), scalar(2, 0)].concat();
    let metadata = [
        field(1, &timestamp),
        field(2, &timestamp),
        field(3, description),
        field(4, &[]),
        field(5, &[]),
        scalar(7, 0),
    ]
    .concat();
    let mut operation_bytes = [
        field(1, &unhex(view_id)),
        field(2, &unhex(parent)),
        field(3, &metadata),
    ]
    .concat();
    if let Some(edges) = predecessor_edges {
        let mut predecessor = field(1, &[0xaa; 20]);
        for _ in 0..edges {
            predecessor.extend(field(2, &[0xbb; 20]));
        }
        operation_bytes.extend(field(4, &predecessor));
    }
    operation_bytes.extend(scalar(5, 1));
    JjOperationEvidence {
        operation_id: id.to_owned(),
        parent_ids: vec![parent.to_owned()],
        view_id: view_id.to_owned(),
        operation_bytes,
        view_bytes,
    }
}

fn unhex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("invalid fixture hex"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
}

fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
    out
}

fn scalar(tag: u64, value: u64) -> Vec<u8> {
    [varint(tag << 3), varint(value)].concat()
}

fn field(tag: u64, value: &[u8]) -> Vec<u8> {
    [
        varint(tag << 3 | 2),
        varint(value.len() as u64),
        value.to_vec(),
    ]
    .concat()
}
