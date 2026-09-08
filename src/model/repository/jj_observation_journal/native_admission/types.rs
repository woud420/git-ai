use super::bounded;
use crate::model::jj_observation::JjOperationEvidence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeAdmissionCursor {
    pub generation: u64,
    pub admitted_head_ids: Vec<String>,
}

pub(crate) struct StoredNativeAdmission {
    pub admission_id: String,
    pub generation: u64,
    pub record: StoredAdmissionRecord,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredAdmissionRecord {
    pub(super) record_version: u16,
    pub(super) domain: String,
    pub source_id: String,
    pub reader_profile: String,
    pub initialization_receipt_id: String,
    pub baseline_id: String,
    pub baseline_generation: u64,
    pub expected_admission_generation: u64,
    #[serde(deserialize_with = "bounded::heads")]
    pub expected_admitted_head_ids: Vec<String>,
    #[serde(deserialize_with = "bounded::heads")]
    pub captured_head_ids: Vec<String>,
    #[serde(
        deserialize_with = "bounded::operations",
        serialize_with = "bounded::serialize_operations"
    )]
    pub operations: Vec<JjOperationEvidence>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeAdmissionState {
    pub(super) state_version: u16,
    pub(super) domain: String,
    pub source_id: String,
    pub reader_profile: String,
    pub initialization_receipt_id: String,
    pub baseline_id: String,
    pub baseline_generation: u64,
    pub admission_id: String,
    pub generation: u64,
    #[serde(deserialize_with = "bounded::heads")]
    pub admitted_head_ids: Vec<String>,
}
