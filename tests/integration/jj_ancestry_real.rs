use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj};
use git_ai::model::jj_observation::MAX_JJ_OBSERVATION_OPERATION_BYTES;
use git_ai::operations::jj::operation::decode_operation;
use std::fs;
use std::path::Path;

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_ancestry_real_later_operations_close_on_reopened_baseline_without_updates() {
    require_pinned_jj();
    let fixture = Fixture::new();
    let repo = &fixture.repo;
    jj(repo, repo.path(), &["git", "init", "--colocate"]);
    jj(
        repo,
        repo.path(),
        &["describe", "-m", "native ancestry baseline"],
    );
    let baseline_id = current_head(repo.path());
    let anchors = [read_pair(repo.path(), &baseline_id)];
    assert!(
        anchors[0]
            .parent_ids
            .iter()
            .all(|id| id != &"00".repeat(64))
    );
    let baseline = durable(&fixture, &anchors);
    assert_eq!(
        baseline.receipt().captured_head_ids(),
        std::slice::from_ref(&baseline_id)
    );
    assert_eq!(baseline.receipt().generation(), 1);
    assert_eq!(baseline.anchors()[0].operation_id, baseline_id);

    // Each fixture command publishes one retained ID. The test reads those
    // exact records rather than adding a history collector to the verifier.
    jj(
        repo,
        repo.path(),
        &["describe", "-m", "first later operation"],
    );
    let first_id = current_head(repo.path());
    let first = read_pair(repo.path(), &first_id);
    assert_eq!(first.parent_ids, std::slice::from_ref(&baseline_id));
    jj(
        repo,
        repo.path(),
        &["describe", "-m", "second later operation"],
    );
    let latest_id = current_head(repo.path());
    let latest = read_pair(repo.path(), &latest_id);
    assert_eq!(latest.parent_ids, std::slice::from_ref(&first_id));
    assert_ne!(first_id, latest_id);
    assert_ne!(baseline_id, first_id);

    let dirty = repo.path().join("unsnapshotted.txt");
    fs::write(&dirty, b"keep this file outside the jj snapshot\n").unwrap();
    let before_repo = snapshot(repo.path());
    let journal = fixture.open();
    let before_status = journal.status(&fixture.source).unwrap();
    let before_pending = journal.pending(&fixture.source, 8).unwrap();
    let before_native = baseline_support::native_rows(&fixture);
    let records = [latest, first];
    let references = records.iter().collect::<Vec<_>>();
    let heads = vec![latest_id.clone()];
    let proof = checked(&fixture, &baseline, input(&baseline, &heads, &references)).unwrap();

    assert_eq!(ordered_ids(&proof), [first_id, latest_id]);
    assert_eq!(proof.reached_baseline_ids(), &[baseline_id]);
    assert!(!proof.reaches_root());
    assert!(std::ptr::eq(proof.baseline_receipt(), baseline.receipt()));
    assert!(std::ptr::eq(proof.head_ids(), heads.as_slice()));
    for (verified, original) in proof
        .ordered_operations()
        .iter()
        .zip([&records[1], &records[0]])
    {
        assert!(std::ptr::eq::<JjOperationEvidence>(
            verified.evidence(),
            original
        ));
    }
    assert_eq!(snapshot(repo.path()), before_repo);
    assert_eq!(
        fs::read(&dirty).unwrap(),
        b"keep this file outside the jj snapshot\n"
    );
    assert_eq!(journal.status(&fixture.source).unwrap(), before_status);
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), before_pending);
    assert_eq!(baseline_support::native_rows(&fixture), before_native);
    assert_eq!(before_status.generation, 0);
    assert!(before_status.observed_heads.is_empty());
    assert!(before_status.applied_heads.is_empty());
    assert!(before_pending.is_empty());
    assert!(!repo.path().join(".git/ai").exists());
}

fn current_head(root: &Path) -> String {
    let entries: Vec<_> = fs::read_dir(root.join(".jj/repo/op_heads/heads"))
        .unwrap()
        .take(3)
        .map(Result::unwrap)
        .collect();
    assert!(
        entries.len() <= 2,
        "fixture must have one head and optional lock"
    );
    let heads: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.file_name() != "lock")
        .collect();
    assert_eq!(heads.len(), 1);
    let head = &heads[0];
    assert!(head.file_type().unwrap().is_file());
    assert!(read_jj_fixture(&head.path(), 0).is_empty());
    let id = head.file_name().to_str().unwrap().to_owned();
    assert_eq!(id.len(), 128);
    assert!(
        id.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    id
}

fn read_pair(root: &Path, operation_id: &str) -> JjOperationEvidence {
    let store = root.join(".jj/repo/op_store");
    let operation_bytes = read_jj_fixture(
        &store.join("operations").join(operation_id),
        MAX_JJ_OBSERVATION_OPERATION_BYTES,
    );
    let operation = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        operation_id,
        &operation_bytes,
    )
    .unwrap();
    let view_bytes = read_jj_fixture(
        &store.join("views").join(&operation.view_id),
        MAX_JJ_OBSERVATION_OPERATION_BYTES - operation_bytes.len(),
    );
    JjOperationEvidence {
        operation_id: operation.operation_id,
        parent_ids: operation.parent_ids,
        view_id: operation.view_id,
        operation_bytes,
        view_bytes,
    }
}
