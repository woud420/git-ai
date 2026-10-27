use super::*;
use git_ai::operations::jj::admission::{
    NativeReconciliationExpectation, NativeReconciliationOutcome, reconcile_registered_history,
};

#[path = "jj_default_policy_support.rs"]
mod support;
use support::{assert_packet, exact_write_bytes, heads, records, require_rows, write_attempt};
#[path = "jj_default_policy_api.rs"]
mod api;
#[path = "jj_default_policy_finite.rs"]
mod finite;

#[test]
fn jj_default_policy_explicit_repeats_keep_complete_v1_and_exact_byte_budget() {
    observe_case("default_policy:admit", true);
}

#[test]
fn jj_default_policy_reconciliation_alternates_independent_heads_past_eight_generations() {
    observe_case("default_policy:reconcile", false);
}

#[test]
fn jj_default_policy_finite_observation_keeps_v1_and_carries_the_current_cursor() {
    observe_case("default_policy:finite", true);
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    match case.name.rsplit(':').next().unwrap() {
        "admit" => api::run(case, config, false),
        "reconcile" => api::run(case, config, true),
        "finite" => finite::run(case, config),
        other => panic!("unknown default policy fixture {other}"),
    }
}
