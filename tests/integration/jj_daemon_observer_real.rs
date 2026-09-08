use super::support::Harness;
use super::*;
use crate::debug_context::{jj, require_pinned_jj};

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_daemon_observer_real_colocated_admission_and_restart_preserve_cutoff() {
    run(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_daemon_observer_real_noncolocated_admission_and_restart_preserve_cutoff() {
    run(false);
}

fn run(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("real daemon observer workspace");
        jj(
            &repo,
            repo.path(),
            &["git", "init", "--no-colocate", root.to_str().unwrap()],
        );
        root
    };
    jj(
        &repo,
        &root,
        &["describe", "-m", "daemon observer baseline"],
    );
    let mut h = Harness::from_fixture(Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    });
    let saved = h.saved_rows();
    jj(
        &h.fixture.repo,
        &h.fixture.root,
        &["describe", "-m", "before observer activation"],
    );
    let captured = capture_current_state(&h.fixture.context(), deadline()).unwrap();
    let expected = captured.anchors()[0].clone();
    assert_eq!(captured.anchors().len(), 1);
    drop(captured);
    h.start();
    fs::write(
        h.fixture.root.join("dirty-daemon-observer.txt"),
        b"preserve dirty bytes after jj
",
    )
    .unwrap();
    let files = manifest(h.fixture.repo.path());
    h.ready_control("enable");
    h.active(1, std::slice::from_ref(&expected.operation_id));
    let first = h.packet(1);
    assert!(first.ordered_operations().contains(&expected));
    assert_eq!(first.receipt().expected_generation(), 0);
    assert_eq!(first.reached_baseline_ids(), h.baseline_heads());
    assert_eq!(manifest(h.fixture.repo.path()), files);
    assert_eq!(h.saved_rows(), saved);
    h.fixture.repo.restart_dedicated_daemon_for_test();
    h.active(1, std::slice::from_ref(&expected.operation_id));
    assert_eq!(h.packet_count(), 1);
    h.disabled();

    jj(
        &h.fixture.repo,
        &h.fixture.root,
        &["describe", "-m", "after observer drain"],
    );
    let captured = capture_current_state(&h.fixture.context(), deadline()).unwrap();
    assert_eq!(captured.anchors().len(), 1);
    let next = captured.anchors()[0].clone();
    assert_ne!(next.operation_id, expected.operation_id);
    drop(captured);
    fs::write(
        h.fixture.root.join("dirty-daemon-observer.txt"),
        b"second dirty state after jj
",
    )
    .unwrap();
    let files = manifest(h.fixture.repo.path());
    h.ready_control("resume");
    h.active(2, std::slice::from_ref(&next.operation_id));
    let second = h.packet(2);
    assert_eq!(second.receipt().expected_generation(), 1);
    assert!(second.ordered_operations().contains(&expected));
    assert!(second.ordered_operations().contains(&next));
    assert_eq!(second.reached_baseline_ids(), h.baseline_heads());
    h.disabled();
    assert_eq!(h.packet_count(), 2);
    assert_eq!(h.saved_rows(), saved);
    assert_eq!(manifest(h.fixture.repo.path()), files);
    assert_eq!(
        fs::read(h.fixture.root.join("dirty-daemon-observer.txt")).unwrap(),
        b"second dirty state after jj
"
    );
}
