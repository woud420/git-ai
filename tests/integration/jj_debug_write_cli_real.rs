use super::*;
use crate::debug_context::{jj, require_pinned_jj};

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_debug_write_cli_real_pinned_jj_initialize_capture_retry_and_receipt() {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    jj(&repo, repo.path(), &["git", "init", "--colocate"]);
    jj(
        &repo,
        repo.path(),
        &["describe", "-m", "write CLI baseline"],
    );
    let root = repo.path().to_owned();
    let mut fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    fixture_case("write:real_initialize", &mut fixture, Policy::Root);
    jj(
        &fixture.repo,
        &fixture.root,
        &["describe", "-m", "write CLI successor"],
    );
    fs::write(
        fixture.root.join("dirty-real-write-cli.txt"),
        b"not snapshotted by CLI\n",
    )
    .unwrap();
    fixture_case("write:real_capture", &mut fixture, Policy::Root);
    assert_eq!(
        fs::read(fixture.root.join("dirty-real-write-cli.txt")).unwrap(),
        b"not snapshotted by CLI\n"
    );
}

pub fn initialize(case: &Case, config: &Config) {
    let (journal, saved) = initialize_cli(case, config);
    let state = status(case, &journal, config);
    assert_eq!(state.cursor().generation(), 0);
    assert_eq!(
        state.cursor().admitted_head_ids(),
        saved.baseline().receipt().captured_head_ids()
    );
    fs::write(
        case.test_home.join("write-cli-real-initialization.json"),
        serde_json::to_vec(&registration_json(&saved, "installed")).unwrap(),
    )
    .unwrap();
}

pub fn capture(case: &Case, config: &Config) {
    let journal = JjObservationJournal::open_read_only_at_path(&case.journal_path).unwrap();
    let before = status(case, &journal, config);
    let captured = capture_current_state(&case.context(), deadline()).unwrap();
    assert_eq!(captured.anchors().len(), 1);
    assert_eq!(
        captured.anchors()[0].parent_ids,
        before
            .registration()
            .baseline()
            .receipt()
            .captured_head_ids()
    );
    let (result, admission, current) = capture_cli(case, config, before.cursor());
    assert_eq!(admission.ordered_operations(), captured.anchors());
    assert_eq!(
        admission.reached_baseline_ids(),
        before.cursor().admitted_head_ids()
    );
    let retry = checked(
        case,
        command(
            case,
            &expect_args(&case.journal_path, before.cursor().expectation()),
            &case.root,
        ),
        None,
    );
    assert_eq!(
        retry,
        capture_json(&admission, &current, "already_admitted")
    );
    assert_eq!(result["admission"], retry["admission"]);
    let args = receipt_args(
        &case.journal_path,
        current.source_id(),
        admission.receipt().admission_id(),
    );
    assert_eq!(
        checked(case, command(case, &args, &case.root), None),
        receipt_json(Some(&admission))
    );
    let raw = fs::read(case.test_home.join("write-cli-real-initialization.json")).unwrap();
    assert!(raw.len() < 16 * 1024);
    let initial: Json = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        initial["registration"]["baseline_id"],
        before.cursor().baseline_id()
    );
    assert_eq!(
        fs::read(case.root.join("dirty-real-write-cli.txt")).unwrap(),
        b"not snapshotted by CLI\n"
    );
}
