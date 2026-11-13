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

#[tokio::test]
async fn mv_is_ordered_without_collecting_reflog_offsets() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    seed_reflog(&target);
    let coord = ActorDaemonCoordinator::new();
    let mut start = json!({
        "event": "start", "sid": "mv-root",
        "argv": ["git", "-C", target, "mv", "--", "nested/file.txt", "."],
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut start));
    let mut primary_repo = json!({
        "event": "def_repo", "sid": "mv-root", "repo": 1, "worktree": target,
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut primary_repo));
    assert!(
        primary_repo
            .get(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD)
            .is_none()
    );
    let ingress = coord.trace_ingress_state.lock().unwrap();
    assert_eq!(ingress.root_mutating.get("mv-root"), Some(&true));
    assert!(!ingress.root_reflog_start_offsets.contains_key("mv-root"));
}

#[tokio::test]
async fn read_only_roots_do_not_resolve_their_repository() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    seed_reflog(&target);
    let coord = ActorDaemonCoordinator::new();
    let frames = |sid: &str, command: &str| {
        [
            json!({"event": "start", "sid": sid, "argv": ["git", "-C", target, command]}),
            json!({"event": "def_repo", "sid": sid, "repo": 1, "worktree": target}),
        ]
    };
    // An IDE-style status names its repository on both frames; neither is
    // queued and nothing reads a read-only root's repository context.
    for mut frame in frames("reader", "status") {
        let received = frame.clone();
        assert!(!coord.prepare_trace_payload_for_ingest(&mut frame));
        assert_eq!(frame, received, "a dropped frame is left as received");
    }
    let [mut start, mut def_repo] = frames("writer", "commit");
    assert!(coord.prepare_trace_payload_for_ingest(&mut start));
    assert!(coord.prepare_trace_payload_for_ingest(&mut def_repo));
    // A queued frame inherits the root's argv unless it carries its own.
    assert!(start.get(TRACE_ROOT_ARGV_FIELD).is_none());
    assert_eq!(def_repo[TRACE_ROOT_ARGV_FIELD], start["argv"]);
    assert!(def_repo[TRACE_ROOT_STARTED_AT_NS_FIELD].is_u64());
    let ingress = coord.trace_ingress_state.lock().unwrap();
    assert!(ingress.root_definitely_read_only.contains("reader"));
    assert!(!ingress.root_families.contains_key("reader"));
    assert!(!ingress.root_worktrees.contains_key("reader"));
    let family = target.join(".git").canonicalize().unwrap();
    assert_eq!(
        ingress.root_families.get("writer").map(String::as_str),
        family.to_str()
    );
    assert_eq!(ingress.root_worktrees.get("writer"), Some(&target));
}
