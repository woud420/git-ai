use super::super::behavior::same_receipt;
use super::*;
use git_ai::operations::jj::admission::{
    DurableNativeAdmission, JjNativeAdmissionError, NativeAdmissionCursor,
    NativeAdmissionExpectation, NativeAdmissionOutcome, NativeAdmissionReceipt,
    RegisteredNativeAdmission, RegisteredNativeAdmissionState, admit_registered_history,
    read_native_admission, read_registered_admission_state,
};

#[path = "jj_debug_cli_native.rs"]
mod debug_cli;

#[path = "jj_admission_support.rs"]
mod support;
use support::{
    admission_budget, admission_rows, admit_checked, admit_now, assert_admission, assert_cursor,
    checked_known, checked_status, failed, initial, outcome, packet, select, status, total_state,
};
#[path = "jj_admission_behavior.rs"]
mod behavior;
#[path = "jj_admission_faults.rs"]
mod faults;
#[path = "jj_admission_policy.rs"]
mod policy;
#[path = "jj_admission_real.rs"]
mod real;
#[path = "jj_admission_storage.rs"]
mod storage;

#[test]
fn jj_admission_status_derives_zero_only_from_complete_registration() {
    for kind in ["zero", "unregistered", "native_only"] {
        run_case(&format!("history:admission:{kind}"), true, Policy::Root);
    }
}

#[test]
fn jj_admission_two_layouts_persist_exact_parent_first_packets_and_reopen() {
    for colocated in [true, false] {
        run_case("history:admission:chain", colocated, Policy::Root);
    }
}

#[test]
fn jj_admission_baseline_only_packet_retry_and_fresh_repeated_heads_are_distinct() {
    run_case("history:admission:baseline", true, Policy::Root);
}

#[test]
fn jj_admission_historical_retry_keeps_later_cursor_and_aba_generations() {
    run_case("history:admission:retry", true, Policy::Root);
}

#[test]
fn jj_admission_lost_reply_then_changed_heads_requires_a_fresh_expectation() {
    run_case("history:admission:lost_reply", true, Policy::Root);
}

#[test]
fn jj_admission_wrong_scope_and_malformed_expectations_cannot_write() {
    run_case("history:admission:expectations", true, Policy::Root);
}

#[test]
fn jj_admission_root_and_mixed_closure_preserve_original_cutoff() {
    for kind in ["late", "mixed"] {
        run_case(&format!("history:admission:{kind}"), true, Policy::Root);
    }
}

#[test]
fn jj_admission_prior_admission_and_opaque_membership_are_not_traversal_terminals() {
    run_case("history:admission:missing_parent", true, Policy::Root);
}

#[test]
fn jj_admission_exact_source_scope_isolated_for_identical_native_bytes() {
    run_case("history:admission:isolation", false, Policy::Root);
}

#[test]
fn jj_admission_known_id_reads_stored_evidence_after_native_files_disappear() {
    run_case("history:admission:historical_gc", true, Policy::Root);
}

#[test]
fn jj_admission_current_state_latest_and_registration_corruption_refuse_reads() {
    for kind in [
        "state_checksum",
        "packet_checksum",
        "native_packet",
        "native_historical",
        "registration",
    ] {
        run_case(
            &format!("history:admission:corrupt:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_admission_one_sided_gaps_and_retained_higher_generation_refuse() {
    for kind in ["state_gap", "packet_gap", "rollback", "both_erased"] {
        run_case(&format!("history:admission:gap:{kind}"), true, Policy::Root);
    }
}

#[test]
fn jj_admission_duplicate_same_generation_is_checked_for_each_selected_packet() {
    for kind in ["latest", "historical"] {
        run_case(
            &format!("history:admission:duplicate:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_admission_each_write_failure_or_readback_rewrite_rolls_back_both_rows() {
    for kind in [
        "packet_abort",
        "packet_ignore",
        "state_abort",
        "state_ignore",
        "update_abort",
        "update_ignore",
        "packet_delete",
        "registration_rewrite",
    ] {
        run_case(
            &format!("history:admission:write:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_admission_shared_read_budgets_charge_distinct_packets_once_and_never_refund() {
    run_case("history:admission:read_budget", true, Policy::Root);
}

#[test]
fn jj_admission_exact_and_short_write_readback_budgets_preserve_atomicity() {
    run_case("history:admission:write_budget", true, Policy::Root);
}

#[test]
fn jj_admission_expired_deadline_rejects_fresh_and_historical_apis() {
    run_case("history:admission:deadline", true, Policy::Root);
}

#[test]
fn jj_admission_native_count_bound_and_separate_encoded_cap_cannot_partially_write() {
    for kind in ["count", "encoded"] {
        run_case(
            &format!("history:admission:bound:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_admission_denied_policy_also_refuses_existing_receipt_retry() {
    for policy in [
        Policy::Empty,
        Policy::OtherRoot,
        Policy::RootWithRemoteExclusion,
    ] {
        let mut fixture = Fixture::new(false);
        history_fixture_case(
            "history:admission:policy_install",
            &mut fixture,
            Policy::Root,
        );
        history_fixture_case("history:admission:policy_denied", &mut fixture, policy);
    }
    let mut fixture = Fixture::new(false);
    history_fixture_case(
        "history:admission:sentinel_install",
        &mut fixture,
        Policy::Root,
    );
    history_fixture_case(
        "history:admission:policy_denied",
        &mut fixture,
        Policy::Empty,
    );
    let mut fixture = Fixture::new(false);
    history_fixture_case(
        "history:admission:policy_install",
        &mut fixture,
        Policy::Root,
    );
    history_fixture_case(
        "history:admission:canonical_denied",
        &mut fixture,
        Policy::EscapedRoot,
    );
}

#[test]
fn jj_admission_readonly_collector_cannot_advance_existing_admission_state() {
    run_case("history:admission:readonly", true, Policy::Root);
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    let name = case.name.strip_prefix("history:admission:").unwrap();
    match name {
        "zero" | "baseline" => behavior::baseline(case, config),
        "chain" => behavior::chain(case, config),
        "retry" => behavior::retry(case, config),
        "lost_reply" => behavior::lost_reply(case, config),
        "late" | "mixed" => behavior::closure(case, config),
        "isolation" => behavior::isolation(case, config),
        "readonly" => behavior::readonly(case, config),
        "unregistered" | "native_only" => faults::unregistered(case, config),
        "expectations" => faults::expectations(case, config),
        "missing_parent" => faults::missing_parent(case, config),
        "historical_gc" => faults::historical_gc(case, config),
        "read_budget" => storage::read_budget(case, config),
        "write_budget" => storage::write_budget(case, config),
        "deadline" => faults::expired(case, config),
        "policy_install" | "sentinel_install" => policy::install(case, config),
        "policy_denied" | "canonical_denied" => policy::denied(case, config),
        "real_install" => real::install(case, config),
        "real_admit" => real::admit(case, config),
        name if name.starts_with("cli:") => debug_cli::dispatch(case, config),
        name if name.starts_with("reconcile:") => storage::reconciliation::dispatch(case, config),
        name if name.starts_with("corrupt:") => storage::corrupt(case, config),
        name if name.starts_with("gap:") => storage::gap(case, config),
        name if name.starts_with("duplicate:") => storage::duplicate(case, config),
        name if name.starts_with("write:") => storage::write_failure(case, config),
        name if name.starts_with("bound:") => faults::bound(case, config),
        other => panic!("unknown admission fixture {other}"),
    }
}
