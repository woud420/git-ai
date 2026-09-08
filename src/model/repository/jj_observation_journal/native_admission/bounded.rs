use super::super::native_baseline::bounded::sequence;
use super::super::registration_records::bounded::ByteString;
use crate::model::jj_observation::{JjOperationEvidence, MAX_JJ_OBSERVATION_OPERATION_BYTES};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub(in crate::model::repository::jj_observation_journal) fn heads<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    sequence(deserializer, 32)
}

pub(in crate::model::repository::jj_observation_journal) fn parents<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    sequence(deserializer, 32)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    operation_id: String,
    #[serde(deserialize_with = "parents")]
    parent_ids: Vec<String>,
    view_id: String,
    operation_bytes: ByteString<MAX_JJ_OBSERVATION_OPERATION_BYTES>,
    view_bytes: ByteString<MAX_JJ_OBSERVATION_OPERATION_BYTES>,
}

pub(in crate::model::repository::jj_observation_journal) fn operations<
    'de,
    D: Deserializer<'de>,
>(
    deserializer: D,
) -> Result<Vec<JjOperationEvidence>, D::Error> {
    let records: Vec<Evidence> = sequence(deserializer, 256)?;
    Ok(records
        .into_iter()
        .map(|record| JjOperationEvidence {
            operation_id: record.operation_id,
            parent_ids: record.parent_ids,
            view_id: record.view_id,
            operation_bytes: record.operation_bytes.0,
            view_bytes: record.view_bytes.0,
        })
        .collect())
}

#[derive(Serialize)]
struct EvidenceRef<'a> {
    operation_id: &'a str,
    parent_ids: &'a [String],
    view_id: &'a str,
    #[serde(serialize_with = "bytes")]
    operation_bytes: &'a [u8],
    #[serde(serialize_with = "bytes")]
    view_bytes: &'a [u8],
}

fn bytes<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(value)
}

pub(super) fn serialize_evidence<'a, S: Serializer>(
    records: impl ExactSizeIterator<Item = &'a JjOperationEvidence>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut sequence = serializer.serialize_seq(Some(records.len()))?;
    for record in records {
        sequence.serialize_element(&EvidenceRef {
            operation_id: &record.operation_id,
            parent_ids: &record.parent_ids,
            view_id: &record.view_id,
            operation_bytes: &record.operation_bytes,
            view_bytes: &record.view_bytes,
        })?;
    }
    sequence.end()
}

pub(super) fn serialize_operations<S: Serializer>(
    records: &[JjOperationEvidence],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serialize_evidence(records.iter(), serializer)
}
