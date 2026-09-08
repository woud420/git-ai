#![cfg(not(any(target_os = "linux", target_os = "macos")))]

use crate::debug_context::snapshot;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::config::Config;
use git_ai::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use git_ai::model::repository::sqlite::open_with_memory_limits;
use git_ai::operations::jj::registration::{
    register_current_state, reopen_registered_current_state,
};
use git_ai::operations::workspace_context::{GitPaths, WorkspaceContext};
use std::time::{Duration, Instant};

#[test]
fn jj_registration_unqualified_platform_rejects_before_context_or_policy_access() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    // TestRepo initializes the existing isolated process HOME; no new env writes
    // are needed because unsupported-platform rejection cannot depend on policy.
    let config = Config::fresh();
    let root = repo.canonical_path().join("must-not-be-opened");
    let context = WorkspaceContext {
        schema_version: 1,
        capability: "discovery_only",
        vcs: "jj",
        workspace_root: root.clone(),
        git: GitPaths {
            git_dir: root.join("missing-git"),
            common_dir: root.join("missing-common"),
        },
        jj: None,
        colocated: false,
    };
    let path = repo
        .test_home_path()
        .join("unsupported-registration.sqlite");
    let mut journal = JjObservationJournal::open_at_path(&path).unwrap();
    let before = snapshot(repo.path());
    let config_bytes = std::fs::read(repo.test_home_path().join(".git-ai/config.json")).ok();
    let until = Instant::now() + Duration::from_secs(10);
    let mut budget = ReadBudget::new(0);
    let register = match register_current_state(&mut journal, &context, &config, until, &mut budget)
    {
        Ok(_) => panic!("unqualified platform installed or reopened registration"),
        Err(error) => error,
    };
    assert!(register.to_string().contains("unsupported"), "{register}");
    let reopen =
        match reopen_registered_current_state(&journal, &context, &config, until, &mut budget) {
            Ok(_) => panic!("unqualified platform performed registered lookup"),
            Err(error) => error,
        };
    assert!(reopen.to_string().contains("unsupported"), "{reopen}");
    assert_eq!(budget.consumed(), 0);
    assert_eq!(snapshot(repo.path()), before);
    assert_eq!(
        std::fs::read(repo.test_home_path().join(".git-ai/config.json")).ok(),
        config_bytes
    );
    assert!(!root.exists());
    let conn = open_with_memory_limits(&path).unwrap();
    for table in [
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
        "jj_sources",
        "jj_operations",
        "jj_views",
        "jj_batches",
    ] {
        let count: usize = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "unsupported platform wrote {table}");
    }
    let version: String = conn
        .query_row(
            "SELECT value FROM schema_metadata WHERE key = 'version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "3");
}
