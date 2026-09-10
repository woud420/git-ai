use super::*;
use git_ai::config::Config;
use git_ai::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use git_ai::model::repository::sqlite::open_with_memory_limits;
use git_ai::operations::jj::baseline_persistence::{
    BaselinePersistenceOutcome, persist_current_state_baseline,
};
use git_ai::operations::jj::registration::{
    JjRegisteredCheckoutRelation, JjRegistrationError, JjRegistrationOutcome,
    RegisteredJjCurrentState, register_current_state, reopen_registered_current_state,
};

#[path = "jj_registration_support.rs"]
mod support;
use support::*;
#[path = "jj_registration_behavior.rs"]
mod behavior;
#[path = "jj_registration_faults.rs"]
mod faults;
#[path = "jj_history.rs"]
mod history;
#[path = "jj_registration_policy.rs"]
mod policy;
#[path = "jj_registration_real.rs"]
mod real;
#[path = "jj_registration_vectors.rs"]
mod vectors;

#[test]
fn jj_registration_first_colocated_install_joins_four_rows_and_native_receipt() {
    run_case("install", true, Policy::Root);
}

#[test]
fn jj_registration_first_noncolocated_install_uses_jj_workspace_policy_root() {
    run_case("install", false, Policy::Root);
}

#[test]
fn jj_registration_reopen_absence_is_read_only_and_does_not_initialize() {
    run_case("absent", true, Policy::Root);
}

#[test]
fn jj_registration_exact_retry_returns_identical_saved_registration() {
    run_case("retry", true, Policy::Root);
}

#[test]
fn jj_registration_advanced_heads_and_checkout_keep_original_cutoff_on_retry() {
    run_case("advance", true, Policy::Root);
}

#[test]
fn jj_registration_stale_checkout_is_outside_saved_baseline() {
    run_case("outside", true, Policy::Root);
}

#[test]
fn jj_registration_cannot_adopt_matching_namespace_only_baseline() {
    run_case("native_only", true, Policy::Root);
}

#[test]
fn jj_registration_each_required_row_gap_is_unavailable_without_repair() {
    for table in TABLES {
        run_case(&format!("row_gap:{table}"), true, Policy::Root);
    }
}

#[test]
fn jj_registration_missing_seal_with_occupied_guard_is_unavailable() {
    run_case("missing_seal", true, Policy::Root);
}

#[test]
fn jj_registration_invalid_seal_is_rejected_without_replacement() {
    run_case("invalid_seal", true, Policy::Root);
}

#[test]
fn jj_registration_seal_only_state_is_never_adopted() {
    run_case("seal_only", true, Policy::Root);
}

#[test]
fn jj_registration_preexisting_empty_namespace_is_unavailable() {
    run_case("directory_only", true, Policy::Root);
}

#[test]
fn jj_registration_empty_or_unmatched_allowlist_cannot_publish() {
    run_case("policy_denied", false, Policy::Empty);
    run_case("policy_denied", false, Policy::OtherRoot);
}

#[test]
fn jj_registration_self_check_remote_cannot_override_empty_opt_in() {
    run_case("sentinel_empty", false, Policy::Empty);
}

#[test]
fn jj_registration_remote_exclusion_overrides_allowed_workspace_path() {
    run_case("remote_control", false, Policy::Root);
    run_case("remote_denied", false, Policy::RootWithRemoteExclusion);
}

#[test]
fn jj_registration_allowed_backing_remote_can_authorize_noncolocated_workspace() {
    run_case("remote_control", false, Policy::Remote);
}

#[test]
fn jj_registration_canonical_parent_steps_cannot_escape_root_policy() {
    run_case("canonical_control", false, Policy::Root);
    run_case("canonical_denied", false, Policy::EscapedRoot);
}

#[test]
fn jj_registration_malformed_included_policy_cannot_hide_excluded_remote() {
    run_case("include_control", false, Policy::RootWithRemoteExclusion);
    run_case("include_denied", false, Policy::RootWithRemoteExclusion);
}

#[test]
fn jj_registration_abort_at_each_insert_rolls_back_all_four_sql_rows() {
    for table in TABLES {
        run_case(&format!("sql_abort:{table}"), true, Policy::Root);
    }
}

#[test]
#[ignore = "isolated Config::fresh helper; invoked by the public registration tests"]
fn registration_case_child() {
    let Some(case) = child_case() else {
        return;
    };
    let config = case.config();
    match case.name.as_str() {
        "install" => behavior::install(&case, &config),
        "absent" => behavior::absent(&case, &config),
        "retry" => behavior::retry(&case, &config),
        "advance" => behavior::advance(&case, &config),
        "outside" => behavior::outside(&case, &config),
        "native_only" => faults::native_only(&case, &config),
        "missing_seal" => faults::missing_seal(&case, &config),
        "invalid_seal" => faults::invalid_seal(&case, &config),
        "seal_only" => faults::seal_only(&case, &config),
        "directory_only" => faults::directory_only(&case, &config),
        "policy_denied" => policy::denied(&case, &config),
        "sentinel_empty" => policy::sentinel_empty(&case, &config),
        "remote_control" => policy::remote(&case, &config, false),
        "remote_denied" => policy::remote(&case, &config, true),
        "canonical_control" => policy::canonical(&case, &config, false),
        "canonical_denied" => policy::canonical(&case, &config, true),
        "include_control" => policy::included(&case, &config, false),
        "include_denied" => policy::included(&case, &config, true),
        name if name.starts_with("row_gap:") => faults::row_gap(&case, &config),
        name if name.starts_with("sql_abort:") => faults::sql_abort(&case, &config),
        name if name.starts_with("history:") => history::dispatch(&case, &config),
        name => panic!("unknown registration fixture {name}"),
    }
    println!("REGISTRATION_CASE_COMPLETED:{}", case.name);
}
