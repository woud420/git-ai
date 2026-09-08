use super::*;
use ciborium::Value;

fn two(
    case: &Case,
    config: &Config,
) -> (
    JjObservationJournal,
    RegisteredJjCurrentState,
    NativeAdmissionCursor,
    RegisteredNativeAdmission,
    RegisteredNativeAdmission,
) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&a.operation_id], None);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    case.heads(&[&b.operation_id]);
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    (journal, saved, zero, first, second)
}

fn field_mut<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    let Value::Map(fields) = value else {
        panic!("expected map")
    };
    &mut fields
        .iter_mut()
        .find(|(key, _)| key.as_text() == Some(name))
        .unwrap()
        .1
}
fn encoded(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).unwrap();
    bytes
}
fn decoded(raw: &[u8]) -> Value {
    ciborium::from_reader(raw).unwrap()
}

fn refusal(
    case: &Case,
    journal: &mut JjObservationJournal,
    saved: &RegisteredJjCurrentState,
    config: &Config,
    expected: &NativeAdmissionCursor,
    id: &str,
) {
    failed(checked_status(
        case,
        journal,
        &case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    ));
    failed(checked_known(
        case,
        journal,
        saved.source_id(),
        id,
        deadline(),
        &mut admission_budget(),
    ));
    failed(admit_checked(
        case,
        journal,
        &case.context(),
        config,
        expected.expectation(),
        deadline(),
        &mut admission_budget(),
    ));
}

pub fn corrupt(case: &Case, config: &Config) {
    let (mut journal, saved, _zero, first, second) = two(case, config);
    let conn = case.sql();
    let mut historical_id = None;
    match case.name.rsplit(':').next().unwrap() {
        "state_checksum" => {
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_admission_states SET checksum=?1",
                    ["00".repeat(32)]
                )
                .unwrap(),
                1
            );
        }
        "packet_checksum" => {
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_admissions SET checksum=?1 WHERE admission_id=?2",
                    [
                        &"00".repeat(32),
                        second.admission().receipt().admission_id()
                    ]
                )
                .unwrap(),
                1
            );
        }
        "registration" => {
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_registrations SET checksum=?1",
                    ["00".repeat(32)]
                )
                .unwrap(),
                1
            );
        }
        "native_packet" | "native_historical" => {
            let historical = case.name.ends_with(":native_historical");
            let old = if historical {
                first.admission().receipt().admission_id()
            } else {
                second.admission().receipt().admission_id()
            };
            let raw: Vec<u8> = conn
                .query_row(
                    "SELECT record FROM jj_native_admissions WHERE admission_id=?1",
                    [old],
                    |row| row.get(0),
                )
                .unwrap();
            let mut value = decoded(&raw);
            let Value::Array(operations) = field_mut(&mut value, "operations") else {
                panic!("expected operations")
            };
            let Value::Bytes(bytes) = field_mut(&mut operations[0], "operation_bytes") else {
                panic!("expected raw byte string")
            };
            assert!(!bytes.is_empty());
            bytes[0] = 0;
            let raw = encoded(&value);
            let id = hash(&raw);
            conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
            assert_eq!(conn.execute("UPDATE jj_native_admissions SET admission_id=?1, record=?2, checksum=?1 WHERE admission_id=?3", rusqlite::params![id, raw, old]).unwrap(), 1);
            if historical {
                historical_id = Some(id);
            } else {
                let state: Vec<u8> = conn
                    .query_row("SELECT state FROM jj_native_admission_states", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                let mut value = decoded(&state);
                *field_mut(&mut value, "admission_id") = Value::Text(id.clone());
                let state = encoded(&value);
                assert_eq!(conn.execute("UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
                    rusqlite::params![id,state,hash(&state)]).unwrap(), 1);
            }
        }
        other => panic!("unknown corruption {other}"),
    }
    drop(conn);
    if let Some(id) = historical_id {
        assert_eq!(status(case, &journal, config).cursor().generation(), 2);
        failed(checked_known(
            case,
            &journal,
            saved.source_id(),
            &id,
            deadline(),
            &mut admission_budget(),
        ));
        return;
    }
    refusal(
        case,
        &mut journal,
        &saved,
        config,
        second.current_cursor(),
        first.admission().receipt().admission_id(),
    );
}

