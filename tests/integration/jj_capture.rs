use git_ai::operations::jj::capture::capture_current_state;
use git_ai::operations::workspace_context::WorkspaceContext;
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use {
    crate::jj_evidence::support::{first, left, merge},
    crate::jj_evidence::vectors::{FIRST_ID, LEFT_ID, MERGE_ID, RIGHT_ID},
    crate::jj_operation::support::{bytes_field, unhex},
    crate::jj_view::vectors::{MINIMAL_ID, RICH_ID},
    crate::repos::test_repo::{DaemonTestScope, TestRepo},
    git_ai::model::jj_observation::{JJ_OBSERVATION_READER_PROFILE, JjOperationEvidence},
    git_ai::operations::jj::baseline::JjBaselineBoundary,
    git_ai::operations::jj::capture::{CapturedJjCurrentState, JjCaptureError, JjCheckoutRelation},
    git_ai::operations::jj::evidence::verify_evidence,
    git_ai::operations::workspace_context::discover,
    std::fs,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "jj_capture_support.rs"]
mod support;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use support::*;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "jj_capture_real.rs"]
mod real;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "jj_capture_rejections.rs"]
mod rejections;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "jj_capture_persistence.rs"]
mod persistence;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn jj_capture_colocated_native_heads_and_checkout_preserve_exact_bytes_without_parents() {
    let fixture = Fixture::new(true);
    let context = fixture.context();
    let expected = merge();
    assert!(!fixture.operation_path(LEFT_ID).exists());
    assert!(!fixture.operation_path(RIGHT_ID).exists());
    fs::write(
        fixture.heads_dir().join("lock"),
        "ignored transient lock bytes",
    )
    .unwrap();
    let captured = checked_capture(&fixture, &context, deadline()).unwrap();
    assert_eq!(captured.reader_profile(), JJ_OBSERVATION_READER_PROFILE);
    assert_eq!(captured.head_ids(), [MERGE_ID]);
    assert_eq!(captured.anchors(), [expected]);
    assert_eq!(
        captured.checkout_bytes(),
        checkout_bytes(MERGE_ID, "default")
    );
    assert_eq!(captured.checkout().operation_id, MERGE_ID);
    assert_eq!(captured.checkout().workspace_name, "default");
    assert_eq!(
        captured.checkout_relation(),
        JjCheckoutRelation::OperationIsCapturedHead
    );
    assert!(std::ptr::eq(
        captured.checkout_evidence(),
        &captured.anchors()[0]
    ));
    let prepared = captured.prepare_baseline().unwrap();
    assert_eq!(prepared.boundary(), JjBaselineBoundary::CurrentState);
    assert_eq!(prepared.captured_head_ids(), [MERGE_ID]);
    assert_eq!(
        prepared.anchors()[0].operation().parent_ids,
        [LEFT_ID, RIGHT_ID]
    );
    assert_eq!(
        prepared.anchors()[0].view().wc_commit_ids["default"],
        "bb".repeat(20)
    );
    assert!(std::ptr::eq(
        prepared.anchors()[0].evidence(),
        &captured.anchors()[0]
    ));
    assert!(!fixture.root.join(".git/ai").exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn jj_capture_noncolocated_separate_git_common_directory_binds_the_discovered_store() {
    let fixture = Fixture::new(false);
    let git = fixture.repo_dir.join("store/git");
    let common = fixture.root.join("common object store");
    minimal_git(&common);
    fs::remove_dir(git.join("objects")).unwrap();
    fs::write(git.join("commondir"), "../../../../common object store\n").unwrap();
    fs::write(fixture.repo_dir.join("store/git-file"), "gitdir: git\n").unwrap();
    fs::write(fixture.repo_dir.join("store/git_target"), "git-file").unwrap();
    let context = fixture.context();
    assert!(!context.colocated);
    assert_ne!(context.git.git_dir, context.git.common_dir);
    assert_eq!(context.git.common_dir, common.canonicalize().unwrap());
    let captured = checked_capture(&fixture, &context, deadline()).unwrap();
    assert_eq!(captured.head_ids(), [MERGE_ID]);
    assert_eq!(captured.checkout_evidence().view_id, RICH_ID);
    assert!(!common.join("ai").exists());
    let copied = Fixture::new(false);
    let second = checked_capture(&copied, &copied.context(), deadline()).unwrap();
    assert_eq!(captured.anchors(), second.anchors());
    assert_ne!(captured.source_binding(), second.source_binding());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn jj_capture_linked_workspaces_share_sampled_source_despite_distinct_raw_repo_pointers() {
    let mut fixture = Fixture::new(true);
    let named = fixture.root.join(".jj/ shared repo \n");
    fs::rename(&fixture.repo_dir, &named).unwrap();
    fs::write(fixture.root.join(".jj/repo"), " shared repo \n").unwrap();
    fixture.repo_dir = named;
    let linked = fixture.root.join("linked workspace");
    fs::create_dir_all(linked.join(".jj/working_copy")).unwrap();
    fs::write(linked.join(".jj/working_copy/type"), "local").unwrap();
    fs::write(
        linked.join(".jj/repo"),
        fixture.repo_dir.canonicalize().unwrap().to_str().unwrap(),
    )
    .unwrap();
    fs::write(
        linked.join(".jj/working_copy/checkout"),
        checkout_bytes(MERGE_ID, "workspace-工"),
    )
    .unwrap();
    fs::write(linked.join("dirty-linked.txt"), "also never snapshotted\n").unwrap();
    let primary = fixture.context();
    let secondary = discover(&linked).unwrap();
    assert_ne!(primary.workspace_root, secondary.workspace_root);
    assert_ne!(
        fs::read(fixture.root.join(".jj/repo")).unwrap(),
        fs::read(linked.join(".jj/repo")).unwrap()
    );
    let first = checked_capture(&fixture, &primary, deadline()).unwrap();
    let second = checked_capture(&fixture, &secondary, deadline()).unwrap();
    assert_eq!(first.source_binding(), second.source_binding());
    assert_eq!(first.checkout().workspace_name, "default");
    assert_eq!(second.checkout().workspace_name, "workspace-工");
    assert_eq!(first.anchors(), second.anchors());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn jj_capture_retains_redundant_raw_heads_and_ignores_unrequested_object_files() {
    let fixture = Fixture::new(true);
    fixture.write_evidence(&first());
    fixture.write_evidence(&left());
    fixture.set_heads(&[MERGE_ID, FIRST_ID, LEFT_ID]);
    fs::write(
        fixture.operation_path(RIGHT_ID),
        "unrequested malformed parent",
    )
    .unwrap();
    fs::write(
        fixture.operation_path(&"ee".repeat(64)),
        "unrelated malformed operation",
    )
    .unwrap();
    fs::write(
        fixture.view_path(&"ff".repeat(64)),
        "unrelated malformed view",
    )
    .unwrap();
    let captured = checked_capture(&fixture, &fixture.context(), deadline()).unwrap();
    let mut expected = vec![MERGE_ID.to_owned(), FIRST_ID.to_owned(), LEFT_ID.to_owned()];
    expected.sort();
    assert_eq!(captured.head_ids(), expected);
    assert_eq!(captured.anchors().len(), expected.len());
    let actual: std::collections::BTreeSet<_> = captured
        .anchors()
        .iter()
        .map(|anchor| anchor.operation_id.as_str())
        .collect();
    assert_eq!(actual, [MERGE_ID, FIRST_ID, LEFT_ID].into_iter().collect());
    assert_eq!(captured.prepare_baseline().unwrap().anchors().len(), 3);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn jj_capture_stale_checkout_joins_its_own_view_without_becoming_an_anchor() {
    let fixture = Fixture::new(true);
    fixture.write_evidence(&left());
    fixture.set_heads(&[LEFT_ID]);
    let captured = checked_capture(&fixture, &fixture.context(), deadline()).unwrap();
    assert_eq!(
        captured.checkout_relation(),
        JjCheckoutRelation::OperationOutsideCapturedHeads
    );
    assert_eq!(captured.head_ids(), [LEFT_ID]);
    assert_eq!(captured.anchors(), [left()]);
    assert_eq!(captured.checkout_evidence(), &merge());
    let own = verify_evidence(captured.reader_profile(), captured.checkout_evidence()).unwrap();
    assert_eq!(own.view().wc_commit_ids["default"], "bb".repeat(20));
    assert_eq!(own.view().view_id, RICH_ID);
    assert_eq!(captured.anchors()[0].view_id, MINIMAL_ID);
    assert_eq!(
        captured.prepare_baseline().unwrap().captured_head_ids(),
        [LEFT_ID]
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn jj_capture_checkout_workspace_must_exist_in_its_own_view_even_at_a_head() {
    for at_head in [true, false] {
        let fixture = Fixture::new(true);
        if at_head {
            fixture.write_checkout(MERGE_ID, "invented-workspace");
        } else {
            fixture.write_evidence(&first());
            fixture.write_checkout(FIRST_ID, "default");
        }
        // The current merge view has "default"; it cannot repair the older empty view.
        rejected(&fixture, &fixture.context(), Some("checkout"));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn jj_capture_unqualified_platform_returns_unsupported_before_path_access() {
    use git_ai::operations::workspace_context::GitPaths;
    let context = WorkspaceContext {
        schema_version: 1,
        capability: "discovery_only",
        vcs: "jj",
        workspace_root: "definitely-missing-workspace".into(),
        git: GitPaths {
            git_dir: "missing-git".into(),
            common_dir: "missing-common".into(),
        },
        jj: None,
        colocated: false,
    };
    let error = match capture_current_state(&context, Instant::now() + Duration::from_secs(1)) {
        Ok(_) => panic!("unqualified platform returned native capture"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("unsupported"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "jj_registration.rs"]
mod registration;
