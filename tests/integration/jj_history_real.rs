use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj};
use serde::{Deserialize, Serialize};

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_history_real_colocated_collects_two_native_operations_to_saved_cutoff() {
    fixture(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_history_real_noncolocated_collects_two_native_operations_to_saved_cutoff() {
    fixture(false);
}

fn fixture(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("real history workspace");
        jj(
            &repo,
            repo.path(),
            &["git", "init", "--no-colocate", root.to_str().unwrap()],
        );
        root
    };
    jj(&repo, &root, &["describe", "-m", "history baseline"]);
    let mut fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    history_fixture_case("history:real_install", &mut fixture, Policy::Root);
    let seal = read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024);
    let mut records = Vec::new();
    for message in ["history operation A", "history operation B"] {
        jj(&fixture.repo, &fixture.root, &["describe", "-m", message]);
        let captured = capture_current_state(&fixture.context(), deadline()).unwrap();
        assert_eq!(captured.anchors().len(), 1);
        let record = captured.anchors()[0].clone();
        verify_evidence(JJ_OBSERVATION_READER_PROFILE, &record).unwrap();
        records.push(record);
    }
    assert_eq!(records[1].parent_ids, [records[0].operation_id.clone()]);
    let raw = serde_json::to_vec(&records).unwrap();
    assert!(raw.len() < 128 * 1024);
    fs::write(
        fixture
            .repo
            .test_home_path()
            .join("history-real-expected.json"),
        raw,
    )
    .unwrap();
    fs::write(
        fixture.root.join("dirty-after-history.txt"),
        b"collector never snapshots this\n",
    )
    .unwrap();
    history_fixture_case("history:real_collect", &mut fixture, Policy::Root);
    assert_eq!(
        read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024),
        seal
    );
    assert_eq!(
        fs::read(fixture.root.join("dirty-after-history.txt")).unwrap(),
        b"collector never snapshots this\n"
    );
}

#[derive(Serialize, Deserialize)]
struct Saved {
    source: String,
    receipt: String,
    attachment: String,
    baseline: String,
    heads: Vec<String>,
    anchors: Vec<JjOperationEvidence>,
}

pub fn install(case: &Case, config: &Config) {
    let (journal, registered) = support::install(case, config);
    let result = support::collect(case, &journal, config);
    let heads = registered.baseline().receipt().captured_head_ids();
    assert_eq!(heads.len(), 1);
    assert_eq!(result.head_ids(), heads);
    assert!(result.ordered_operations().is_empty());
    assert_eq!(result.reached_baseline_ids(), heads);
    assert!(!result.reaches_root());
    super::super::behavior::same_receipt(result.registration(), &registered);
    let saved = Saved {
        source: registered.source_id().to_owned(),
        receipt: registered.initialization_receipt_id().to_owned(),
        attachment: registered.attachment_id().to_owned(),
        baseline: registered.baseline().receipt().baseline_id().to_owned(),
        heads: heads.to_vec(),
        anchors: registered.baseline().anchors().to_vec(),
    };
    let raw = serde_json::to_vec(&saved).unwrap();
    assert!(raw.len() < 128 * 1024);
    fs::write(case.test_home.join("history-real-saved.json"), raw).unwrap();
}

pub fn collect(case: &Case, config: &Config) {
    let saved: Saved = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("history-real-saved.json"),
        128 * 1024,
    ))
    .unwrap();
    let records: Vec<JjOperationEvidence> = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("history-real-expected.json"),
        128 * 1024,
    ))
    .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].parent_ids, saved.heads);
    assert_eq!(records[1].parent_ids, [records[0].operation_id.clone()]);
    let journal = case.open();
    for _ in 0..2 {
        let result = support::collect(case, &journal, config);
        let registered = result.registration();
        assert_eq!(registered.source_id(), saved.source);
        assert_eq!(registered.initialization_receipt_id(), saved.receipt);
        assert_eq!(registered.attachment_id(), saved.attachment);
        assert_eq!(
            registered.baseline().receipt().baseline_id(),
            saved.baseline
        );
        assert_eq!(
            registered.baseline().receipt().captured_head_ids(),
            saved.heads
        );
        assert_eq!(registered.baseline().anchors(), saved.anchors);
        assert_eq!(registered.baseline().receipt().generation(), 1);
        assert_eq!(registered.checkout().operation_id, records[1].operation_id);
        assert!(matches!(
            registered.checkout_relation(),
            JjRegisteredCheckoutRelation::OutsideBaseline
        ));
        assert_eq!(
            result.head_ids(),
            std::slice::from_ref(&records[1].operation_id)
        );
        assert_eq!(result.ordered_operations(), records);
        assert_eq!(result.reached_baseline_ids(), saved.heads);
        assert!(!result.reaches_root());
        let status = journal.status(registered.source_id()).unwrap();
        assert_eq!(status.generation, 0);
        assert!(status.observed_heads.is_empty());
        assert!(status.applied_heads.is_empty());
        assert_eq!(status.pending_operations, 0);
    }
}
