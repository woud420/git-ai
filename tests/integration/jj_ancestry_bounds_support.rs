use super::*;
use crate::jj_operation::support::{numbered_id, operation, predecessor};

pub(super) fn record(
    id: &str,
    parent: &str,
    description: &[u8],
    view_id: &str,
    view_bytes: Vec<u8>,
    predecessors: &[Vec<u8>],
) -> JjOperationEvidence {
    let timestamp = [scalar(1, 0), scalar(2, 0)].concat();
    let metadata = [
        bytes_field(1, &timestamp),
        bytes_field(2, &timestamp),
        bytes_field(3, description),
        bytes_field(4, &[]),
        bytes_field(5, &[]),
        scalar(7, 0),
    ]
    .concat();
    let mut operation_bytes = operation(Some(&metadata), &[unhex(parent)], predecessors, Some(1));
    let original_view = bytes_field(1, &[0x11; 64]);
    assert!(operation_bytes.starts_with(&original_view));
    operation_bytes[..original_view.len()].copy_from_slice(&bytes_field(1, &unhex(view_id)));
    JjOperationEvidence {
        operation_id: id.to_owned(),
        parent_ids: vec![parent.to_owned()],
        view_id: view_id.to_owned(),
        operation_bytes,
        view_bytes,
    }
}

pub(super) fn chain(count: usize) -> Vec<JjOperationEvidence> {
    assert!(count <= CHAIN_IDS.len());
    CHAIN_IDS
        .iter()
        .take(count)
        .enumerate()
        .map(|(index, id)| {
            let parent = if index == 0 {
                FIRST_ID
            } else {
                CHAIN_IDS[index - 1]
            };
            record(id, parent, b"chain", MINIMAL_ID, unhex(MINIMAL_HEX), &[])
        })
        .collect()
}

pub(super) fn raw_chain(over: bool) -> Vec<JjOperationEvidence> {
    RAW_CHAIN_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let last_over = over && index == RAW_CHAIN_IDS.len() - 1;
            let id = if last_over { RAW_OVER_ID } else { *id };
            let parent = if index == 0 {
                FIRST_ID
            } else {
                RAW_CHAIN_IDS[index - 1]
            };
            let description = vec![b'x'; RAW_DESCRIPTION_BYTES + usize::from(last_over)];
            record(
                id,
                parent,
                &description,
                MINIMAL_ID,
                unhex(MINIMAL_HEX),
                &[],
            )
        })
        .collect()
}

pub(super) fn predecessor_records(over: bool) -> Vec<JjOperationEvidence> {
    let first_map = predecessor(&[0xaa; 20], &vec![vec![0xbb; 20]; 2047]);
    let second_map = predecessor(&[0xaa; 20], &vec![vec![0xbb; 20]; 2047 + usize::from(over)]);
    vec![
        record(
            PREDECESSOR_LEFT_ID,
            FIRST_ID,
            b"pred-left",
            MINIMAL_ID,
            unhex(MINIMAL_HEX),
            &[first_map],
        ),
        record(
            if over {
                PREDECESSOR_OVER_ID
            } else {
                PREDECESSOR_RIGHT_ID
            },
            PREDECESSOR_LEFT_ID,
            b"pred-right",
            MINIMAL_ID,
            unhex(MINIMAL_HEX),
            &[second_map],
        ),
    ]
}

pub(super) fn view_records(over: bool) -> Vec<JjOperationEvidence> {
    let mut shared_view = (0..2048)
        .flat_map(|index| bytes_field(1, &numbered_id(index, 20)))
        .collect::<Vec<_>>();
    shared_view.extend(scalar(12, 1));
    let mut records = vec![
        record(
            VIEW_LEFT_ID,
            FIRST_ID,
            b"view-left",
            SHARED_VIEW_ID,
            shared_view.clone(),
            &[],
        ),
        record(
            VIEW_RIGHT_ID,
            VIEW_LEFT_ID,
            b"view-right",
            SHARED_VIEW_ID,
            shared_view,
            &[],
        ),
    ];
    if over {
        records.push(record(
            VIEW_OVER_ID,
            VIEW_RIGHT_ID,
            b"view-over",
            MINIMAL_ID,
            unhex(MINIMAL_HEX),
            &[],
        ));
    }
    records
}

pub(super) fn raw_size(records: &[JjOperationEvidence]) -> usize {
    records
        .iter()
        .map(|record| record.operation_bytes.len() + record.view_bytes.len())
        .sum()
}

pub(super) fn verify_each(records: &[JjOperationEvidence]) {
    for record in records {
        verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
    }
}
