use super::*;
use git_ai::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use git_ai::operations::jj::baseline_persistence::{
    BaselinePersistenceOutcome, persist_current_state_baseline, reopen_current_state_baseline,
};

#[test]
fn jj_capture_explicit_persistence_reopens_only_captured_heads_without_applying_checkout() {
    for outside in [false, true] {
        let fixture = Fixture::new(true);
        if outside {
            fixture.write_evidence(&left());
            fixture.set_heads(&[LEFT_ID]);
        }
        let captured = checked_capture(&fixture, &fixture.context(), deadline()).unwrap();
        let before = manifest(fixture.repo.path());
        let journal_path = fixture
            .repo
            .test_home_path()
            .join("explicit-baseline.sqlite");
        let source = "cd".repeat(32);
        let mut journal = JjObservationJournal::open_at_path(&journal_path).unwrap();
        let prepared = captured.prepare_baseline().unwrap();
        let installed =
            persist_current_state_baseline(&mut journal, &source, 0, &prepared).unwrap();
        let receipt = match installed {
            BaselinePersistenceOutcome::Installed(receipt) => receipt,
            other => panic!("first explicit persistence did not install: {other:?}"),
        };
        drop(journal);
        let mut journal = JjObservationJournal::open_at_path(&journal_path).unwrap();
        let durable =
            reopen_current_state_baseline(&journal, &source, &mut ReadBudget::new(9 * 1024 * 1024))
                .unwrap()
                .unwrap();
        assert_eq!(durable.receipt(), &receipt);
        assert_eq!(durable.anchors(), captured.anchors());
        assert_eq!(durable.receipt().captured_head_ids(), captured.head_ids());
        if outside {
            assert!(
                durable
                    .anchors()
                    .iter()
                    .all(|a| a.operation_id != captured.checkout().operation_id)
            );
        }
        assert_eq!(
            persist_current_state_baseline(&mut journal, &source, 0, &prepared).unwrap(),
            BaselinePersistenceOutcome::AlreadyInstalled(receipt)
        );
        let status = journal.status(&source).unwrap();
        assert_eq!(status.generation, 0);
        assert!(status.observed_heads.is_empty());
        assert!(status.applied_heads.is_empty());
        assert_eq!(status.pending_operations, 0);
        assert!(journal.pending(&source, 8).unwrap().is_empty());
        assert_eq!(manifest(fixture.repo.path()), before);
    }
}
