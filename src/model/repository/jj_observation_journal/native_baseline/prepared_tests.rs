use super::super::{NativeBaselineSnapshot, read, types::Request};
use super::PreparedBaseline;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::model::repository::sqlite::open_with_memory_limits;
use rusqlite::{Connection, TransactionBehavior};

const FROZEN_V2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-observation-v2.sql"
));

fn prepared<'a>(source: &'a str, seed: &'a NativeBaselineSnapshot) -> PreparedBaseline<'a> {
    let anchors: Vec<_> = seed.record.anchors.iter().collect();
    let request = Request::new(
        source,
        0,
        &seed.record.reader_profile,
        &seed.record.captured_head_ids,
        &anchors,
    )
    .unwrap();
    PreparedBaseline::new(request).unwrap()
}

fn counts(conn: &Connection, source: &str) -> (usize, usize) {
    let baseline = conn
        .query_row(
            "SELECT count(*) FROM jj_native_baselines WHERE source_id = ?1",
            [source],
            |row| row.get(0),
        )
        .unwrap();
    let state = conn
        .query_row(
            "SELECT count(*) FROM jj_native_sources WHERE source_id = ?1",
            [source],
            |row| row.get(0),
        )
        .unwrap();
    (baseline, state)
}

#[test]
fn prepared_baseline_inserts_leave_rollback_and_commit_to_the_caller() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native-transaction.sqlite");
    open_with_memory_limits(&path)
        .unwrap()
        .execute_batch(FROZEN_V2)
        .unwrap();
    let mut journal = JjObservationJournal::open_at_path(&path).unwrap();
    let seed = read::snapshot(
        &journal.conn,
        &format!("{:064x}", 1),
        &mut ReadBudget::new(8 * 1024 * 1024 + 128 * 1024),
    )
    .unwrap()
    .unwrap();
    let independent = open_with_memory_limits(&path).unwrap();
    let source = "02".repeat(32);

    for commit in [false, true] {
        let prepared = prepared(&source, &seed);
        let tx = journal
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let (request, expected_state) = prepared.insert_into(&tx).unwrap();
        assert_eq!(counts(&tx, &source), (1, 1));
        assert_eq!(counts(&independent, &source), (0, 0));
        let written = read::snapshot(
            &tx,
            &source,
            &mut ReadBudget::new(8 * 1024 * 1024 + 128 * 1024),
        )
        .unwrap()
        .unwrap();
        assert_eq!(written.state, expected_state);
        assert!(request.matches(&written.record));
        if commit {
            tx.commit().unwrap();
        } else {
            tx.rollback().unwrap();
        }
        let expected = if commit { (1, 1) } else { (0, 0) };
        assert_eq!(counts(&independent, &source), expected);
    }
}
