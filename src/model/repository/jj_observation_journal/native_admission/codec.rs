use super::super::{JournalError, codec, invalid};
use super::types::{NativeAdmissionState, StoredAdmissionRecord, StoredNativeAdmission};
use super::validate::{MAX_PACKET_BYTES, MAX_STATE_BYTES};
use crate::model::jj_observation::validate_source;

pub(super) fn decode_packet(
    bytes: &[u8],
    source: &str,
    admission_id: &str,
    generation: u64,
) -> Result<StoredNativeAdmission, JournalError> {
    validate_source(source)?;
    validate_source(admission_id)?;
    let record: StoredAdmissionRecord =
        codec::decode(bytes, bytes.len() as u64, admission_id, MAX_PACKET_BYTES)?;
    record.validate(source, generation)?;
    if codec::encode(&record, MAX_PACKET_BYTES)? != bytes {
        return Err(invalid("native admission packet encoding is not canonical"));
    }
    Ok(StoredNativeAdmission {
        admission_id: admission_id.to_owned(),
        generation,
        record,
    })
}

pub(super) fn decode_state(
    bytes: &[u8],
    source: &str,
    admission_id: &str,
    checksum: &str,
) -> Result<NativeAdmissionState, JournalError> {
    let state: NativeAdmissionState =
        codec::decode(bytes, bytes.len() as u64, checksum, MAX_STATE_BYTES)?;
    state.validate(source, admission_id)?;
    if codec::encode(&state, MAX_STATE_BYTES)? != bytes {
        return Err(invalid("native admission state encoding is not canonical"));
    }
    Ok(state)
}
