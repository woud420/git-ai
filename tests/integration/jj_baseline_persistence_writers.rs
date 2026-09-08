use super::*;
use git_ai::error::Retryability;
use std::sync::{Arc, Barrier};

fn retryable(error: &JjBaselinePersistenceError) -> bool {
    let Some(JournalError::Persistence(error)) =
        std::error::Error::source(error).and_then(|source| source.downcast_ref::<JournalError>())
    else {
        return false;
    };
    matches!(error.retryability(), Retryability::Retryable { .. })
}

fn race(
    fixture: &Fixture,
    anchors: [JjOperationEvidence; 2],
) -> Vec<Result<BaselinePersistenceOutcome, JjBaselinePersistenceError>> {
    let journals = [fixture.open(), fixture.open()];
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let writers = journals
            .into_iter()
            .zip(anchors)
            .map(|(mut journal, anchor)| {
                let source = fixture.source.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    let mut result = install(&mut journal, &source, std::slice::from_ref(&anchor));
                    for _ in 0..3 {
                        if !result.as_ref().is_err_and(retryable) {
                            break;
                        }
                        std::thread::yield_now();
                        result = install(&mut journal, &source, std::slice::from_ref(&anchor));
                    }
                    result
                })
            })
            .collect::<Vec<_>>();
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect()
    })
}

#[test]
fn jj_baseline_persistence_two_distinct_writers_have_one_generation_zero_winner() {
    let fixture = Fixture::new();
    let outcomes = race(&fixture, [first(), left()]);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Ok(BaselinePersistenceOutcome::Installed(_))))
            .count(),
        1
    );
    assert_eq!(outcomes.iter().filter(|result| result.is_err()).count(), 1);
    let mut winner = None;
    for outcome in outcomes {
        match outcome {
            Ok(BaselinePersistenceOutcome::Installed(receipt)) => winner = Some(receipt),
            Ok(BaselinePersistenceOutcome::AlreadyInstalled(_)) => {
                panic!("distinct requests shared a receipt")
            }
            Err(error) => {
                rejected::<BaselinePersistenceOutcome>(Err(error), "conflict");
            }
        }
    }
    let durable = reopen(&fixture.open(), &fixture.source).unwrap().unwrap();
    assert_same_receipt(durable.receipt(), &winner.unwrap());
    assert_eq!(native_counts(&fixture), (1, 1));
}

#[test]
fn jj_baseline_persistence_two_identical_writers_share_one_immutable_receipt() {
    let fixture = Fixture::new();
    let outcomes = race(&fixture, [merge(), merge()]);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Ok(BaselinePersistenceOutcome::Installed(_))))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Ok(BaselinePersistenceOutcome::AlreadyInstalled(_))))
            .count(),
        1
    );
    let mut receipts = outcomes.into_iter().map(|outcome| match outcome.unwrap() {
        BaselinePersistenceOutcome::Installed(receipt)
        | BaselinePersistenceOutcome::AlreadyInstalled(receipt) => receipt,
    });
    assert_same_receipt(&receipts.next().unwrap(), &receipts.next().unwrap());
    assert_eq!(native_counts(&fixture), (1, 1));
}
