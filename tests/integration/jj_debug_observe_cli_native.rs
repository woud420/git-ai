use super::*;
use serde::{Deserialize, Serialize};

#[path = "jj_debug_observe_cli_stream.rs"]
mod stream;
#[path = "jj_debug_observe_cli_native_support.rs"]
mod support;
use support::*;
#[path = "jj_debug_observe_cli_behavior.rs"]
mod behavior;
#[path = "jj_debug_observe_cli_errors.rs"]
mod errors;
#[path = "jj_debug_observe_cli_output_limit.rs"]
mod output_limit;
#[path = "jj_debug_observe_cli_real.rs"]
mod real;

#[test]
fn jj_debug_observe_cli_default_and_finite_unchanged_records_are_exact() {
    for colocated in [true, false] {
        observe_case("unchanged", colocated);
    }
}

#[test]
fn jj_debug_observe_cli_streams_change_then_carries_its_verified_cursor() {
    observe_case("changed", true);
}

#[test]
fn jj_debug_observe_cli_exact_retry_carries_current_cursor_without_repeating_admission() {
    observe_case("retry", false);
}

#[test]
fn jj_debug_observe_cli_fresh_file_policy_revocation_stops_after_visible_record() {
    observe_case("policy", false);
}

#[test]
fn jj_debug_observe_cli_late_cursor_or_source_conflict_is_not_automatically_refreshed() {
    for kind in ["cursor", "seal"] {
        observe_case(kind, true);
    }
}

#[test]
fn jj_debug_observe_cli_required_ancestor_loss_and_original_cutoff_overflow_stay_terminal() {
    for kind in ["missing_parent", "overflow"] {
        observe_case(kind, false);
    }
}

#[test]
fn jj_debug_observe_cli_initial_refusal_does_not_initialize_or_adopt_a_source() {
    for kind in ["unregistered", "malformed"] {
        observe_case(kind, true);
    }
}

#[test]
fn jj_debug_observe_cli_closed_stdout_terminates_without_recovery_record() {
    observe_case("closed_stdout", true);
}

fn observe_case(name: &str, colocated: bool) {
    let mut fixture = Fixture::new(colocated);
    fixture_case(&format!("observe:{name}"), &mut fixture, Policy::Root);
    let bytes = crate::debug_context::read_jj_fixture(
        &fixture
            .repo
            .test_home_path()
            .join("registration-child.stderr"),
        1024 * 1024,
    );
    for line in String::from_utf8_lossy(&bytes).lines() {
        if line.starts_with("OBSERVE_STREAM_") {
            eprintln!("OBSERVE_CASE:{name} colocated={colocated} {line}");
        }
    }
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    match case
        .name
        .strip_prefix("history:admission:cli:observe:")
        .unwrap()
    {
        "head_closure_output" => output_limit::run(case, config),
        "unchanged" => behavior::unchanged(case, config),
        "changed" => behavior::changed(case, config),
        "retry" => behavior::retry(case, config),
        "policy" => errors::policy(case, config),
        "cursor" | "seal" => errors::late_conflict(case, config),
        "missing_parent" | "overflow" => errors::history_limit(case, config),
        "unregistered" | "malformed" => errors::initial_refusal(case, config),
        "closed_stdout" => errors::closed_stdout(case, config),
        "real_install" => real::install_real(case, config),
        "real_verify" => real::verify_real(case, config),
        other => panic!("unknown observer fixture {other}"),
    }
}
