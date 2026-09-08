use crate::operations::jj::admission::{
    DurableNativeAdmission, NativeAdmissionCursor, NativeAdmissionOutcome, NativeAdmissionReceipt,
    NativeReconciliationOutcome, RegisteredNativeAdmissionState,
};
use crate::operations::jj::registration::{
    JjRegisteredCheckoutRelation, JjRegistrationOutcome, RegisteredJjCurrentState,
};
use serde_json::{Value, json};

fn cursor(value: &NativeAdmissionCursor) -> Value {
    json!({
        "source_id": value.source_id(),
        "initialization_receipt_id": value.initialization_receipt_id(),
        "reader_profile": value.reader_profile(),
        "baseline_id": value.baseline_id(),
        "baseline_generation": value.baseline_generation(),
        "generation": value.generation(),
        "admitted_head_ids": value.admitted_head_ids()
    })
}

fn receipt(value: &NativeAdmissionReceipt) -> Value {
    json!({
        "admission_id": value.admission_id(),
        "source_id": value.source_id(),
        "initialization_receipt_id": value.initialization_receipt_id(),
        "reader_profile": value.reader_profile(),
        "baseline_id": value.baseline_id(),
        "baseline_generation": value.baseline_generation(),
        "generation": value.generation(),
        "expected_generation": value.expected_generation(),
        "expected_admitted_head_ids": value.expected_admitted_head_ids(),
        "captured_head_ids": value.captured_head_ids()
    })
}

fn workspace(value: &RegisteredJjCurrentState) -> Value {
    let relation = match value.checkout_relation() {
        JjRegisteredCheckoutRelation::BaselineAnchor => "baseline_anchor",
        JjRegisteredCheckoutRelation::OutsideBaseline => "outside_baseline",
    };
    json!({"name": value.workspace_name(), "checkout_relation": relation})
}

pub(super) fn status(value: &RegisteredNativeAdmissionState) -> Value {
    json!({
        "schema_version": 1, "backend": "jj", "attribution_enabled": false,
        "action": "status", "scope": "current_source", "cursor": cursor(value.cursor()),
        "latest_receipt": value.latest_receipt().map(receipt),
        "workspace": workspace(value.registration())
    })
}

fn admission_value(value: &DurableNativeAdmission) -> Value {
    json!({
        "receipt": receipt(value.receipt()),
        "operation_count": value.ordered_operations().len(),
        "reached_baseline_ids": value.reached_baseline_ids(),
        "reaches_root": value.reaches_root()
    })
}

pub(super) fn admission(value: Option<&DurableNativeAdmission>) -> Value {
    json!({
        "schema_version": 1, "backend": "jj", "attribution_enabled": false,
        "action": "receipt", "scope": "historical_saved_evidence",
        "admission": value.map(admission_value)
    })
}

pub(super) fn initialize(value: &JjRegistrationOutcome) -> Value {
    let (outcome, value) = match value {
        JjRegistrationOutcome::Installed(value) => ("installed", value),
        JjRegistrationOutcome::AlreadyRegistered(value) => ("already_registered", value),
    };
    let baseline = value.baseline().receipt();
    json!({
        "schema_version": 1, "backend": "jj", "attribution_enabled": false,
        "action": "initialize", "scope": "current_source", "outcome": outcome,
        "registration": {
            "source_id": value.source_id(),
            "initialization_receipt_id": value.initialization_receipt_id(),
            "attachment_id": value.attachment_id(),
            "reader_profile": baseline.reader_profile(),
            "baseline_id": baseline.baseline_id(),
            "baseline_generation": baseline.generation(),
            "captured_head_ids": baseline.captured_head_ids(),
            "workspace": workspace(value)
        }
    })
}

pub(super) fn capture(value: &NativeAdmissionOutcome) -> Value {
    let (outcome, value) = match value {
        NativeAdmissionOutcome::Admitted(value) => ("admitted", value),
        NativeAdmissionOutcome::AlreadyAdmitted(value) => ("already_admitted", value),
    };
    json!({
        "schema_version": 1, "backend": "jj", "attribution_enabled": false,
        "action": "capture", "scope": "current_source", "outcome": outcome,
        "cursor": cursor(value.current_cursor()),
        "admission": admission_value(value.admission())
    })
}

pub(super) fn observe(value: &NativeReconciliationOutcome, attempt: u64) -> Value {
    let (mut result, registration) = match value {
        NativeReconciliationOutcome::Unchanged(value) => {
            let mut result = status(value);
            result["outcome"] = json!("unchanged");
            (result, value.registration())
        }
        NativeReconciliationOutcome::Admission(value) => {
            let registration = match value {
                NativeAdmissionOutcome::Admitted(value)
                | NativeAdmissionOutcome::AlreadyAdmitted(value) => value.registration(),
            };
            (capture(value), registration)
        }
    };
    result["action"] = json!("observe");
    result["attempt"] = json!(attempt);
    result["workspace"] = json!({
        "name": registration.workspace_name(), "attachment_id": registration.attachment_id()
    });
    result
}
