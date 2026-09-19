use super::*;

#[tokio::test]
async fn fetch_pack_sync_sequences_without_ingress_reflog_reads() {
    let coordinator = ActorDaemonCoordinator::new();
    let directory = tempfile::tempdir().unwrap();
    let git_dir = directory.path().join(".git");
    crate::operations::git::test_utils::seed_valid_git_dir(&git_dir);
    std::fs::create_dir_all(git_dir.join("logs")).unwrap();
    std::fs::write(git_dir.join("logs/HEAD"), "existing log\n").unwrap();
    let sid = "fetch-pack-ingress";
    for mut payload in [
        serde_json::json!({"event":"start", "sid":sid, "argv":["git", "fetch-pack", "--all", "/source"], "worktree":directory.path()}),
        serde_json::json!({"event":"def_repo", "sid":sid, "repo":1, "worktree":directory.path()}),
    ] {
        assert!(coordinator.prepare_trace_payload_for_ingest(&mut payload));
        assert!(payload.get(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD).is_none());
    }
    assert_eq!(
        coordinator
            .trace_ingress_state
            .lock()
            .unwrap()
            .root_mutating
            .get(sid),
        Some(&true)
    );
    assert!(crate::operations::git::command_classification::git_invocation_participates_in_family_sequencer(
        "fetch-pack", &["--all".into(), "/source".into()]
    ));
}
