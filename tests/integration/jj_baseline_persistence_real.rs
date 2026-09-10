use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj, snapshot};
use git_ai::operations::jj::operation::{MAX_OPERATION_BYTES, decode_operation};
use git_ai::operations::jj::view::MAX_VIEW_BYTES;
use std::fs;

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_baseline_persistence_real_reopen_preserves_native_cutoff_and_dirty_workspace() {
    require_pinned_jj();
    let fixture = Fixture::new();
    let repo = &fixture.repo;
    jj(repo, repo.path(), &["git", "init", "--colocate"]);
    jj(repo, repo.path(), &["describe", "-m", "older operation"]);
    jj(repo, repo.path(), &["describe", "-m", "selected boundary"]);
    fs::write(repo.path().join("unsnapshotted.txt"), b"leave dirty\n").unwrap();
    let store = repo.path().join(".jj/repo/op_store");
    let entries: Vec<_> = fs::read_dir(repo.path().join(".jj/repo/op_heads/heads"))
        .unwrap()
        .take(2)
        .map(Result::unwrap)
        .collect();
    assert_eq!(entries.len(), 1);
    let id = entries[0].file_name().to_str().unwrap().to_owned();
    let raw = read_jj_fixture(&store.join("operations").join(&id), MAX_OPERATION_BYTES);
    let operation = decode_operation(JJ_OBSERVATION_READER_PROFILE, &id, &raw).unwrap();
    assert!(!operation.parent_ids.is_empty());
    assert!(operation.parent_ids.iter().all(|id| id != &"00".repeat(64)));
    let anchors = [JjOperationEvidence {
        operation_id: id,
        parent_ids: operation.parent_ids,
        view_bytes: read_jj_fixture(
            &store.join("views").join(&operation.view_id),
            MAX_VIEW_BYTES,
        ),
        view_id: operation.view_id,
        operation_bytes: raw,
    }];
    let before = snapshot(repo.path());
    let mut journal = fixture.open();
    let receipt = installed(install(&mut journal, &fixture.source, &anchors).unwrap());
    drop(journal);
    let mut journal = fixture.open();
    let durable = reopen(&journal, &fixture.source).unwrap().unwrap();
    assert_same_receipt(durable.receipt(), &receipt);
    assert_eq!(durable.anchors(), anchors);
    assert_same_receipt(
        &already(install(&mut journal, &fixture.source, &anchors).unwrap()),
        &receipt,
    );
    let opaque = journal.status(&fixture.source).unwrap();
    assert_eq!(opaque.generation, 0);
    assert!(opaque.observed_heads.is_empty());
    assert!(opaque.applied_heads.is_empty());
    assert_eq!(opaque.pending_operations, 0);
    assert!(journal.pending(&fixture.source, 8).unwrap().is_empty());
    assert_eq!(snapshot(repo.path()), before);
}
