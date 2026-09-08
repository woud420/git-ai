use super::*;
use crate::model::jj_observation::JjOperationEvidence;
use ciborium::value::Value;
use sha2::{Digest, Sha256};

pub(super) fn vector(label: &str) -> &'static vectors::RecordVector {
    vectors::RECORDS
        .iter()
        .find(|item| item.label == label)
        .unwrap()
}
pub(super) fn checksum(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
pub(super) fn value(raw: &[u8]) -> Value {
    ciborium::from_reader(raw).unwrap()
}
pub(super) fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).unwrap();
    out
}
pub(super) fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value
        .as_map()
        .unwrap()
        .iter()
        .find(|(name, _)| name.as_text() == Some(key))
        .map(|(_, value)| value)
        .unwrap()
}
pub(super) fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    value
        .as_map_mut()
        .unwrap()
        .iter_mut()
        .find(|(name, _)| name.as_text() == Some(key))
        .map(|(_, value)| value)
        .unwrap()
}
pub(super) fn text(value: &Value) -> String {
    value.as_text().unwrap().to_owned()
}
pub(super) fn number(value: &Value) -> u64 {
    value.as_integer().unwrap().try_into().unwrap()
}
pub(super) fn strings(value: &Value) -> Vec<String> {
    value.as_array().unwrap().iter().map(text).collect()
}
fn map(fields: Vec<(&str, Value)>) -> Value {
    Value::Map(
        fields
            .into_iter()
            .map(|(key, value)| (Value::Text(key.to_owned()), value))
            .collect(),
    )
}
fn list(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::Text).collect())
}
fn evidence(raw: &Value) -> JjOperationEvidence {
    JjOperationEvidence {
        operation_id: text(field(raw, "operation_id")),
        parent_ids: strings(field(raw, "parent_ids")),
        view_id: text(field(raw, "view_id")),
        operation_bytes: field(raw, "operation_bytes").as_bytes().unwrap().to_vec(),
        view_bytes: field(raw, "view_bytes").as_bytes().unwrap().to_vec(),
    }
}
fn evidence_value(record: &JjOperationEvidence) -> Value {
    map(vec![
        ("operation_id", Value::Text(record.operation_id.clone())),
        ("parent_ids", list(&record.parent_ids)),
        ("view_id", Value::Text(record.view_id.clone())),
        (
            "operation_bytes",
            Value::Bytes(record.operation_bytes.clone()),
        ),
        ("view_bytes", Value::Bytes(record.view_bytes.clone())),
    ])
}

pub(super) struct Input {
    pub source: String,
    pub profile: String,
    pub receipt: String,
    pub baseline: String,
    pub baseline_generation: u64,
    pub generation: u64,
    pub expected: Vec<String>,
    pub heads: Vec<String>,
    pub operations: Vec<JjOperationEvidence>,
}
impl Input {
    pub fn from(label: &str) -> Self {
        let packet = value(vector(label).raw);
        Self {
            source: text(field(&packet, "source_id")),
            profile: text(field(&packet, "reader_profile")),
            receipt: text(field(&packet, "initialization_receipt_id")),
            baseline: text(field(&packet, "baseline_id")),
            baseline_generation: number(field(&packet, "baseline_generation")),
            generation: number(field(&packet, "expected_admission_generation")),
            expected: strings(field(&packet, "expected_admitted_head_ids")),
            heads: strings(field(&packet, "captured_head_ids")),
            operations: field(&packet, "operations")
                .as_array()
                .unwrap()
                .iter()
                .map(evidence)
                .collect(),
        }
    }
    pub fn prepare(&self) -> Result<PreparedNativeAdmission<'_>, JournalError> {
        let refs: Vec<_> = self.operations.iter().collect();
        PreparedNativeAdmission::new(
            NativeAdmissionScope {
                source_id: &self.source,
                reader_profile: &self.profile,
                initialization_receipt_id: &self.receipt,
                baseline_id: &self.baseline,
                baseline_generation: self.baseline_generation,
            },
            ExpectedAdmissionCursor {
                generation: self.generation,
                admitted_head_ids: &self.expected,
            },
            &self.heads,
            &refs,
        )
    }
    pub fn bytes(&self) -> Vec<u8> {
        encode(&map(vec![
            ("record_version", Value::Integer(1.into())),
            (
                "domain",
                Value::Text("git-ai/jj/native-admission/packet/v1".to_owned()),
            ),
            ("source_id", Value::Text(self.source.clone())),
            ("reader_profile", Value::Text(self.profile.clone())),
            (
                "initialization_receipt_id",
                Value::Text(self.receipt.clone()),
            ),
            ("baseline_id", Value::Text(self.baseline.clone())),
            (
                "baseline_generation",
                Value::Integer(self.baseline_generation.into()),
            ),
            (
                "expected_admission_generation",
                Value::Integer(self.generation.into()),
            ),
            ("expected_admitted_head_ids", list(&self.expected)),
            ("captured_head_ids", list(&self.heads)),
            (
                "operations",
                Value::Array(self.operations.iter().map(evidence_value).collect()),
            ),
        ]))
    }
}

pub(super) fn encoded_boundary(over: bool) -> Input {
    use native_wire::{bytes_field, scalar, unhex};
    let mut input = Input::from("left");
    input.operations = boundary::OPERATION_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let last = index + 1 == boundary::OPERATION_IDS.len();
            let id = if last && over {
                boundary::OVER_LAST_OPERATION_ID
            } else {
                id
            };
            let parent = if index == 0 {
                vectors::FIRST
            } else {
                boundary::OPERATION_IDS[index - 1]
            };
            let timestamp = [scalar(1, 0), scalar(2, 0)].concat();
            let description = vec![
                b'x';
                boundary::DESCRIPTION_BYTES
                    + if last {
                        boundary::LAST_EXTRA_BYTES + usize::from(over)
                    } else {
                        0
                    }
            ];
            let metadata = [
                bytes_field(1, &timestamp),
                bytes_field(2, &timestamp),
                bytes_field(3, &description),
                bytes_field(4, &[]),
                bytes_field(5, &[]),
                scalar(7, 0),
            ]
            .concat();
            let operation_bytes = [
                bytes_field(1, &unhex(boundary::VIEW_ID)),
                bytes_field(2, &unhex(parent)),
                bytes_field(3, &metadata),
                scalar(5, 1),
            ]
            .concat();
            JjOperationEvidence {
                operation_id: id.to_string(),
                parent_ids: vec![parent.to_owned()],
                view_id: boundary::VIEW_ID.to_owned(),
                operation_bytes,
                view_bytes: unhex(boundary::VIEW_HEX),
            }
        })
        .collect();
    input.heads = vec![input.operations.last().unwrap().operation_id.clone()];
    input
}
