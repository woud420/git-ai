use super::*;
use git_ai::operations::jj::history::{
    CollectedJjHistory, JjHistoryCollectionError, collect_registered_history,
};

#[path = "jj_history_support.rs"]
mod support;
use support::{
    assert_result, checked, collect, failure, history_fixture_case, install, op_path, opaque,
    rejected, view_path, write_records,
};
#[path = "jj_history_behavior.rs"]
mod behavior;
#[path = "jj_history_bounds.rs"]
mod bounds;
#[path = "jj_history_faults.rs"]
mod faults;
// Public and private collector suites share independently generated fixtures.
#[allow(dead_code, unused_imports)]
#[path = "fixtures/jj-history/helpers.rs"]
mod native;
#[path = "jj_history_policy.rs"]
mod policy;
#[path = "jj_history_real.rs"]
mod real;

#[test]
fn jj_history_unregistered_and_empty_heads_are_unavailable_without_initialization() {
    run_case("history:unregistered", true, Policy::Root);
    run_case("history:empty_heads", true, Policy::Root);
}

#[test]
fn jj_history_exact_baseline_heads_are_terminals_without_parent_reads() {
    run_case("history:baseline", true, Policy::Root);
}

#[test]
fn jj_history_registered_two_step_chain_is_exact_and_parent_first() {
    for colocated in [true, false] {
        run_case("history:chain", colocated, Policy::Root);
    }
}

#[test]
fn jj_history_redundant_heads_and_merge_convergence_preserve_roots_without_duplicates() {
    run_case("history:redundant", true, Policy::Root);
}

#[test]
fn jj_history_late_branch_requires_complete_root_path_despite_opaque_membership() {
    run_case("history:late", true, Policy::Root);
}

#[test]
fn jj_history_mixed_baseline_and_root_closure_reports_both_boundaries() {
    run_case("history:mixed", true, Policy::Root);
}

#[test]
fn jj_history_unrelated_native_files_checkout_and_opaque_rows_do_not_seed_history() {
    run_case("history:unrelated", true, Policy::Root);
}

#[test]
fn jj_history_stale_checkout_enters_output_only_through_a_verified_parent() {
    run_case("history:stale", true, Policy::Root);
}

#[test]
fn jj_history_required_operation_and_view_gaps_reject_without_progress() {
    run_case("history:missing_operation", true, Policy::Root);
    run_case("history:missing_view", true, Policy::Root);
}

#[test]
fn jj_history_required_malformed_or_unsupported_native_records_reject() {
    run_case("history:malformed", true, Policy::Root);
    run_case("history:unsupported", true, Policy::Root);
}

#[test]
fn jj_history_each_registered_row_gap_or_checksum_corruption_is_unavailable() {
    for table in TABLES {
        for kind in ["gap", "checksum"] {
            run_case(&format!("history:row:{kind}:{table}"), true, Policy::Root);
        }
    }
}

#[test]
fn jj_history_seal_backend_and_untrusted_context_changes_are_unavailable() {
    for kind in ["missing_seal", "invalid_seal", "backend", "context"] {
        run_case(&format!("history:binding:{kind}"), true, Policy::Root);
    }
}

#[test]
fn jj_history_matching_namespace_only_baseline_is_never_adopted() {
    run_case("history:native_only", true, Policy::Root);
}

#[test]
fn jj_history_caller_sql_budget_remains_consumed_across_success_and_failure() {
    run_case("history:sql_budget", true, Policy::Root);
}

#[test]
fn jj_history_expired_deadline_cannot_return_partial_history() {
    run_case("history:deadline", true, Policy::Root);
}

#[test]
fn jj_history_policy_is_rechecked_for_an_existing_registration() {
    for policy in [
        Policy::Empty,
        Policy::OtherRoot,
        Policy::RootWithRemoteExclusion,
    ] {
        let mut fixture = Fixture::new(false);
        history_fixture_case("history:policy_install", &mut fixture, Policy::Root);
        history_fixture_case("history:policy_denied", &mut fixture, policy);
    }
}

#[test]
fn jj_history_empty_opt_in_rejects_even_the_debug_self_check_remote() {
    let mut fixture = Fixture::new(false);
    history_fixture_case("history:sentinel_install", &mut fixture, Policy::Root);
    history_fixture_case("history:policy_denied", &mut fixture, Policy::Empty);
}

#[test]
fn jj_history_canonical_parent_steps_cannot_bypass_rechecked_policy() {
    let mut fixture = Fixture::new(false);
    history_fixture_case("history:policy_install", &mut fixture, Policy::Root);
    history_fixture_case(
        "history:canonical_denied",
        &mut fixture,
        Policy::EscapedRoot,
    );
}

#[test]
fn jj_history_exact_and_one_over_pair_union_include_sampled_baseline_heads() {
    for kind in ["pairs", "pairs_with_baseline"] {
        run_case(&format!("history:bound:{kind}"), true, Policy::Root);
    }
}

#[test]
fn jj_history_exact_and_one_over_raw_union_charge_repeated_view_bytes() {
    for kind in ["raw", "raw_with_baseline"] {
        run_case(&format!("history:bound:{kind}"), true, Policy::Root);
    }
}

#[test]
fn jj_history_aggregate_semantic_limits_count_each_retained_input_occurrence() {
    for kind in ["predecessors", "views"] {
        run_case(&format!("history:bound:{kind}"), true, Policy::Root);
    }
}

#[test]
fn jj_history_exact_envelope_reaches_native_decode_and_one_over_hits_byte_limit() {
    run_case("history:envelope", true, Policy::Root);
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    match case.name.strip_prefix("history:").unwrap() {
        "unregistered" | "empty_heads" => faults::unregistered(case, config),
        "baseline" => behavior::baseline(case, config),
        "chain" => behavior::chain(case, config, false),
        "stale" => behavior::chain(case, config, true),
        "redundant" => behavior::redundant(case, config),
        "late" => behavior::late(case, config),
        "mixed" => behavior::mixed(case, config),
        "unrelated" => behavior::unrelated(case, config),
        "missing_operation" | "missing_view" | "malformed" | "unsupported" => {
            faults::evidence(case, config)
        }
        "native_only" => faults::native_only(case, config),
        "sql_budget" => faults::sql_budget(case, config),
        "deadline" => faults::expired(case, config),
        "envelope" => bounds::envelope(case, config),
        "policy_install" | "sentinel_install" => policy::install(case, config),
        "policy_denied" | "canonical_denied" => policy::denied(case, config),
        "real_install" => real::install(case, config),
        "real_collect" => real::collect(case, config),
        name if name.starts_with("row:") => faults::row(case, config),
        name if name.starts_with("binding:") => faults::binding(case, config),
        name if name.starts_with("bound:") => bounds::run(case, config),
        other => panic!("unknown history fixture {other}"),
    }
}
