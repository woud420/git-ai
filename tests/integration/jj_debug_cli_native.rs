use super::*;
use crate::repos::test_repo::get_binary_path;
use serde_json::{Value as Json, json};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

#[path = "jj_debug_cli_support.rs"]
mod support;
use support::{checked, *};
#[path = "jj_debug_cli_behavior.rs"]
mod behavior;
#[path = "jj_debug_cli_errors.rs"]
mod errors;
#[path = "jj_debug_observe_cli_native.rs"]
mod observe;
#[path = "jj_debug_write_cli_native.rs"]
mod write;

fn cli_case(name: &str, colocated: bool, policy: Policy) {
    let mut fixture = Fixture::new(colocated);
    fixture_case(name, &mut fixture, policy);
}

fn fixture_case(name: &str, fixture: &mut Fixture, policy: Policy) {
    fs::write(
        fixture.repo.test_home_path().join("jj-cli-binary.json"),
        serde_json::to_vec(get_binary_path()).unwrap(),
    )
    .unwrap();
    history_fixture_case(&format!("history:admission:cli:{name}"), fixture, policy);
}

#[test]
fn jj_debug_cli_status_zero_latest_and_outside_checkout_match_existing_api() {
    for colocated in [true, false] {
        cli_case("status", colocated, Policy::Root);
    }
}

#[test]
fn jj_debug_cli_receipt_distinct_latest_and_all_zero_absence_are_historical() {
    cli_case("receipts", true, Policy::Root);
}

#[test]
fn jj_debug_cli_receipt_ignores_current_layout_and_policy_configuration() {
    cli_case("historical", false, Policy::Root);
}

#[test]
fn jj_debug_cli_corrupt_native_packet_with_repaired_storage_hashes_is_unavailable() {
    cli_case("native_packet", true, Policy::Root);
}

#[test]
fn jj_debug_cli_empty_opt_in_precedes_discovery_and_journal_creation() {
    cli_case("empty", true, Policy::Empty);
}

#[test]
fn jj_debug_cli_status_rechecks_nonempty_collection_policy() {
    let mut fixture = Fixture::new(false);
    fixture_case("policy_install", &mut fixture, Policy::Root);
    fixture_case("policy_denied", &mut fixture, Policy::OtherRoot);
}

#[test]
fn jj_debug_cli_context_errors_and_git_workspace_do_not_create_a_journal() {
    for kind in ["git", "malformed"] {
        cli_case(kind, true, Policy::Root);
    }
}

#[test]
fn jj_debug_cli_missing_journal_is_explicit_and_never_initialized() {
    cli_case("missing", true, Policy::Root);
}

#[test]
fn jj_debug_cli_unregistered_source_is_not_implicitly_initialized() {
    cli_case("unregistered", true, Policy::Root);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_debug_cli_real_pinned_jj_status_and_baseline_receipt_are_read_only() {
    use crate::debug_context::{jj, require_pinned_jj};
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    jj(&repo, repo.path(), &["git", "init", "--colocate"]);
    jj(
        &repo,
        repo.path(),
        &["describe", "-m", "debug CLI baseline"],
    );
    let root = repo.path().to_owned();
    fs::write(root.join("dirty-cli.txt"), b"not snapshotted by CLI\n").unwrap();
    let mut fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    fixture_case("real", &mut fixture, Policy::Root);
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    match case.name.strip_prefix("history:admission:cli:").unwrap() {
        name if name.starts_with("write:") => write::dispatch(case, config),
        name if name.starts_with("observe:") => observe::dispatch(case, config),
        "status" => behavior::status_case(case, config),
        "receipts" => behavior::receipts(case, config, false),
        "historical" => behavior::receipts(case, config, true),
        "real" => behavior::real(case, config),
        "native_packet" => errors::native(case, config),
        "policy_install" => {
            let _ = initial(case, config);
        }
        "policy_denied" => errors::policy(case),
        "empty" | "git" | "malformed" | "missing" => errors::before_open(case),
        "unregistered" => errors::unregistered(case, config),
        other => panic!("unknown debug CLI fixture {other}"),
    }
}