pub fn gap(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&a.operation_id], None);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    let old_state: (String, Vec<u8>, String) = case
        .sql()
        .query_row(
            "SELECT admission_id,state,checksum FROM jj_native_admission_states",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    case.heads(&[&b.operation_id]);
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    let conn = case.sql();
    conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let kind = case.name.rsplit(':').next().unwrap();
    match kind {
        "state_gap" => {
            assert_eq!(
                conn.execute("DELETE FROM jj_native_admission_states", [])
                    .unwrap(),
                1
            );
        }
        "packet_gap" => {
            assert_eq!(
                conn.execute("DELETE FROM jj_native_admissions", [])
                    .unwrap(),
                2
            );
        }
        "rollback" => {
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
                    rusqlite::params![old_state.0, old_state.1, old_state.2]
                )
                .unwrap(),
                1
            );
        }
        "both_erased" => {
            conn.execute_batch(
                "DELETE FROM jj_native_admission_states; DELETE FROM jj_native_admissions;",
            )
            .unwrap();
        }
        _ => unreachable!(),
    }
    drop(conn);
    if kind == "both_erased" {
        let observed = status(case, &journal, config);
        assert_cursor(observed.cursor(), &saved, 0, &[MERGE_ID]);
        assert!(observed.latest_receipt().is_none());
        assert!(
            checked_known(
                case,
                &journal,
                saved.source_id(),
                first.admission().receipt().admission_id(),
                deadline(),
                &mut admission_budget()
            )
            .unwrap()
            .is_none()
        );
        failed(admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            second.current_cursor().expectation(),
            deadline(),
            &mut admission_budget(),
        ));
    } else {
        refusal(
            case,
            &mut journal,
            &saved,
            config,
            second.current_cursor(),
            first.admission().receipt().admission_id(),
        );
    }
}

pub fn duplicate(case: &Case, config: &Config) {
    let (mut journal, saved, _zero, first, second) = two(case, config);
    let historical = case.name.ends_with(":historical");
    let id = if historical {
        first.admission().receipt().admission_id()
    } else {
        second.admission().receipt().admission_id()
    };
    let conn = case.sql();
    conn.execute_batch(
        "PRAGMA foreign_keys=OFF;
        CREATE TABLE unindexed_admissions AS SELECT * FROM jj_native_admissions;
        DROP TABLE jj_native_admissions;
        ALTER TABLE unindexed_admissions RENAME TO jj_native_admissions;",
    )
    .unwrap();
    assert_eq!(conn.execute("INSERT INTO jj_native_admissions SELECT source_id,?1,generation,record,checksum FROM jj_native_admissions WHERE admission_id=?2", [&"cd".repeat(32), id]).unwrap(), 1);
    drop(conn);
    if historical {
        assert_eq!(status(case, &journal, config).cursor().generation(), 2);
        failed(checked_known(
            case,
            &journal,
            saved.source_id(),
            id,
            deadline(),
            &mut admission_budget(),
        ));
    } else {
        refusal(
            case,
            &mut journal,
            &saved,
            config,
            second.current_cursor(),
            id,
        );
    }
}

pub fn write_failure(case: &Case, config: &Config) {
    let (mut journal, _saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&a.operation_id], None);
    let kind = case.name.rsplit(':').next().unwrap();
    let expected = if kind.starts_with("update_") {
        let first = outcome(admit_now(case, &mut journal, config, &zero), false);
        case.heads(&[&b.operation_id]);
        first.current_cursor().clone()
    } else {
        zero
    };
    let trigger = match kind {
        "packet_abort" => {
            "BEFORE INSERT ON jj_native_admissions BEGIN SELECT RAISE(ABORT,'fixture abort'); END"
        }
        "packet_ignore" => "BEFORE INSERT ON jj_native_admissions BEGIN SELECT RAISE(IGNORE); END",
        "state_abort" => {
            "BEFORE INSERT ON jj_native_admission_states BEGIN SELECT RAISE(ABORT,'fixture abort'); END"
        }
        "state_ignore" => {
            "BEFORE INSERT ON jj_native_admission_states BEGIN SELECT RAISE(IGNORE); END"
        }
        "update_abort" => {
            "BEFORE UPDATE ON jj_native_admission_states BEGIN SELECT RAISE(ABORT,'fixture abort'); END"
        }
        "update_ignore" => {
            "BEFORE UPDATE ON jj_native_admission_states BEGIN SELECT RAISE(IGNORE); END"
        }
        "packet_delete" => {
            "AFTER INSERT ON jj_native_admission_states BEGIN DELETE FROM jj_native_admission_states WHERE source_id=NEW.source_id; DELETE FROM jj_native_admissions WHERE source_id=NEW.source_id AND admission_id=NEW.admission_id; END"
        }
        "registration_rewrite" => {
            "AFTER INSERT ON jj_native_admission_states BEGIN UPDATE jj_native_registrations SET checksum='corrupt'; END"
        }
        _ => unreachable!(),
    };
    case.sql()
        .execute_batch(&format!("CREATE TRIGGER admission_fixture {trigger}"))
        .unwrap();
    if kind == "packet_delete" {
        let conn = case.sql();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            1
        );
        conn.execute_batch("SAVEPOINT deletion_fixture").unwrap();
        let id = "ef".repeat(32);
        assert_eq!(
            conn.execute(
                "INSERT INTO jj_native_admissions VALUES (?1,?2,1,?3,?2)",
                rusqlite::params![
                    expected.source_id(),
                    id,
                    b"SQL trigger calibration".as_slice()
                ]
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.execute(
                "INSERT INTO jj_native_admission_states VALUES (?1,?2,?3,?2)",
                rusqlite::params![
                    expected.source_id(),
                    id,
                    b"SQL trigger calibration".as_slice()
                ]
            )
            .unwrap(),
            1
        );
        for table in support::ADMISSION_TABLES {
            assert_eq!(
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                    .get::<_, usize>(0))
                    .unwrap(),
                0
            );
        }
        conn.execute_batch("ROLLBACK TO deletion_fixture; RELEASE deletion_fixture;")
            .unwrap();
    }
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        expected.expectation(),
        deadline(),
        &mut admission_budget(),
    ));
}

