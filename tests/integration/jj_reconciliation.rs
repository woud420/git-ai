use super::*;
use git_ai::operations::jj::admission::{
    NativeReconciliationExpectation, NativeReconciliationOutcome, reconcile_registered_history,
};

#[path = "jj_reconciliation_checks.rs"]
mod checks;
use checks::*;
#[path = "jj_reconciliation_behavior.rs"]
mod behavior;
#[path = "jj_reconciliation_faults.rs"]
mod faults;
#[path = "jj_reconciliation_real.rs"]
mod real;
#[path = "jj_reconciliation_targets.rs"]
mod targets;

#[test]
fn jj_reconciliation_zero_and_positive_unchanged_heads_never_append() {
    for colocated in [true, false] {
        run_case("history:admission:reconcile:zero", colocated, Policy::Root);
        run_case(
            "history:admission:reconcile:positive",
            colocated,
            Policy::Root,
        );
    }
}

#[test]
fn jj_reconciliation_unchanged_needs_no_missing_ancestor_but_changed_does() {
    run_case("history:admission:reconcile:no_walk", true, Policy::Root);
}

#[test]
fn jj_reconciliation_manual_repeated_heads_make_old_expectations_stale() {
    run_case("history:admission:reconcile:manual", true, Policy::Root);
}

#[test]
fn jj_reconciliation_changed_head_retry_keeps_later_cursor_and_aba_history() {
    for colocated in [true, false] {
        run_case("history:admission:reconcile:retry", colocated, Policy::Root);
    }
}

#[test]
fn jj_reconciliation_reply_loss_and_restart_do_not_refresh_a_stale_request() {
    run_case("history:admission:reconcile:restart", false, Policy::Root);
}

#[test]
fn jj_reconciliation_full_head_bound_and_reordered_expected_set_coalesce() {
    run_case("history:admission:reconcile:heads", true, Policy::Root);
}

#[test]
fn jj_reconciliation_unchanged_checks_saved_native_integrity_and_scope() {
    for kind in [
        "state_checksum",
        "packet_checksum",
        "registration",
        "native_packet",
        "scope",
    ] {
        run_case(
            &format!("history:admission:reconcile:corrupt:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_reconciliation_unchanged_refuses_gaps_rewind_and_selected_duplicates() {
    for kind in [
        "state_gap",
        "packet_gap",
        "both_erased",
        "higher",
        "duplicate",
    ] {
        run_case(
            &format!("history:admission:reconcile:gap:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_reconciliation_unchanged_uses_one_cumulative_sql_budget() {
    run_case("history:admission:reconcile:budget", true, Policy::Root);
}

#[test]
fn jj_reconciliation_refuses_missing_registration_expired_time_and_wrong_scope() {
    for kind in ["absent", "deadline", "expected"] {
        run_case(
            &format!("history:admission:reconcile:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_reconciliation_unchanged_keeps_full_repository_policy() {
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
        history_fixture_case(
            "history:admission:reconcile:policy_save_target",
            &mut fixture,
            Policy::Root,
        );
        history_fixture_case("history:admission:reconcile:policy", &mut fixture, policy);
    }
}

pub fn dispatch(case: &Case, config: &Config) {
    let name = case
        .name
        .strip_prefix("history:admission:reconcile:")
        .unwrap();
    match name {
        "zero" | "positive" => behavior::unchanged(case, config),
        "no_walk" => behavior::no_walk(case, config),
        "manual" => behavior::manual(case, config),
        "retry" => behavior::retry(case, config),
        "restart" => behavior::restart(case, config),
        "heads" => behavior::heads(case, config),
        "budget" => faults::budget(case, config),
        "absent" | "deadline" | "expected" => faults::early(case, config),
        "policy" => faults::policy(case, config),
        "policy_save_target" => faults::save_policy_target(case, config),
        name if name.starts_with("target:") => targets::dispatch(case, config),
        kind if kind.starts_with("corrupt:") => faults::corrupt(case, config),
        kind if kind.starts_with("gap:") => faults::gap(case, config),
        name if name.starts_with("real:") => real::dispatch(case, config),
        other => panic!("unknown reconciliation fixture {other}"),
    }
}
