use super::*;

#[test]
fn jj_journal_readonly_reads_live_committed_wal_without_a_write_transaction() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let keeper = open_with_memory_limits(&fixture.path).unwrap();
    keeper.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    let checkpoint = rows(&keeper, "PRAGMA wal_checkpoint(TRUNCATE)");
    assert_eq!(
        checkpoint,
        [vec![
            Value::Integer(0),
            Value::Integer(0),
            Value::Integer(0)
        ]]
    );
    let main_before = fs::read(&fixture.path).unwrap();
    let mut writer = fixture.open();
    fixture.capture_first(&mut writer);
    assert_eq!(
        fs::read(&fixture.path).unwrap(),
        main_before,
        "fixture write escaped the WAL"
    );
    let wal = fixture.path.with_file_name(format!(
        "{}-wal",
        fixture.path.file_name().unwrap().to_str().unwrap()
    ));
    assert!(fs::metadata(wal).unwrap().len() > 32);
    let first_snapshot = readonly_snapshot(&keeper);

    keeper.execute_batch("BEGIN IMMEDIATE").unwrap();
    let journal = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
    fixture.assert_only_first(&journal);
    assert!(readonly_snapshot(&keeper) == first_snapshot);
    keeper.execute_batch("ROLLBACK").unwrap();

    writer
        .capture(&batch(
            &fixture.source,
            1,
            &[1],
            &[2],
            vec![evidence(2, &[1])],
        ))
        .unwrap();
    let second_snapshot = readonly_snapshot(&keeper);
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 2);
    assert_eq!(
        journal.pending(&fixture.source, 2).unwrap(),
        [evidence(1, &[0]), evidence(2, &[1])]
    );
    drop(journal);
    let reopened = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
    assert_eq!(reopened.status(&fixture.source).unwrap().generation, 2);
    assert!(readonly_snapshot(&keeper) == second_snapshot);
    assert_eq!(fs::read(&fixture.path).unwrap(), main_before);
}