fn registration_bytes(case: &Case) -> usize {
    let conn = case.sql();
    [
        ("jj_native_registrations", "record"),
        ("jj_native_workspaces", "record"),
        ("jj_native_baselines", "record"),
        ("jj_native_sources", "state"),
    ]
    .into_iter()
    .map(|(table, column)| {
        conn.query_row(
            &format!("SELECT length({column}) FROM {table}"),
            [],
            |row| row.get::<_, usize>(0),
        )
        .unwrap()
    })
    .sum()
}
fn selected_bytes(case: &Case, distinct: Option<&str>) -> usize {
    let conn = case.sql();
    let state: usize = conn
        .query_row(
            "SELECT length(state) FROM jj_native_admission_states",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let latest: usize = conn.query_row("SELECT length(record) FROM jj_native_admissions WHERE admission_id=(SELECT admission_id FROM jj_native_admission_states)", [], |row| row.get(0)).unwrap();
    let extra = distinct
        .map(|id| {
            conn.query_row(
                "SELECT length(record) FROM jj_native_admissions WHERE admission_id=?1",
                [id],
                |row| row.get::<_, usize>(0),
            )
            .unwrap()
        })
        .unwrap_or(0);
    registration_bytes(case) + state + latest + extra
}

pub fn read_budget(case: &Case, config: &Config) {
    let (journal, saved, _zero, first, second) = two(case, config);
    let required = selected_bytes(case, None);
    let mut reads = ReadBudget::new(required);
    checked_status(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut reads,
    )
    .unwrap();
    assert_eq!(reads.consumed(), required);
    assert_eq!(reads.remaining(), 0);
    failed(checked_status(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut reads,
    ));
    assert_eq!(reads.consumed(), required);
    let mut reads = ReadBudget::new(required);
    checked_known(
        case,
        &journal,
        saved.source_id(),
        second.admission().receipt().admission_id(),
        deadline(),
        &mut reads,
    )
    .unwrap()
    .unwrap();
    assert_eq!(reads.consumed(), required);
    let required = selected_bytes(case, Some(first.admission().receipt().admission_id()));
    let mut reads = ReadBudget::new(required);
    checked_known(
        case,
        &journal,
        saved.source_id(),
        first.admission().receipt().admission_id(),
        deadline(),
        &mut reads,
    )
    .unwrap()
    .unwrap();
    assert_eq!(reads.consumed(), required);
    assert_eq!(reads.remaining(), 0);
    let mut short = ReadBudget::new(required - 1);
    failed(checked_known(
        case,
        &journal,
        saved.source_id(),
        first.admission().receipt().admission_id(),
        deadline(),
        &mut short,
    ));
    assert!(short.consumed() > 0 && short.consumed() < required);
}

fn erase_admissions(case: &Case) {
    case.sql()
        .execute_batch("DELETE FROM jj_native_admission_states; DELETE FROM jj_native_admissions;")
        .unwrap();
}

pub fn write_budget(case: &Case, config: &Config) {
    let (mut journal, _saved, zero) = initial(case, config);
    let a = native::rich_parent();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    let mut probe = admission_budget();
    let first = outcome(
        admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            zero.expectation(),
            deadline(),
            &mut probe,
        )
        .unwrap(),
        false,
    );
    let required = probe.consumed();
    assert!(required > registration_bytes(case));
    let id = first.admission().receipt().admission_id().to_owned();
    // Reset only the two optional tables to calibrate the identical fresh-write path.
    erase_admissions(case);
    let mut exact = ReadBudget::new(required);
    let repeated = outcome(
        admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            zero.expectation(),
            deadline(),
            &mut exact,
        )
        .unwrap(),
        false,
    );
    assert_eq!(repeated.admission().receipt().admission_id(), id);
    assert_eq!(exact.consumed(), required);
    assert_eq!(exact.remaining(), 0);
    erase_admissions(case);
    let mut short = ReadBudget::new(required - 1);
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        zero.expectation(),
        deadline(),
        &mut short,
    ));
    assert!(short.consumed() > 0 && short.consumed() < required);
    assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));
}
