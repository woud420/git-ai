use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj};
use git_ai::operations::jj::operation::{MAX_OPERATION_BYTES, decode_operation};
use git_ai::operations::jj::view::MAX_VIEW_BYTES;
use std::fs;

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_baseline_real_current_head_preserves_unprovided_history_and_dirty_files() {
    require_pinned_jj();
    let fixture = Fixture::new();
    let repo = &fixture.repo;
    jj(repo, repo.path(), &["git", "init", "--colocate"]);
    jj(repo, repo.path(), &["describe", "-m", "before baseline"]);
    jj(repo, repo.path(), &["describe", "-m", "current baseline"]);
    fs::write(
        repo.path().join("unsnapshotted.txt"),
        "left dirty at baseline\n",
    )
    .unwrap();

    let store = repo.path().join(".jj/repo/op_store");
    let entries: Vec<_> = fs::read_dir(repo.path().join(".jj/repo/op_heads/heads"))
        .unwrap()
        .take(2)
        .map(|entry| entry.unwrap())
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].file_type().unwrap().is_file());
    let operation_id = entries[0].file_name().to_str().unwrap().to_owned();
    let operation_bytes = read_jj_fixture(
        &store.join("operations").join(&operation_id),
        MAX_OPERATION_BYTES,
    );
    let operation = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        &operation_id,
        &operation_bytes,
    )
    .unwrap();
    assert!(!operation.parent_ids.is_empty());
    assert!(operation.parent_ids.iter().all(|id| id != &"00".repeat(64)));
    let view_bytes = read_jj_fixture(
        &store.join("views").join(&operation.view_id),
        MAX_VIEW_BYTES,
    );
    let anchors = [JjOperationEvidence {
        operation_id,
        parent_ids: operation.parent_ids.clone(),
        view_id: operation.view_id,
        operation_bytes,
        view_bytes,
    }];
    let heads = heads(&anchors);
    let journal = fixture.open();
    let before_status = journal.status(&fixture.source).unwrap();
    let before_pending = journal.pending(&fixture.source, 1).unwrap();
    let before_files = snapshot(repo.path());
    let before_home = snapshot(repo.test_home_path());

    let prepared = prepare(&heads, &anchors).unwrap();
    assert_eq!(prepared.boundary(), JjBaselineBoundary::CurrentState);
    assert_eq!(prepared.anchors().len(), 1);
    assert!(std::ptr::eq(prepared.anchors()[0].evidence(), &anchors[0]));
    assert_eq!(
        prepared.anchors()[0].operation().parent_ids,
        operation.parent_ids
    );
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 1).unwrap(), before_pending);
    assert_eq!(before_status.generation, 0);
    assert!(before_status.applied_heads.is_empty());
    assert_eq!(snapshot(repo.path()), before_files);
    assert_eq!(snapshot(repo.test_home_path()), before_home);
}
