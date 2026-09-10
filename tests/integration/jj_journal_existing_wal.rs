use super::*;

#[test]
fn jj_journal_existing_sees_uncheckpointed_commits_without_reserving_a_writer() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let keeper = wal_connection(&fixture);
    keeper.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    assert_eq!(
        rows(&keeper, "PRAGMA wal_checkpoint(TRUNCATE)"),
        [vec![
            Value::Integer(0),
            Value::Integer(0),
            Value::Integer(0)
        ]]
    );
    let empty_before = readonly_snapshot(&keeper);
    let empty = JjObservationJournal::open_existing_at_path(&fixture.path).unwrap();
    assert_eq!(empty.status(&fixture.source).unwrap().generation, 0);
    assert!(empty.pending(&fixture.source, 1).unwrap().is_empty());
    assert_eq!(readonly_snapshot(&keeper), empty_before);
    drop(empty);
    let pinned = open_with_memory_limits(&fixture.path).unwrap();
    pinned.execute_batch("BEGIN").unwrap();
    let old_count: i64 = pinned
        .query_row("SELECT COUNT(*) FROM jj_sources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(old_count, 0);
    let mut writer = fixture.open();
    fixture.capture_first(&mut writer);
    let (_, logged, checkpointed): (i64, i64, i64) = keeper
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert!(
        checkpointed >= 0 && logged > checkpointed,
        "fixture needs committed frames beyond the pinned snapshot: {logged}/{checkpointed}"
    );
    assert_eq!(
        pinned
            .query_row("SELECT COUNT(*) FROM jj_sources", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let before = readonly_snapshot(&keeper);

    keeper.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut journal = JjObservationJournal::open_existing_at_path(&fixture.path).unwrap();
    fixture.assert_only_first(&journal);
    assert_eq!(readonly_snapshot(&keeper), before);
    keeper.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        journal
            .capture(&batch(
                &fixture.source,
                1,
                &[1],
                &[2],
                vec![evidence(2, &[1])]
            ))
            .unwrap(),
        CaptureOutcome::Captured {
            inserted_operations: 1
        }
    );
    let reader = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
    assert_eq!(reader.status(&fixture.source).unwrap().generation, 2);
    assert_eq!(
        reader.pending(&fixture.source, 2).unwrap(),
        [evidence(1, &[0]), evidence(2, &[1])]
    );
    pinned.execute_batch("ROLLBACK").unwrap();
}
