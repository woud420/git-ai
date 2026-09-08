use super::*;
use crate::jj_operation::support::{bytes_field, operation, scalar, unhex};

#[path = "jj_head_closure_output_vectors.rs"]
mod vectors;
pub(super) use vectors::{ANCHORS, HEADS};

pub(super) fn workspace() -> String {
    "\u{1}".repeat(16_000)
}

pub(super) fn records(anchors: bool) -> Vec<JjOperationEvidence> {
    let workspace = workspace();
    let view = [
        bytes_field(1, &[0xbb; 20]),
        bytes_field(
            8,
            &[
                bytes_field(1, workspace.as_bytes()),
                bytes_field(2, &[0xbb; 20]),
            ]
            .concat(),
        ),
        scalar(12, 1),
    ]
    .concat();
    let ids = if anchors { &ANCHORS } else { &HEADS };
    let parents: Vec<String> = if anchors {
        vec!["00".repeat(64)]
    } else {
        ANCHORS.iter().map(|id| (*id).to_owned()).collect()
    };
    let parent_bytes: Vec<_> = parents.iter().map(|id| unhex(id)).collect();
    ids.iter()
        .enumerate()
        .map(|(index, id)| {
            let kind = if anchors { "anchor" } else { "head" };
            let description = format!("output {kind} {index:02}");
            let timestamp = [scalar(1, 0), scalar(2, 0)].concat();
            let metadata = [
                bytes_field(1, &timestamp),
                bytes_field(2, &timestamp),
                bytes_field(3, description.as_bytes()),
                bytes_field(4, &[]),
                bytes_field(5, &[]),
                scalar(7, 0),
            ]
            .concat();
            let mut bytes = operation(Some(&metadata), &parent_bytes, &[], Some(1));
            let prefix = bytes_field(1, &unhex(vectors::VIEW_ID));
            bytes[..prefix.len()].copy_from_slice(&prefix);
            JjOperationEvidence {
                operation_id: (*id).to_owned(),
                parent_ids: parents.clone(),
                view_id: vectors::VIEW_ID.to_owned(),
                operation_bytes: bytes,
                view_bytes: view.clone(),
            }
        })
        .collect()
}
