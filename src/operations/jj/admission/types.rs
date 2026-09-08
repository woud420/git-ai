use super::NativeAdmissionExpectation;
use crate::model::jj_observation::JjOperationEvidence;
use crate::operations::jj::registration::RegisteredJjCurrentState;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAdmissionCursor {
    pub(super) source_id: String,
    pub(super) initialization_receipt_id: String,
    pub(super) reader_profile: String,
    pub(super) baseline_id: String,
    pub(super) baseline_generation: u64,
    pub(super) generation: u64,
    pub(super) admitted_head_ids: Vec<String>,
}

impl NativeAdmissionCursor {
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
    pub fn initialization_receipt_id(&self) -> &str {
        &self.initialization_receipt_id
    }
    pub fn reader_profile(&self) -> &str {
        &self.reader_profile
    }
    pub fn baseline_id(&self) -> &str {
        &self.baseline_id
    }
    pub fn baseline_generation(&self) -> u64 {
        self.baseline_generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn admitted_head_ids(&self) -> &[String] {
        &self.admitted_head_ids
    }
    pub fn expectation(&self) -> NativeAdmissionExpectation<'_> {
        NativeAdmissionExpectation {
            source_id: &self.source_id,
            initialization_receipt_id: &self.initialization_receipt_id,
            baseline_id: &self.baseline_id,
            generation: self.generation,
            admitted_head_ids: &self.admitted_head_ids,
        }
    }
}

#[derive(Debug)]
pub struct NativeAdmissionReceipt {
    pub(super) admission_id: String,
    pub(super) cursor: NativeAdmissionCursor,
    pub(super) expected_generation: u64,
    pub(super) expected_admitted_head_ids: Vec<String>,
}

impl NativeAdmissionReceipt {
    pub fn admission_id(&self) -> &str {
        &self.admission_id
    }
    pub fn source_id(&self) -> &str {
        self.cursor.source_id()
    }
    pub fn initialization_receipt_id(&self) -> &str {
        self.cursor.initialization_receipt_id()
    }
    pub fn reader_profile(&self) -> &str {
        self.cursor.reader_profile()
    }
    pub fn baseline_id(&self) -> &str {
        self.cursor.baseline_id()
    }
    pub fn baseline_generation(&self) -> u64 {
        self.cursor.baseline_generation()
    }
    pub fn expected_generation(&self) -> u64 {
        self.expected_generation
    }
    pub fn expected_admitted_head_ids(&self) -> &[String] {
        &self.expected_admitted_head_ids
    }
    pub fn generation(&self) -> u64 {
        self.cursor.generation()
    }
    pub fn captured_head_ids(&self) -> &[String] {
        self.cursor.admitted_head_ids()
    }
}

#[derive(Debug)]
pub struct RegisteredNativeAdmissionState {
    pub(super) registration: RegisteredJjCurrentState,
    pub(super) cursor: NativeAdmissionCursor,
    pub(super) latest_receipt: Option<NativeAdmissionReceipt>,
}

impl RegisteredNativeAdmissionState {
    pub fn registration(&self) -> &RegisteredJjCurrentState {
        &self.registration
    }
    pub fn cursor(&self) -> &NativeAdmissionCursor {
        &self.cursor
    }
    pub fn latest_receipt(&self) -> Option<&NativeAdmissionReceipt> {
        self.latest_receipt.as_ref()
    }
}

pub struct DurableNativeAdmission {
    pub(super) receipt: NativeAdmissionReceipt,
    pub(super) ordered_operations: Vec<JjOperationEvidence>,
    pub(super) reached_baseline_ids: Vec<String>,
    pub(super) reaches_root: bool,
}

impl DurableNativeAdmission {
    pub fn receipt(&self) -> &NativeAdmissionReceipt {
        &self.receipt
    }
    pub fn ordered_operations(&self) -> &[JjOperationEvidence] {
        &self.ordered_operations
    }
    pub fn reached_baseline_ids(&self) -> &[String] {
        &self.reached_baseline_ids
    }
    pub fn reaches_root(&self) -> bool {
        self.reaches_root
    }
}

impl fmt::Debug for DurableNativeAdmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DurableNativeAdmission")
            .field("receipt", &self.receipt)
            .field("operation_count", &self.ordered_operations.len())
            .field("reached_baseline_ids", &self.reached_baseline_ids)
            .field("reaches_root", &self.reaches_root)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct RegisteredNativeAdmission {
    pub(super) registration: RegisteredJjCurrentState,
    pub(super) admission: DurableNativeAdmission,
    pub(super) current_cursor: NativeAdmissionCursor,
}

impl RegisteredNativeAdmission {
    pub fn registration(&self) -> &RegisteredJjCurrentState {
        &self.registration
    }
    pub fn admission(&self) -> &DurableNativeAdmission {
        &self.admission
    }
    pub fn current_cursor(&self) -> &NativeAdmissionCursor {
        &self.current_cursor
    }
}

#[derive(Debug)]
pub enum NativeAdmissionOutcome {
    Admitted(RegisteredNativeAdmission),
    AlreadyAdmitted(RegisteredNativeAdmission),
}
