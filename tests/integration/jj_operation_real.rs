use super::*;
use crate::debug_context::{jj, require_pinned_jj, snapshot};
use std::fs;

fn qualify_operation_store(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("jj operation fixture");
        jj(
            &repo,
            repo.path(),
            &["git", "init", "--no-colocate", root.to_str().unwrap()],
        );
        root
    };
    fs::write(root.join("example.txt"), "fixture line\n").unwrap();
    jj(&repo, &root, &["status"]);
    jj(&repo, &root, &["commit", "-m", "synthetic decoder fixture"]);
    assert_eq!(
        fs::read_to_string(root.join("example.txt")).unwrap(),
        "fixture line\n"
    );
    jj(
        &repo,
        &root,
        &["describe", "-m", "synthetic operation rewrite"],
    );
    assert_eq!(
        fs::read_to_string(root.join("example.txt")).unwrap(),
        "fixture line\n"
    );
    fs::write(root.join("unsnapshotted.txt"), "left dirty during decode\n").unwrap();

    let before = snapshot(repo.path());
    let operations = root.join(".jj/repo/op_store/operations");
    let files: Vec<_> = fs::read_dir(operations)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        (3..=16).contains(&files.len()),
        "unexpected fixture operation count"
    );
    let mut saw_snapshot = false;
    let mut saw_predecessor = false;
    let mut saw_root_parent = false;
    for path in files {
        let id = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(id.len(), 128);
        assert!(fs::metadata(&path).unwrap().len() <= MAX_OPERATION_BYTES as u64);
        let raw = fs::read(&path).unwrap();
        let decoded = decode_operation(JJ_OBSERVATION_READER_PROFILE, id, &raw).unwrap();
        assert_eq!(decoded.operation_id, id);
        assert_eq!(decoded.view_id.len(), 128);
        assert!(!decoded.parent_ids.is_empty());
        saw_snapshot |= decoded.is_snapshot;
        saw_root_parent |= decoded.parent_ids.iter().any(|id| id == &"00".repeat(64));
        let predecessors = decoded
            .commit_predecessors
            .expect("jj 0.45.1 records predecessors");
        for (commit, predecessors) in predecessors {
            assert_eq!(commit.len(), 40);
            saw_predecessor |= !predecessors.is_empty();
            assert!(predecessors.iter().all(|id| id.len() == 40));
        }
    }
    assert!(saw_snapshot);
    assert!(saw_predecessor);
    assert!(saw_root_parent);
    assert_eq!(snapshot(repo.path()), before);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_operation_real_colocated_store_qualifies_without_decoder_writes() {
    qualify_operation_store(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_operation_real_noncolocated_store_qualifies_without_decoder_writes() {
    qualify_operation_store(false);
}
