use super::support::checked;
use super::*;

#[path = "jj_debug_write_cli_support.rs"]
mod support;
use support::*;
#[path = "jj_debug_write_cli_behavior.rs"]
mod behavior;
#[path = "jj_debug_write_cli_errors.rs"]
mod errors;
#[path = "jj_head_closure_consumers.rs"]
mod head_closures;
#[path = "jj_debug_write_cli_real.rs"]
mod real;

#[test]
fn jj_debug_write_cli_initialize_installs_and_preserves_original_cutoff_on_retry() {
    for colocated in [true, false] {
        cli_case("write:initialize", colocated, Policy::Root);
    }
}

#[test]
fn jj_debug_write_cli_capture_baseline_later_history_retry_and_cas() {
    cli_case("write:capture", true, Policy::Root);
}

#[test]
fn jj_debug_write_cli_reordered_expected_heads_return_exact_receipt() {
    cli_case("write:head_order", false, Policy::Root);
}

#[test]
fn jj_debug_write_cli_early_gates_and_sqlite_special_names_do_not_open_storage() {
    cli_case("write:empty", true, Policy::Empty);
    for case in ["git", "malformed", "paths"] {
        cli_case(&format!("write:{case}"), true, Policy::Root);
    }
}

#[test]
fn jj_debug_write_cli_denied_nonempty_policy_leaves_initialized_empty_journal() {
    cli_case("write:denied", false, Policy::OtherRoot);
}

#[test]
fn jj_debug_write_cli_valid_expectation_bounds_reach_unregistered_source_refusal() {
    cli_case("write:valid_bounds", true, Policy::Root);
}

#[test]
fn jj_debug_write_cli_partial_registration_and_namespace_only_state_are_not_adopted() {
    for case in ["directory_only", "native_only", "seal_only"] {
        cli_case(&format!("write:{case}"), true, Policy::Root);
    }
}

#[test]
fn jj_debug_write_cli_registration_sql_failure_retains_unavailable_seal_without_cleanup() {
    cli_case("write:init_abort", true, Policy::Root);
}

#[test]
fn jj_debug_write_cli_capture_sql_and_native_failures_preserve_state() {
    for case in ["capture_abort", "native_packet"] {
        cli_case(&format!("write:{case}"), true, Policy::Root);
    }
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    match case
        .name
        .strip_prefix("history:admission:cli:write:")
        .unwrap()
    {
        name if name.starts_with("head_closures:") => head_closures::dispatch(case, config),
        "initialize" => behavior::initialize_case(case, config),
        "capture" => behavior::capture_case(case, config),
        "head_order" => behavior::head_order(case, config),
        "empty" | "git" | "malformed" | "paths" => errors::early(case),
        "denied" | "valid_bounds" => errors::created_empty(case),
        "directory_only" | "native_only" | "seal_only" => errors::incomplete(case, config),
        "init_abort" => errors::init_abort(case),
        "capture_abort" | "native_packet" => errors::capture_failure(case, config),
        "real_initialize" => real::initialize(case, config),
        "real_capture" => real::capture(case, config),
        other => panic!("unknown write CLI case {other}"),
    }
}
