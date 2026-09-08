use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj};
use serde::{Deserialize, Serialize};

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_admission_real_colocated_persists_successive_native_observations() {
    fixture(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_admission_real_noncolocated_persists_successive_native_observations() {
    fixture(false);
}

fn fixture(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("real admission workspace");
        jj(
            &repo,
            repo.path(),
            &["git", "init", "--no-colocate", root.to_str().unwrap()],
        );
        root
    };
    jj(&repo, &root, &["describe", "-m", "admission baseline"]);
    let mut fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    history_fixture_case("history:admission:real_install", &mut fixture, Policy::Root);
    let seal = read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024);
    let mut records: Vec<JjOperationEvidence> = Vec::new();
    for (index, message) in ["admission operation A", "admission operation B"]
        .into_iter()
        .enumerate()
    {
        jj(&fixture.repo, &fixture.root, &["describe", "-m", message]);
        let captured = capture_current_state(&fixture.context(), deadline()).unwrap();
        assert_eq!(captured.anchors().len(), 1);
        let record = captured.anchors()[0].clone();
        if let Some(parent) = records.last() {
            assert_eq!(
                record.parent_ids,
                std::slice::from_ref(&parent.operation_id)
            );
        }
        records.push(record);
        let raw = serde_json::to_vec(&records).unwrap();
        assert!(raw.len() < 128 * 1024);
        fs::write(
            fixture
                .repo
                .test_home_path()
                .join("admission-real-expected.json"),
            raw,
        )
        .unwrap();
        if index == 1 {
            fs::write(
                fixture.root.join("dirty-after-admission.txt"),
                b"never snapshotted by admission\n",
            )
            .unwrap();
        }
        history_fixture_case("history:admission:real_admit", &mut fixture, Policy::Root);
    }
    assert_eq!(
        read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024),
        seal
    );
    assert_eq!(
        fs::read(fixture.root.join("dirty-after-admission.txt")).unwrap(),
        b"never snapshotted by admission\n"
    );
}

#[derive(Serialize, Deserialize)]
struct Saved {
    source: String,
    initialization: String,
    attachment: String,
    baseline: String,
    baseline_heads: Vec<String>,
    anchors: Vec<JjOperationEvidence>,
    generation: u64,
    admitted_heads: Vec<String>,
    last_admission: Option<String>,
}

pub fn install(case: &Case, config: &Config) {
    let (journal, registered) = super::super::support::install(case, config);
    let observed = status(case, &journal, config);
    assert_eq!(observed.cursor().generation(), 0);
    assert!(observed.latest_receipt().is_none());
    let heads = registered.baseline().receipt().captured_head_ids();
    assert_eq!(heads.len(), 1);
    assert_eq!(observed.cursor().admitted_head_ids(), heads);
    let saved = Saved {
        source: registered.source_id().to_owned(),
        initialization: registered.initialization_receipt_id().to_owned(),
        attachment: registered.attachment_id().to_owned(),
        baseline: registered.baseline().receipt().baseline_id().to_owned(),
        baseline_heads: heads.to_vec(),
        anchors: registered.baseline().anchors().to_vec(),
        generation: 0,
        admitted_heads: heads.to_vec(),
        last_admission: None,
    };
    fs::write(
        case.test_home.join("admission-real-saved.json"),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
}

pub fn admit(case: &Case, config: &Config) {
    let path = case.test_home.join("admission-real-saved.json");
    let mut saved: Saved = serde_json::from_slice(&read_jj_fixture(&path, 128 * 1024)).unwrap();
    let records: Vec<JjOperationEvidence> = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("admission-real-expected.json"),
        128 * 1024,
    ))
    .unwrap();
    assert_eq!(records.len(), (saved.generation + 1) as usize);
    assert_eq!(records[0].parent_ids, saved.baseline_heads);
    let mut journal = case.open();
    let current = status(case, &journal, config);
    assert_eq!(current.cursor().generation(), saved.generation);
    assert_eq!(current.cursor().admitted_head_ids(), saved.admitted_heads);
    let expected = NativeAdmissionExpectation {
        source_id: &saved.source,
        initialization_receipt_id: &saved.initialization,
        baseline_id: &saved.baseline,
        generation: saved.generation,
        admitted_head_ids: &saved.admitted_heads,
    };
    let admitted = outcome(
        admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            expected,
            deadline(),
            &mut admission_budget(),
        )
        .unwrap(),
        false,
    );
    let registered = admitted.registration();
    assert_eq!(registered.source_id(), saved.source);
    assert_eq!(registered.initialization_receipt_id(), saved.initialization);
    assert_eq!(registered.attachment_id(), saved.attachment);
    assert_eq!(
        registered.baseline().receipt().baseline_id(),
        saved.baseline
    );
    assert_eq!(
        registered.baseline().receipt().captured_head_ids(),
        saved.baseline_heads
    );
    assert_eq!(registered.baseline().anchors(), saved.anchors);
    assert_eq!(registered.baseline().receipt().generation(), 1);
    assert_eq!(
        registered.checkout().operation_id,
        records.last().unwrap().operation_id
    );
    assert_eq!(admitted.admission().ordered_operations(), records);
    assert_eq!(
        admitted.admission().reached_baseline_ids(),
        saved.baseline_heads
    );
    assert!(!admitted.admission().reaches_root());
    let retry = outcome(
        admit_now(case, &mut journal, config, current.cursor()),
        true,
    );
    let id = admitted.admission().receipt().admission_id().to_owned();
    assert_eq!(retry.admission().receipt().admission_id(), id);
    drop(journal);
    let journal = case.open();
    assert_eq!(
        packet(case, &journal, &saved.source, &id).ordered_operations(),
        records
    );
    if let Some(old_id) = &saved.last_admission {
        assert_eq!(
            packet(case, &journal, &saved.source, old_id).ordered_operations(),
            &records[..records.len() - 1]
        );
    }
    let observed = status(case, &journal, config);
    assert_eq!(observed.cursor().generation(), saved.generation + 1);
    assert_eq!(observed.latest_receipt().unwrap().admission_id(), id);
    saved.generation = observed.cursor().generation();
    saved.admitted_heads = observed.cursor().admitted_head_ids().to_vec();
    saved.last_admission = Some(id);
    fs::write(path, serde_json::to_vec(&saved).unwrap()).unwrap();
}
