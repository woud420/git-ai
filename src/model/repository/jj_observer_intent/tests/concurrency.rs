use super::*;

#[test]
fn jj_observer_intent_busy_writer_refuses_and_later_exact_cas_succeeds() {
    let fixture = Fixture::new();
    replace(&fixture.path, &None, &record(1)).unwrap();
    let blocker = fixture.connection();
    let before = snapshot(&blocker);
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_loaded(&fixture.path, &record(1));
    let error = replace(&fixture.path, &Some(record(1)), &record(2)).unwrap_err();
    assert!(
        matches!(error, ObserverStoreError::Persistence(_)),
        "{error}"
    );
    assert!(!blocker.is_autocommit());
    assert_eq!(snapshot(&blocker), before);
    blocker.execute_batch("ROLLBACK").unwrap();
    replace(&fixture.path, &Some(record(1)), &record(2)).unwrap();
    assert_loaded(&fixture.path, &record(2));
    let after = snapshot(&blocker);
    assert!(replace(&fixture.path, &Some(record(1)), &record(2)).is_err());
    assert_eq!(snapshot(&blocker), after);
}

#[test]
fn jj_observer_intent_reads_committed_wal_and_updates_while_old_reader_is_pinned() {
    let fixture = Fixture::new();
    replace(&fixture.path, &None, &record(1)).unwrap();
    let reader = fixture.connection();
    reader.execute_batch("BEGIN DEFERRED").unwrap();
    let old_raw: Vec<u8> = reader
        .query_row(
            "SELECT payload FROM jj_observer_intent WHERE slot=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    replace(&fixture.path, &Some(record(1)), &record(2)).unwrap();
    let keeper = fixture.connection();
    let (busy, logged, checkpointed): (i64, i64, i64) = keeper
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(busy, 0);
    assert!(logged > checkpointed && checkpointed >= 0);
    assert_loaded(&fixture.path, &record(2));
    let pinned_raw: Vec<u8> = reader
        .query_row(
            "SELECT payload FROM jj_observer_intent WHERE slot=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pinned_raw, old_raw);
    replace(&fixture.path, &Some(record(2)), &record(3)).unwrap();
    assert_loaded(&fixture.path, &record(3));
    reader.execute_batch("ROLLBACK").unwrap();
}
