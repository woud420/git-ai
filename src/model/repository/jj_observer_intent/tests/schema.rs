use super::*;

#[test]
fn jj_observer_intent_schema_drift_and_versions_are_not_repaired() {
    let variants = [
        DDL.replace("payload BLOB NOT NULL", "payload TEXT NOT NULL"),
        DDL.replace("payload BLOB NOT NULL", "payload BLOB"),
        DDL.replace("revision INTEGER NOT NULL", "revision TEXT NOT NULL"),
        DDL.replace("slot INTEGER PRIMARY KEY", "slot INTEGER"),
        DDL.replace(
            ", payload BLOB NOT NULL CHECK (length(payload) <= 524288)",
            "",
        ),
        DDL.replace("payload BLOB NOT NULL", "extra TEXT, payload BLOB NOT NULL"),
        format!("{DDL}; CREATE TABLE unexpected(value BLOB)"),
        format!("{DDL}; CREATE VIEW unexpected AS SELECT slot FROM jj_observer_intent"),
        format!("{DDL}; CREATE INDEX unexpected ON jj_observer_intent(revision)"),
        format!(
            "{DDL}; CREATE TRIGGER unexpected AFTER UPDATE ON jj_observer_intent BEGIN SELECT RAISE(FAIL,'blocked'); END"
        ),
    ];
    for (index, ddl) in variants.iter().enumerate() {
        let fixture = Fixture::new();
        let connection = fixture.manual(ddl, 1, true);
        let before = snapshot(&connection);
        assert!(load(&fixture.path).is_err(), "schema variant {index}");
        assert!(
            replace(&fixture.path, &None, &record(1)).is_err(),
            "schema variant {index}"
        );
        assert_eq!(snapshot(&connection), before, "schema variant {index}");
    }
    for version in [0, 2, i32::MAX as i64] {
        let fixture = Fixture::new();
        let connection = fixture.manual(DDL, version, true);
        raw_insert(&connection, 1, &encoded(&record(1)));
        let before = snapshot(&connection);
        assert!(load(&fixture.path).is_err(), "version {version}");
        assert!(replace(&fixture.path, &Some(record(1)), &record(2)).is_err());
        assert_eq!(snapshot(&connection), before);
    }
}

#[test]
fn jj_observer_intent_requires_existing_wal_without_mode_conversion() {
    let fixture = Fixture::new();
    let connection = fixture.manual(DDL, 1, false);
    raw_insert(&connection, 1, &encoded(&record(1)));
    let before = snapshot(&connection);
    assert!(load(&fixture.path).is_err());
    assert!(replace(&fixture.path, &Some(record(1)), &record(2)).is_err());
    assert_eq!(snapshot(&connection), before);
    drop(connection);
    let connection = fixture.connection();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    assert_loaded(&fixture.path, &record(1));
    replace(&fixture.path, &Some(record(1)), &record(2)).unwrap();
    assert_loaded(&fixture.path, &record(2));
}

#[test]
fn jj_observer_intent_slot_cardinality_and_revision_scalars_are_checked() {
    for statement in [
        "UPDATE jj_observer_intent SET slot=2",
        "INSERT INTO jj_observer_intent SELECT 2,revision,payload FROM jj_observer_intent",
        "UPDATE jj_observer_intent SET revision=0",
        "UPDATE jj_observer_intent SET revision=-1",
        "UPDATE jj_observer_intent SET revision=1.5",
        "UPDATE jj_observer_intent SET revision='wrong'",
        "UPDATE jj_observer_intent SET revision=x'01'",
        "UPDATE jj_observer_intent SET revision=2",
    ] {
        let fixture = Fixture::new();
        let connection = fixture.manual(DDL, 1, true);
        connection
            .pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        raw_insert(&connection, 1, &encoded(&record(1)));
        connection.execute_batch(statement).unwrap();
        let before = snapshot(&connection);
        assert_validation(load(&fixture.path).unwrap_err());
        assert!(replace(&fixture.path, &Some(record(1)), &record(2)).is_err());
        assert_eq!(snapshot(&connection), before, "{statement}");
    }
}
