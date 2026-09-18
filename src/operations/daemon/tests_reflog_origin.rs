use super::*;
use serde_json::json;
use std::path::Path;

fn seed_reflog(worktree: &Path) -> String {
    let git_dir = worktree.join(".git");
    crate::operations::git::test_utils::seed_valid_git_dir(&git_dir);
    std::fs::create_dir_all(git_dir.join("logs")).unwrap();
    std::fs::write(git_dir.join("logs/HEAD"), "existing reflog entry\n").unwrap();
    format!(
        "worktree:{}:HEAD",
        git_dir.canonicalize().unwrap().display()
    )
}

#[tokio::test]
async fn reflog_capture_ignores_start_hints_that_contradict_the_root_repository() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    let expected_key = seed_reflog(&target);
    let decoy = temp.path().join("decoy");
    seed_reflog(&decoy);
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    for hint in [&decoy, &outside] {
        let coord = ActorDaemonCoordinator::new();
        let mut start = json!({
            "event": "start", "sid": "root",
            "argv": ["git", "-C", hint, "commit", "-m", "next"],
        });
        assert!(coord.prepare_trace_payload_for_ingest(&mut start));
        assert!(start.get(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD).is_none());
        let mut primary_repo = json!({
            "event": "def_repo", "sid": "root", "repo": 1, "worktree": target,
        });
        assert!(coord.prepare_trace_payload_for_ingest(&mut primary_repo));
        let offsets = primary_repo[TRACE_ROOT_REFLOG_START_OFFSETS_FIELD]
            .as_object()
            .expect("root repository should supply the snapshot");
        assert_eq!(offsets.len(), 1);
        assert_eq!(offsets[&expected_key], 22);
    }
}

#[tokio::test]
async fn child_and_secondary_repositories_cannot_supply_root_reflog_offsets() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    let expected_key = seed_reflog(&target);
    let decoy = temp.path().join("decoy");
    seed_reflog(&decoy);
    for (sid, index) in [("root/child", 1), ("root", 2)] {
        let coord = ActorDaemonCoordinator::new();
        let mut start = json!({
            "event": "start", "sid": "root", "argv": ["git", "commit", "-m", "next"],
        });
        assert!(coord.prepare_trace_payload_for_ingest(&mut start));
        let mut unrelated_repo = json!({
            "event": "def_repo", "sid": sid, "repo": index, "worktree": decoy,
        });
        assert!(coord.prepare_trace_payload_for_ingest(&mut unrelated_repo));
        assert!(
            unrelated_repo
                .get(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD)
                .is_none()
        );
        let mut primary_repo = json!({
            "event": "def_repo", "sid": "root", "repo": 1, "worktree": target,
        });
        assert!(coord.prepare_trace_payload_for_ingest(&mut primary_repo));
        let offsets = primary_repo[TRACE_ROOT_REFLOG_START_OFFSETS_FIELD]
            .as_object()
            .unwrap();
        assert_eq!(offsets.len(), 1);
        assert_eq!(offsets[&expected_key], 22);
        std::fs::write(target.join(".git/logs/HEAD"), "later entry\n").unwrap();
        primary_repo
            .as_object_mut()
            .unwrap()
            .remove(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD);
        assert!(coord.prepare_trace_payload_for_ingest(&mut primary_repo));
        assert_eq!(
            primary_repo[TRACE_ROOT_REFLOG_START_OFFSETS_FIELD][&expected_key],
            22
        );
        seed_reflog(&target);
    }
}

#[tokio::test]
async fn restore_is_ordered_without_collecting_reflog_offsets() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    seed_reflog(&target);
    let coord = ActorDaemonCoordinator::new();
    let mut start = json!({
        "event": "start", "sid": "restore-root",
        "argv": ["git", "restore", "--source", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "--worktree", "--", "file.txt"],
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut start));
    let mut primary_repo = json!({
        "event": "def_repo", "sid": "restore-root", "repo": 1, "worktree": target,
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut primary_repo));
    assert!(
        primary_repo
            .get(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD)
            .is_none()
    );
    let ingress = coord.trace_ingress_state.lock().unwrap();
    assert_eq!(ingress.root_mutating.get("restore-root"), Some(&true));
    assert!(
        !ingress
            .root_reflog_start_offsets
            .contains_key("restore-root")
    );
}
