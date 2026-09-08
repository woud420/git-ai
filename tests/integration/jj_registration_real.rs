use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj};
use serde::{Deserialize, Serialize};

const CHILD: &str = "jj_capture::registration::real::registration_real_child";

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_registration_real_colocated_install_reopen_and_advanced_retry() {
    real_fixture(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_registration_real_noncolocated_install_reopen_and_advanced_retry() {
    real_fixture(false);
}

fn real_fixture(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("real jj workspace");
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
        &["describe", "-m", "registration baseline fixture"],
    );
    fs::write(root.join("dirty.txt"), b"not snapshotted by registration\n").unwrap();
    let mut fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    run_fixture_case("real_install", &mut fixture, Policy::Root, CHILD);
    let seal = read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024);
    jj(
        &fixture.repo,
        &fixture.root,
        &["describe", "-m", "actual jj operation after registration"],
    );
    assert_eq!(
        read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024),
        seal
    );
    fs::write(
        fixture.root.join("dirty-after-advance.txt"),
        b"also remains unsnapshotted\n",
    )
    .unwrap();
    run_fixture_case("real_advance", &mut fixture, Policy::Root, CHILD);
    assert_eq!(
        read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024),
        seal
    );
    assert_eq!(
        fs::read(fixture.root.join("dirty-after-advance.txt")).unwrap(),
        b"also remains unsnapshotted\n"
    );
}

#[derive(Serialize, Deserialize)]
struct Saved {
    source_id: String,
    initialization_receipt_id: String,
    attachment_id: String,
    baseline_id: String,
    head_ids: Vec<String>,
    anchor_digests: Vec<(String, String, String, String)>,
    checkout_operation_id: String,
}

impl Saved {
    fn from_registered(value: &RegisteredJjCurrentState) -> Self {
        Self {
            source_id: value.source_id().to_owned(),
            initialization_receipt_id: value.initialization_receipt_id().to_owned(),
            attachment_id: value.attachment_id().to_owned(),
            baseline_id: value.baseline().receipt().baseline_id().to_owned(),
            head_ids: value.baseline().receipt().captured_head_ids().to_vec(),
            anchor_digests: anchor_digests(value),
            checkout_operation_id: value.checkout().operation_id.clone(),
        }
    }

    fn assert_receipt(&self, value: &RegisteredJjCurrentState) {
        assert_eq!(value.source_id(), self.source_id);
        assert_eq!(
            value.initialization_receipt_id(),
            self.initialization_receipt_id
        );
        assert_eq!(value.attachment_id(), self.attachment_id);
        assert_eq!(value.workspace_name(), "default");
        assert_eq!(value.baseline().receipt().baseline_id(), self.baseline_id);
        assert_eq!(
            value.baseline().receipt().captured_head_ids(),
            self.head_ids
        );
        assert_eq!(anchor_digests(value), self.anchor_digests);
    }
}

fn anchor_digests(value: &RegisteredJjCurrentState) -> Vec<(String, String, String, String)> {
    value
        .baseline()
        .anchors()
        .iter()
        .map(|anchor| {
            (
                anchor.operation_id.clone(),
                hash(&anchor.operation_bytes),
                anchor.view_id.clone(),
                hash(&anchor.view_bytes),
            )
        })
        .collect()
}

#[test]
#[ignore = "isolated Config helper for the explicit real-jj registration lane"]
fn registration_real_child() {
    let Some(case) = child_case() else { return };
    let config = case.config();
    match case.name.as_str() {
        "real_install" => install_real(&case, &config),
        "real_advance" => advance_real(&case, &config),
        other => panic!("unknown real registration fixture {other}"),
    }
    println!("REGISTRATION_CASE_COMPLETED:{}", case.name);
}

fn install_real(case: &Case, config: &Config) {
    let captured = capture_current_state(&case.context(), deadline()).unwrap();
    assert_eq!(captured.head_ids().len(), 1);
    assert_eq!(captured.checkout().workspace_name, "default");
    assert_eq!(captured.head_ids()[0], captured.checkout().operation_id);
    assert_eq!(
        captured.checkout_relation(),
        JjCheckoutRelation::OperationIsCapturedHead
    );
    let mut journal = case.open();
    let before = manifest(&case.repository);
    let registered = installed(case.register(&mut journal, config).unwrap());
    behavior::assert_registration(
        case,
        &journal,
        &registered,
        captured.anchors(),
        &captured.checkout().operation_id,
        false,
    );
    behavior::assert_only_publication(case, before, registered.source_id());
    drop(journal);
    let mut journal = case.open();
    let before = state(case);
    let reopened = case.reopen(&journal, config).unwrap().unwrap();
    let retry = already(case.register(&mut journal, config).unwrap());
    for value in [&reopened, &retry] {
        behavior::same_receipt(value, &registered);
        behavior::assert_registration(
            case,
            &journal,
            value,
            captured.anchors(),
            &captured.checkout().operation_id,
            false,
        );
    }
    assert_eq!(state(case), before);
    let saved = serde_json::to_vec(&Saved::from_registered(&registered)).unwrap();
    assert!(saved.len() < 16 * 1024);
    fs::write(case.test_home.join("registration-real-saved.json"), saved).unwrap();
}

fn advance_real(case: &Case, config: &Config) {
    let saved: Saved = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("registration-real-saved.json"),
        16 * 1024,
    ))
    .unwrap();
    let captured = capture_current_state(&case.context(), deadline()).unwrap();
    assert_eq!(captured.head_ids().len(), 1);
    assert_ne!(captured.head_ids(), saved.head_ids);
    assert_ne!(
        captured.checkout().operation_id,
        saved.checkout_operation_id
    );
    assert_eq!(captured.head_ids()[0], captured.checkout().operation_id);
    assert_eq!(
        captured.checkout_relation(),
        JjCheckoutRelation::OperationIsCapturedHead
    );
    let mut journal = case.open();
    let before = state(case);
    let reopened = case.reopen(&journal, config).unwrap().unwrap();
    let retry = already(case.register(&mut journal, config).unwrap());
    for value in [&reopened, &retry] {
        saved.assert_receipt(value);
        assert_eq!(
            value.checkout().operation_id,
            captured.checkout().operation_id
        );
        assert!(matches!(
            value.checkout_relation(),
            JjRegisteredCheckoutRelation::OutsideBaseline
        ));
        behavior::assert_registration(
            case,
            &journal,
            value,
            reopened.baseline().anchors(),
            &captured.checkout().operation_id,
            true,
        );
    }
    assert_eq!(state(case), before);
}
