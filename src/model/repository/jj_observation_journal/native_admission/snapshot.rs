use super::super::registration::StoredRegistrationSnapshot;
use super::super::{JournalError, invalid};
use super::types::{
    NativeAdmissionCursor, NativeAdmissionState, StoredAdmissionRecord, StoredNativeAdmission,
};

pub(crate) struct StoredAdmissionSnapshot {
    pub registration: StoredRegistrationSnapshot,
    pub cursor: NativeAdmissionCursor,
    pub latest: Option<StoredNativeAdmission>,
    pub(super) distinct_requested: Option<StoredNativeAdmission>,
    pub(super) requested_is_latest: bool,
}

impl StoredAdmissionSnapshot {
    pub(crate) fn requested(&self) -> Option<&StoredNativeAdmission> {
        if self.requested_is_latest {
            self.latest.as_ref()
        } else {
            self.distinct_requested.as_ref()
        }
    }

    pub(crate) fn into_requested(
        self,
    ) -> (
        StoredRegistrationSnapshot,
        NativeAdmissionCursor,
        Option<StoredNativeAdmission>,
    ) {
        let selected = if self.requested_is_latest {
            self.latest
        } else {
            self.distinct_requested
        };
        (self.registration, self.cursor, selected)
    }
}

pub(super) fn require_scope(
    saved: &StoredRegistrationSnapshot,
    source: &str,
    profile: &str,
    receipt: &str,
    baseline: &str,
    baseline_generation: u64,
) -> Result<(), JournalError> {
    let registered = &saved.registration.record;
    if source != registered.source_id
        || profile != registered.reader_profile
        || receipt != saved.registration.checksum
        || baseline != registered.baseline_id
        || baseline_generation != registered.baseline_generation
    {
        return Err(invalid("native admission registration scope mismatch"));
    }
    Ok(())
}

pub(super) fn require_packet_scope(
    registration: &StoredRegistrationSnapshot,
    packet: &StoredAdmissionRecord,
) -> Result<(), JournalError> {
    require_scope(
        registration,
        &packet.source_id,
        &packet.reader_profile,
        &packet.initialization_receipt_id,
        &packet.baseline_id,
        packet.baseline_generation,
    )?;
    if packet.expected_admission_generation == 0
        && packet.expected_admitted_head_ids != registration.native.state.captured_head_ids
    {
        return Err(invalid("native admission initial cutoff mismatch"));
    }
    Ok(())
}

pub(super) fn require_state_packet(
    state: &NativeAdmissionState,
    packet: &StoredNativeAdmission,
) -> Result<(), JournalError> {
    let record = &packet.record;
    if state.admission_id != packet.admission_id
        || state.generation != packet.generation
        || state.source_id != record.source_id
        || state.reader_profile != record.reader_profile
        || state.initialization_receipt_id != record.initialization_receipt_id
        || state.baseline_id != record.baseline_id
        || state.baseline_generation != record.baseline_generation
        || state.admitted_head_ids != record.captured_head_ids
    {
        return Err(invalid("native admission state and packet mismatch"));
    }
    Ok(())
}

// Checked canonical record digests bind the large immutable baseline/registration payloads.
// Retaining only these small identities allows the preflight snapshot to be released before readback.
pub(super) struct RegistrationIdentity {
    receipt: String,
    original_workspace: String,
    selected_workspace: String,
    baseline_state: super::super::native_baseline::NativeBaselineState,
}

impl RegistrationIdentity {
    pub(super) fn from(snapshot: &StoredRegistrationSnapshot) -> Self {
        Self {
            receipt: snapshot.registration.checksum.clone(),
            original_workspace: snapshot.original_workspace.checksum.clone(),
            selected_workspace: snapshot.selected_workspace().checksum.clone(),
            baseline_state: snapshot.native.state.clone(),
        }
    }

    pub(super) fn matches(&self, snapshot: &StoredRegistrationSnapshot) -> bool {
        self.receipt == snapshot.registration.checksum
            && self.original_workspace == snapshot.original_workspace.checksum
            && self.selected_workspace == snapshot.selected_workspace().checksum
            && self.baseline_state == snapshot.native.state
    }
}
