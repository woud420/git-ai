use super::super::{JournalError, codec, invalid};
use super::bounded;
use super::types::{NativeAdmissionState, StoredAdmissionRecord};
use super::validate::{self, MAX_PACKET_BYTES, MAX_STATE_BYTES, PACKET_DOMAIN, STATE_DOMAIN};
use crate::model::jj_observation::JjOperationEvidence;
use serde::{Serialize, Serializer};

pub(crate) struct NativeAdmissionScope<'a> {
    pub source_id: &'a str,
    pub reader_profile: &'a str,
    pub initialization_receipt_id: &'a str,
    pub baseline_id: &'a str,
    pub baseline_generation: u64,
}

pub(crate) struct ExpectedAdmissionCursor<'a> {
    pub generation: u64,
    pub admitted_head_ids: &'a [String],
}

#[derive(Serialize)]
pub(super) struct Request<'a> {
    record_version: u16,
    domain: &'static str,
    pub source_id: &'a str,
    pub reader_profile: &'a str,
    pub initialization_receipt_id: &'a str,
    pub baseline_id: &'a str,
    pub baseline_generation: u64,
    pub expected_admission_generation: u64,
    pub expected_admitted_head_ids: Vec<&'a String>,
    captured_head_ids: Vec<&'a String>,
    #[serde(serialize_with = "serialize_operations")]
    operations: Vec<&'a JjOperationEvidence>,
}

fn serialize_operations<S: Serializer>(
    records: &[&JjOperationEvidence],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    bounded::serialize_evidence(records.iter().copied(), serializer)
}

pub(crate) struct PreparedNativeAdmission<'a> {
    pub(super) request: Request<'a>,
    pub(super) state: NativeAdmissionState,
    pub(super) record_bytes: Vec<u8>,
    pub(super) state_bytes: Vec<u8>,
}

impl<'a> PreparedNativeAdmission<'a> {
    pub(crate) fn new(
        scope: NativeAdmissionScope<'a>,
        expected: ExpectedAdmissionCursor<'a>,
        captured_head_ids: &'a [String],
        ordered_evidence: &[&'a JjOperationEvidence],
    ) -> Result<Self, JournalError> {
        validate::scope(
            scope.source_id,
            scope.reader_profile,
            scope.initialization_receipt_id,
            scope.baseline_id,
            scope.baseline_generation,
        )?;
        validate::contents(
            expected.generation,
            expected.admitted_head_ids,
            captured_head_ids,
            ordered_evidence.iter().copied(),
        )?;
        let mut expected_admitted_head_ids: Vec<_> = expected.admitted_head_ids.iter().collect();
        expected_admitted_head_ids.sort();
        let mut captured_head_ids: Vec<_> = captured_head_ids.iter().collect();
        captured_head_ids.sort();
        let request = Request {
            record_version: 1,
            domain: PACKET_DOMAIN,
            source_id: scope.source_id,
            reader_profile: scope.reader_profile,
            initialization_receipt_id: scope.initialization_receipt_id,
            baseline_id: scope.baseline_id,
            baseline_generation: scope.baseline_generation,
            expected_admission_generation: expected.generation,
            expected_admitted_head_ids,
            captured_head_ids,
            operations: ordered_evidence.to_vec(),
        };
        let record_bytes = codec::encode(&request, MAX_PACKET_BYTES)?;
        let state = request.state(codec::checksum(&record_bytes));
        let state_bytes = codec::encode(&state, MAX_STATE_BYTES)?;
        Ok(Self {
            request,
            state,
            record_bytes,
            state_bytes,
        })
    }

    pub(crate) fn admission_id(&self) -> &str {
        &self.state.admission_id
    }
}

impl Request<'_> {
    fn state(&self, admission_id: String) -> NativeAdmissionState {
        NativeAdmissionState {
            state_version: 1,
            domain: STATE_DOMAIN.to_owned(),
            source_id: self.source_id.to_owned(),
            reader_profile: self.reader_profile.to_owned(),
            initialization_receipt_id: self.initialization_receipt_id.to_owned(),
            baseline_id: self.baseline_id.to_owned(),
            baseline_generation: self.baseline_generation,
            admission_id,
            generation: self.expected_admission_generation + 1,
            admitted_head_ids: self
                .captured_head_ids
                .iter()
                .map(|id| (*id).clone())
                .collect(),
        }
    }

    pub(super) fn matches(&self, record: &StoredAdmissionRecord) -> bool {
        self.record_version == record.record_version
            && self.domain == record.domain
            && self.source_id == record.source_id
            && self.reader_profile == record.reader_profile
            && self.initialization_receipt_id == record.initialization_receipt_id
            && self.baseline_id == record.baseline_id
            && self.baseline_generation == record.baseline_generation
            && self.expected_admission_generation == record.expected_admission_generation
            && self
                .expected_admitted_head_ids
                .iter()
                .copied()
                .eq(record.expected_admitted_head_ids.iter())
            && self
                .captured_head_ids
                .iter()
                .copied()
                .eq(record.captured_head_ids.iter())
            && self.operations.iter().copied().eq(record.operations.iter())
    }

    pub(super) fn require_registration(
        &self,
        registration: &super::super::registration::StoredRegistrationSnapshot,
    ) -> Result<(), JournalError> {
        super::snapshot::require_scope(
            registration,
            self.source_id,
            self.reader_profile,
            self.initialization_receipt_id,
            self.baseline_id,
            self.baseline_generation,
        )?;
        if self.expected_admission_generation == 0
            && !self
                .expected_admitted_head_ids
                .iter()
                .copied()
                .eq(registration.native.state.captured_head_ids.iter())
        {
            return Err(invalid("native admission initial cutoff mismatch"));
        }
        Ok(())
    }
}
