use crate::operations::jj::admission::{
    DurableNativeAdmission, NativeAdmissionCursor, NativeAdmissionReceipt,
    RegisteredNativeAdmissionState,
};
use crate::operations::jj::registration::JjRegisteredCheckoutRelation;
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

pub(super) fn status(value: &RegisteredNativeAdmissionState) -> Value {
    let relation = match value.registration().checkout_relation() {
        JjRegisteredCheckoutRelation::BaselineAnchor => "baseline_anchor",
        JjRegisteredCheckoutRelation::OutsideBaseline => "outside_baseline",
    };
    json!({
        "schema_version": 1, "backend": "jj", "attribution_enabled": false,
        "action": "status", "scope": "current_source", "cursor": cursor(value.cursor()),
        "latest_receipt": value.latest_receipt().map(receipt),
        "workspace": {"name": value.registration().workspace_name(), "checkout_relation": relation}
    })
}

pub(super) fn admission(value: Option<&DurableNativeAdmission>) -> Value {
    json!({
        "schema_version": 1, "backend": "jj", "attribution_enabled": false,
        "action": "receipt", "scope": "historical_saved_evidence",
        "admission": value.map(|value| json!({
            "receipt": receipt(value.receipt()),
            "operation_count": value.ordered_operations().len(),
            "reached_baseline_ids": value.reached_baseline_ids(),
            "reaches_root": value.reaches_root()
        }))
    })
}
