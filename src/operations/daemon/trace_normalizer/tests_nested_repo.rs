use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::TraceNormalizer;
use super::tests_lifecycle::{MockBackend, atexit_payload};

#[test]
fn normalizer_keeps_the_primary_worktree_after_a_secondary_def_repo() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let parent = temp.path().join("parent");
    let nested = parent.join("nested");
    crate::operations::git::test_utils::seed_valid_git_dir(&parent.join(".git"));
    crate::operations::git::test_utils::seed_valid_git_dir(&nested.join(".git"));

    let mut normalizer = TraceNormalizer::new(Arc::new(MockBackend::default()));
    let start = serde_json::json!({
        "event": "start",
        "sid": "nested-def-repo",
        "ts": 1,
        "argv": ["git", "commit", "-m", "message"],
        "worktree": parent,
    });
    let primary = serde_json::json!({
        "event": "def_repo",
        "sid": "nested-def-repo",
        "ts": 2,
        "repo": 1,
        "worktree": parent,
    });
    let mut secondary = serde_json::json!({
        "event": "def_repo",
        "sid": "nested-def-repo",
        "ts": 3,
        "repo": 2,
        "worktree": nested,
    });
    secondary.as_object_mut().unwrap().insert(
        crate::operations::daemon::TRACE_ROOT_REFLOG_START_OFFSETS_FIELD.to_string(),
        serde_json::json!({"HEAD": 42_u64}),
    );

    assert!(normalizer.ingest_payload(&start).unwrap().is_none());
    assert!(normalizer.ingest_payload(&primary).unwrap().is_none());
    assert!(normalizer.ingest_payload(&secondary).unwrap().is_none());
    let command = normalizer
        .ingest_payload(&atexit_payload("nested-def-repo", 4))
        .unwrap()
        .expect("command should normalize");

    assert_eq!(command.worktree.as_deref(), Some(parent.as_path()));
    assert_eq!(command.reflog_start_offsets.get("HEAD"), Some(&42));
}

const ROOT_SID: &str = "20261013T000000.000000Z-Hdeadbeef-P00010000";
const CHILD_SID: &str =
    "20261013T000000.000000Z-Hdeadbeef-P00010000/20261013T000000.000001Z-Hdeadbeef-P00010001";

fn local_and_source_repos() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("create tempdir");
    let local = temp.path().join("local");
    let source = temp.path().join("source");
    crate::operations::git::test_utils::seed_valid_git_dir(&local.join(".git"));
    crate::operations::git::test_utils::seed_valid_git_dir(&source.join(".git"));
    (temp, local, source)
}

fn def_repo(sid: &str, ts: u64, worktree: &Path) -> serde_json::Value {
    serde_json::json!({"event": "def_repo", "sid": sid, "ts": ts, "repo": 1, "worktree": worktree})
}

fn cmd_name(sid: &str, ts: u64, name: &str) -> serde_json::Value {
    serde_json::json!({"event": "cmd_name", "sid": sid, "ts": ts, "name": name})
}

fn normalized_worktree(frames: &[&serde_json::Value]) -> Option<PathBuf> {
    let mut normalizer = TraceNormalizer::new(Arc::new(MockBackend::default()));
    for frame in frames {
        assert!(normalizer.ingest_payload(frame).unwrap().is_none());
    }
    normalizer
        .ingest_payload(&atexit_payload(ROOT_SID, 9))
        .unwrap()
        .expect("command should normalize")
        .worktree
}

#[test]
fn normalizer_keeps_the_root_worktree_after_a_child_def_repo_in_another_repo() {
    // `git -C local fetch-pack --all <source>` runs upload-pack and its
    // pack-objects in the source repository. Their def_repo frames share the
    // root sid but arrive on other connections, before or after the root's.
    let (_temp, local, source) = local_and_source_repos();
    let start = serde_json::json!({
        "event": "start",
        "sid": ROOT_SID,
        "ts": 1,
        "argv": ["git", "-C", local, "fetch-pack", "--all", source],
    });
    let root_def_repo = def_repo(ROOT_SID, 2, &local);
    let root_cmd_name = cmd_name(ROOT_SID, 3, "fetch-pack");
    let child_def_repo = def_repo(CHILD_SID, 4, &source.join(".git"));

    for frames in [
        [&start, &root_def_repo, &root_cmd_name, &child_def_repo],
        [&start, &child_def_repo, &root_def_repo, &root_cmd_name],
    ] {
        assert_eq!(normalized_worktree(&frames), Some(local.clone()));
    }
}

#[test]
fn normalizer_uses_the_child_worktree_for_a_shell_alias_launcher() {
    // `git -C local alias` with `alias = !git -C <source> commit` reports the
    // root's repository, then launches the real command in the source.
    let (_temp, local, source) = local_and_source_repos();
    let start = serde_json::json!({
        "event": "start",
        "sid": ROOT_SID,
        "ts": 1,
        "argv": ["git", "-C", local, "alias"],
    });
    let frames = [
        &start,
        &cmd_name(ROOT_SID, 2, "_run_dashed_"),
        &def_repo(ROOT_SID, 3, &local),
        &cmd_name(ROOT_SID, 4, "_run_shell_alias_"),
        &def_repo(CHILD_SID, 5, &source),
        &cmd_name(CHILD_SID, 6, "commit"),
    ];

    assert_eq!(normalized_worktree(&frames), Some(source));
}
