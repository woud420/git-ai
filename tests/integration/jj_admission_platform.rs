#![cfg(not(any(target_os = "linux", target_os = "macos")))]

use crate::debug_context::snapshot;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::config::Config;
use git_ai::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use git_ai::model::repository::sqlite::open_with_memory_limits;
use git_ai::operations::jj::admission::{
    NativeAdmissionExpectation, admit_registered_history, read_native_admission,
    read_registered_admission_state,
};
use git_ai::operations::workspace_context::{GitPaths, WorkspaceContext};
use std::time::{Duration, Instant};

#[test]
fn jj_admission_unqualified_platform_rejects_all_apis_before_input_or_path_access() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let config = Config::fresh();
    let root = repo.canonical_path().join("admission-must-not-be-opened");
    let context = WorkspaceContext {
        schema_version: 0,
        capability: "invalid",
        vcs: "jj",
        workspace_root: root.clone(),
        git: GitPaths {
            git_dir: root.join("missing-git"),
            common_dir: root.join("missing-common"),
        },
        jj: None,
        colocated: false,
    };
    let path = repo.test_home_path().join("unsupported-admission.sqlite");
    let mut journal = JjObservationJournal::open_at_path(&path).unwrap();
    let before = snapshot(repo.path());
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    let config_bytes = std::fs::read(&config_path).ok();
    let mut reads = ReadBudget::new(0);
    let until = Instant::now() + Duration::from_secs(10);
    let errors = [
        read_registered_admission_state(&journal, &context, &config, until, &mut reads)
            .err()
            .unwrap(),
        admit_registered_history(
            &mut journal,
            &context,
            &config,
            NativeAdmissionExpectation {
                source_id: "invalid",
                initialization_receipt_id: "invalid",
                baseline_id: "invalid",
                generation: u64::MAX,
                admitted_head_ids: &[],
            },
            until,
            &mut reads,
        )
        .err()
        .unwrap(),
        read_native_admission(&journal, "invalid", "invalid", until, &mut reads)
            .err()
            .unwrap(),
    ];
    for error in errors {
        assert!(error.to_string().contains("unsupported"), "{error}");
    }
    assert_eq!(reads.consumed(), 0);
    assert_eq!(snapshot(repo.path()), before);
    assert_eq!(std::fs::read(config_path).ok(), config_bytes);
    assert!(!root.exists());
    let conn = open_with_memory_limits(&path).unwrap();
    for table in [
        "jj_native_admissions",
        "jj_native_admission_states",
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
        assert_eq!(count, 0, "unsupported admission wrote {table}");
    }
}
