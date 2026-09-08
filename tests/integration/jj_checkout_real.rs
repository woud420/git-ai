use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj, snapshot};
use git_ai::operations::jj::operation::{MAX_OPERATION_BYTES, decode_operation};
use git_ai::operations::jj::view::{MAX_VIEW_BYTES, decode_view};
use std::fs;

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_checkout_real_stale_workspace_joins_its_own_operation_view_without_updates() {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let primary = repo.path().join("primary workspace");
    let linked = repo.path().join("linked workspace");
    jj(
        &repo,
        repo.path(),
        &["git", "init", "--no-colocate", primary.to_str().unwrap()],
    );
    fs::write(primary.join("example.txt"), "original fixture\n").unwrap();
    jj(&repo, &primary, &["commit", "-m", "checkout fixture base"]);
    assert_eq!(
        fs::read_to_string(primary.join("example.txt")).unwrap(),
        "original fixture\n"
    );
    jj(
        &repo,
        &primary,
        &[
            "workspace",
            "add",
            "--name",
            "fixture-linked",
            linked.to_str().unwrap(),
        ],
    );
    let checkout_path = linked.join(".jj/working_copy/checkout");
    let checkout_before = read_jj_fixture(&checkout_path, MAX_CHECKOUT_BYTES);
    jj(
        &repo,
        &primary,
        &[
            "describe",
            "-r",
            "fixture-linked@",
            "-m",
            "rewritten from primary workspace",
        ],
    );
    assert_eq!(
        read_jj_fixture(&checkout_path, MAX_CHECKOUT_BYTES),
        checkout_before
    );
    let dirty_primary = primary.join("dirty-primary.txt");
    let dirty_linked = linked.join("dirty-linked.txt");
    fs::write(&dirty_primary, "primary remains dirty\n").unwrap();
    fs::write(&dirty_linked, "linked remains dirty and stale\n").unwrap();

    let store = primary.join(".jj/repo/op_store");
    for name in ["operations", "views"] {
        let entries: Vec<_> = fs::read_dir(store.join(name)).unwrap().take(17).collect();
        assert!((3..=16).contains(&entries.len()));
        for entry in entries {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            let maximum = if name == "operations" {
                MAX_OPERATION_BYTES
            } else {
                MAX_VIEW_BYTES
            };
            assert!(entry.metadata().unwrap().len() <= maximum as u64);
        }
    }
    let heads: Vec<_> = fs::read_dir(primary.join(".jj/repo/op_heads/heads"))
        .unwrap()
        .take(2)
        .collect();
    assert_eq!(heads.len(), 1);
    let latest_id = heads[0]
        .as_ref()
        .unwrap()
        .file_name()
        .to_str()
        .unwrap()
        .to_owned();
    let before = snapshot(repo.path());

    let checkout = decode(&read_jj_fixture(&checkout_path, MAX_CHECKOUT_BYTES)).unwrap();
    assert_eq!(checkout.workspace_name, "fixture-linked");
    assert_ne!(checkout.operation_id, latest_id);
    let operation = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        &checkout.operation_id,
        &read_jj_fixture(
            &store.join("operations").join(&checkout.operation_id),
            MAX_OPERATION_BYTES,
        ),
    )
    .unwrap();
    let own_view = decode_view(
        JJ_OBSERVATION_READER_PROFILE,
        &operation.view_id,
        &read_jj_fixture(
            &store.join("views").join(&operation.view_id),
            MAX_VIEW_BYTES,
        ),
    )
    .unwrap();
    let completed_commit = own_view
        .wc_commit_ids
        .get(&checkout.workspace_name)
        .unwrap();
    assert_eq!(completed_commit.len(), 40);
    let latest_operation = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        &latest_id,
        &read_jj_fixture(
            &store.join("operations").join(&latest_id),
            MAX_OPERATION_BYTES,
        ),
    )
    .unwrap();
    let latest_view = decode_view(
        JJ_OBSERVATION_READER_PROFILE,
        &latest_operation.view_id,
        &read_jj_fixture(
            &store.join("views").join(&latest_operation.view_id),
            MAX_VIEW_BYTES,
        ),
    )
    .unwrap();
    assert_ne!(own_view.view_id, latest_view.view_id);
    assert_ne!(
        Some(completed_commit),
        latest_view.wc_commit_ids.get(&checkout.workspace_name)
    );
    assert_eq!(
        read_jj_fixture(&checkout_path, MAX_CHECKOUT_BYTES),
        checkout_before
    );
    assert_eq!(
        fs::read_to_string(dirty_primary).unwrap(),
        "primary remains dirty\n"
    );
    assert_eq!(
        fs::read_to_string(dirty_linked).unwrap(),
        "linked remains dirty and stale\n"
    );
    assert_eq!(snapshot(repo.path()), before);
}
