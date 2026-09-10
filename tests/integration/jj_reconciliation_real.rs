use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj};
use serde::Deserialize;

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_reconciliation_real_colocated_skips_unchanged_and_retries_changed() {
    fixture(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_reconciliation_real_noncolocated_reuses_explicit_changed_receipt() {
    fixture(false);
}

fn fixture(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("real reconciliation workspace");
        jj(
            &repo,
            repo.path(),
            &["git", "init", "--no-colocate", root.to_str().unwrap()],
        );
        root
    };
    jj(&repo, &root, &["describe", "-m", "reconciliation baseline"]);
    let mut fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    history_fixture_case("history:admission:real_install", &mut fixture, Policy::Root);
    let seal = read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024);
    history_fixture_case(
        "history:admission:reconcile:real:before",
        &mut fixture,
        Policy::Root,
    );
    jj(
        &fixture.repo,
        &fixture.root,
        &["describe", "-m", "actual reconciliation successor"],
    );
    let captured = capture_current_state(&fixture.context(), deadline()).unwrap();
    assert_eq!(captured.anchors().len(), 1);
    let raw = serde_json::to_vec(&captured.anchors()[0]).unwrap();
    assert!(raw.len() < 128 * 1024);
    fs::write(
        fixture
            .repo
            .test_home_path()
            .join("reconciliation-real-expected.json"),
        raw,
    )
    .unwrap();
    fs::write(
        fixture.root.join("dirty-after-reconciliation.txt"),
        b"never snapshotted by reconciliation\n",
    )
    .unwrap();
    history_fixture_case(
        "history:admission:reconcile:real:after",
        &mut fixture,
        Policy::Root,
    );
    assert_eq!(
        read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024),
        seal
    );
    assert_eq!(
        fs::read(fixture.root.join("dirty-after-reconciliation.txt")).unwrap(),
        b"never snapshotted by reconciliation\n"
    );
}

#[derive(Deserialize)]
struct Saved {
    source: String,
    initialization: String,
    attachment: String,
    baseline: String,
    baseline_heads: Vec<String>,
    anchors: Vec<JjOperationEvidence>,
    generation: u64,
    admitted_heads: Vec<String>,
}

impl Saved {
    fn expected(&self) -> NativeAdmissionExpectation<'_> {
        NativeAdmissionExpectation {
            source_id: &self.source,
            initialization_receipt_id: &self.initialization,
            baseline_id: &self.baseline,
            generation: self.generation,
            admitted_head_ids: &self.admitted_heads,
        }
    }

    fn assert_registration(&self, value: &RegisteredJjCurrentState) {
        assert_eq!(value.source_id(), self.source);
        assert_eq!(value.initialization_receipt_id(), self.initialization);
        assert_eq!(value.attachment_id(), self.attachment);
        assert_eq!(value.baseline().receipt().baseline_id(), self.baseline);
        assert_eq!(
            value.baseline().receipt().captured_head_ids(),
            self.baseline_heads
        );
        assert_eq!(value.baseline().anchors(), self.anchors);
        assert_eq!(value.baseline().receipt().generation(), 1);
    }
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    let saved: Saved = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("admission-real-saved.json"),
        128 * 1024,
    ))
    .unwrap();
    assert_eq!(saved.generation, 0);
    assert_eq!(saved.admitted_heads, saved.baseline_heads);
    assert_eq!(saved.baseline_heads.len(), 1);
    let target = SavedTarget {
        workspace_name: "default".to_owned(),
        attachment_id: saved.attachment.clone(),
    };
    let mut journal = case.open();
    if case.name.ends_with(":before") {
        for _ in 0..3 {
            let result = checks::unchanged(
                reconcile(
                    case,
                    &mut journal,
                    config,
                    target.with(saved.expected()),
                    deadline(),
                    &mut admission_budget(),
                )
                .unwrap(),
            );
            saved.assert_registration(result.registration());
            assert_eq!(result.cursor().generation(), 0);
            assert_eq!(result.cursor().admitted_head_ids(), saved.baseline_heads);
            assert!(result.latest_receipt().is_none());
            assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));
        }
        return;
    }
    assert!(case.name.ends_with(":after"));
    let record: JjOperationEvidence = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("reconciliation-real-expected.json"),
        128 * 1024,
    ))
    .unwrap();
    verify_evidence(JJ_OBSERVATION_READER_PROFILE, &record).unwrap();
    assert_eq!(record.parent_ids, saved.baseline_heads);
    let current = status(case, &journal, config);
    assert_eq!(current.cursor().generation(), 0);
    let result = if case.context().colocated {
        admitted(
            reconcile(
                case,
                &mut journal,
                config,
                target.with(saved.expected()),
                deadline(),
                &mut admission_budget(),
            )
            .unwrap(),
            false,
        )
    } else {
        // Packet identity is shared with the explicit API and carries no caller provenance.
        outcome(
            admit_now(case, &mut journal, config, current.cursor()),
            false,
        )
    };
    saved.assert_registration(result.registration());
    assert_eq!(
        result.registration().checkout().operation_id,
        record.operation_id
    );
    assert_eq!(
        result.registration().checkout_relation(),
        JjRegisteredCheckoutRelation::OutsideBaseline
    );
    assert_eq!(
        result.admission().ordered_operations(),
        std::slice::from_ref(&record)
    );
    assert_eq!(
        result.admission().reached_baseline_ids(),
        saved.baseline_heads
    );
    assert!(!result.admission().reaches_root());
    assert_eq!(result.current_cursor().generation(), 1);
    assert_eq!(
        result.current_cursor().admitted_head_ids(),
        std::slice::from_ref(&record.operation_id)
    );
    let id = result.admission().receipt().admission_id().to_owned();
    drop(journal);
    let mut journal = case.open();
    let retry = admitted(
        reconcile(
            case,
            &mut journal,
            config,
            target.with(saved.expected()),
            deadline(),
            &mut admission_budget(),
        )
        .unwrap(),
        true,
    );
    assert_eq!(retry.admission().receipt().admission_id(), id);
    assert_eq!(retry.current_cursor().generation(), 1);
    saved.assert_registration(retry.registration());
    for _ in 0..3 {
        let unchanged = checks::unchanged(now(
            case,
            &mut journal,
            config,
            &target,
            retry.current_cursor(),
        ));
        saved.assert_registration(unchanged.registration());
        assert_eq!(
            unchanged.registration().checkout().operation_id,
            record.operation_id
        );
        assert_eq!(unchanged.cursor().generation(), 1);
        assert_eq!(unchanged.latest_receipt().unwrap().admission_id(), id);
    }
    assert_eq!(
        packet(case, &journal, &saved.source, &id).ordered_operations(),
        [record]
    );
    assert_eq!(
        admission_rows(case)
            .iter()
            .map(|(_, rows)| rows.len())
            .collect::<Vec<_>>(),
        [1, 1]
    );
}
