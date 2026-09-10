use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj, snapshot};
use git_ai::operations::jj::checkout::MAX_CHECKOUT_BYTES;

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_capture_real_colocated_captures_without_snapshotting_dirty_files() {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    jj(&repo, repo.path(), &["git", "init", "--colocate"]);
    let root = repo.path().to_owned();
    let fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    fs::write(fixture.root.join("dirty.txt"), "left unsnapshotted\n").unwrap();
    let context = fixture.context();
    let _ = manifest(fixture.repo.path());
    let before = snapshot(fixture.repo.path());
    let captured = checked_capture(&fixture, &context, deadline()).unwrap();
    assert_eq!(
        captured.head_ids(),
        [captured.checkout().operation_id.as_str()]
    );
    assert_eq!(
        captured.checkout_relation(),
        JjCheckoutRelation::OperationIsCapturedHead
    );
    assert_eq!(
        captured.checkout_bytes(),
        read_jj_fixture(&fixture.checkout_path(), MAX_CHECKOUT_BYTES)
    );
    let own = verify_evidence(captured.reader_profile(), captured.checkout_evidence()).unwrap();
    assert!(
        own.view()
            .wc_commit_ids
            .contains_key(&captured.checkout().workspace_name)
    );
    assert_eq!(captured.prepare_baseline().unwrap().anchors().len(), 1);
    assert_eq!(snapshot(fixture.repo.path()), before);
    assert!(!fixture.root.join(".git/ai").exists());
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_capture_real_linked_stale_checkout_uses_its_own_view_and_leaves_both_workspaces_dirty() {
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
    fs::write(primary.join("dirty-primary.txt"), "primary remains dirty\n").unwrap();
    fs::write(
        linked.join("dirty-linked.txt"),
        "linked remains dirty and stale\n",
    )
    .unwrap();
    let fixture = Fixture {
        repo,
        repo_dir: primary.join(".jj/repo"),
        root: primary,
    };
    let primary_context = fixture.context();
    let linked_context = discover(&linked).unwrap();
    let current = checked_capture(&fixture, &primary_context, deadline()).unwrap();
    let stale = checked_capture(&fixture, &linked_context, deadline()).unwrap();
    assert_eq!(current.source_binding(), stale.source_binding());
    assert_eq!(current.head_ids(), stale.head_ids());
    assert_eq!(
        stale.checkout_relation(),
        JjCheckoutRelation::OperationOutsideCapturedHeads
    );
    assert_eq!(stale.checkout_bytes(), checkout_before);
    assert_eq!(stale.checkout().workspace_name, "fixture-linked");
    assert!(!stale.head_ids().contains(&stale.checkout().operation_id));
    let own = verify_evidence(stale.reader_profile(), stale.checkout_evidence()).unwrap();
    let prepared = stale.prepare_baseline().unwrap();
    let latest = &prepared.anchors()[0];
    assert_ne!(own.view().view_id, latest.view().view_id);
    assert_ne!(
        own.view().wc_commit_ids.get("fixture-linked"),
        latest.view().wc_commit_ids.get("fixture-linked")
    );
    assert_eq!(
        stale.prepare_baseline().unwrap().anchors().len(),
        stale.head_ids().len()
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("dirty-primary.txt")).unwrap(),
        "primary remains dirty\n"
    );
    assert_eq!(
        fs::read_to_string(linked.join("dirty-linked.txt")).unwrap(),
        "linked remains dirty and stale\n"
    );
    assert!(!fixture.repo_dir.join("store/git/ai").exists());
}
