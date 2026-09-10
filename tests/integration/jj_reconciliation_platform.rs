#![cfg(not(any(target_os = "linux", target_os = "macos")))]

use crate::debug_context::snapshot;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::config::Config;
use git_ai::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use git_ai::model::repository::sqlite::open_with_memory_limits;
use git_ai::operations::jj::admission::{
    NativeAdmissionExpectation, NativeReconciliationExpectation, reconcile_registered_history,
};
use git_ai::operations::workspace_context::{GitPaths, WorkspaceContext};
use std::time::{Duration, Instant};

#[test]
fn jj_reconciliation_unqualified_platform_rejects_before_paths_or_policy_access() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let config = Config::fresh();
    let root = repo.canonical_path().join("history-must-not-be-opened");
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
    let path = repo.test_home_path().join("unsupported-history.sqlite");
    let mut journal = JjObservationJournal::open_at_path(&path).unwrap();
    let before = snapshot(repo.path());
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    let config_bytes = std::fs::read(&config_path).ok();
    let mut budget = ReadBudget::new(0);
    let source = "ab".repeat(32);
    let heads = ["01".repeat(64)];
    let expected = NativeAdmissionExpectation {
        source_id: &source,
        initialization_receipt_id: &source,
        baseline_id: &source,
        generation: 0,
        admitted_head_ids: &heads,
    };
    let error = match reconcile_registered_history(
        &mut journal,
        &context,
        &config,
        NativeReconciliationExpectation {
            admission: expected,
            workspace_name: "",
            attachment_id: "malformed",
        },
        Instant::now() + Duration::from_secs(10),
        &mut budget,
    ) {
        Ok(_) => panic!("unqualified platform reconciled history"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("unsupported"), "{error}");
    assert_eq!(budget.consumed(), 0);
    assert_eq!(snapshot(repo.path()), before);
    assert_eq!(std::fs::read(config_path).ok(), config_bytes);
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
        "jj_native_admissions",
        "jj_native_admission_states",
    ] {
        let count: usize = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "unsupported reconciler wrote {table}");
    }
}
