use super::*;

#[derive(Clone, Copy)]
enum Branch {
    Unchanged,
    Changed,
    Retry,
}

fn fixture(branch: Branch) -> (Case, JjObservationJournal, NativeAdmissionCursor) {
    let (case, mut journal, _) = Case::new();
    let expected = install_selected_fixture(&case);
    if matches!(branch, Branch::Changed | Branch::Retry) {
        let record = fixtures::rich_parent();
        case.select(std::slice::from_ref(&record), &record);
        if matches!(branch, Branch::Retry) {
            let first =
                admitted(run(&case, &mut journal, &expected, &mut Hook::new(|_| {})).unwrap());
            assert_eq!(first.current_cursor().generation(), 1);
            assert_eq!(first.admission().receipt().expected_generation(), 0);
        }
    }
    let current = status(&case);
    assert_eq!(
        current.registration().workspace_name(),
        case.registered.workspace_name()
    );
    assert_eq!(
        current.registration().attachment_id(),
        case.registered.attachment_id()
    );
    assert_eq!(
        current.cursor().initialization_receipt_id(),
        expected.initialization_receipt_id()
    );
    (case, journal, expected)
}

fn replacement(case: &Case) -> Vec<u8> {
    let (_, mut selected) = load_records(&case.db);
    selected.attachment_id = if selected.attachment_id == "d".repeat(64) {
        "e"
    } else {
        "d"
    }
    .repeat(64);
    encode(&selected)
}

fn replace_selected(case: &Case) {
    let before = status(case).cursor().clone();
    let raw = replacement(case);
    let connection = open_with_memory_limits(&case.db).unwrap();
    assert_eq!(connection.execute(
        "UPDATE jj_native_workspaces SET record=?1,checksum=?2 WHERE workspace_name='default'",
        params![raw,checksum(&raw)]
    ).unwrap(), 1);
    drop(connection);
    let after = status(case);
    assert_eq!(after.cursor(), &before);
    assert_eq!(
        after.registration().workspace_name(),
        case.registered.workspace_name()
    );
    assert_ne!(
        after.registration().attachment_id(),
        case.registered.attachment_id()
    );
}

fn require_target_error(error: &JjNativeAdmissionError) {
    let text = error.to_string();
    assert!(
        text.contains("target") || text.contains("workspace") || text.contains("attachment"),
        "{text}"
    );
}

#[test]
fn reconciliation_target_private_remembered_attachment_is_checked_before_any_branch() {
    for branch in [Branch::Unchanged, Branch::Changed, Branch::Retry] {
        let (case, mut journal, expected) = fixture(branch);
        replace_selected(&case);
        let sql = sql_snapshot(&case.db, true);
        let files = filesystem(&case.fixture);
        let mut hooks = Hook::new(|_| panic!("wrong target reached a reconciliation phase"));
        let error = refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
        require_target_error(&error);
        assert!(hooks.phases.is_empty());
        assert!(sql_snapshot(&case.db, true) == sql);
        assert!(filesystem(&case.fixture) == files);
        writer_reservation(&case.db, false);
    }
}

#[test]
fn reconciliation_target_private_history_to_transaction_replacement_refuses_new_and_retry() {
    for branch in [Branch::Changed, Branch::Retry] {
        let (case, mut journal, expected) = fixture(branch);
        let files = filesystem(&case.fixture);
        let mut injected_sql = None;
        let mut hooks = Hook::new(|phase| {
            assert_eq!(phase, AdmissionPhase::HistoryCollected);
            replace_selected(&case);
            injected_sql = Some(sql_snapshot(&case.db, true));
        });
        let error = refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
        require_target_error(&error);
        assert_eq!(hooks.phases, [AdmissionPhase::HistoryCollected]);
        drop(hooks);
        assert!(sql_snapshot(&case.db, true) == injected_sql.unwrap());
        assert!(filesystem(&case.fixture) == files);
        writer_reservation(&case.db, false);
    }
}

fn install_readback_replacement(case: &Case) {
    let replacement = replacement(case);
    let connection = open_with_memory_limits(&case.db).unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TRIGGER replace_selected_attachment AFTER INSERT ON jj_native_admission_states
         BEGIN UPDATE jj_native_workspaces SET record=X'{}', checksum='{}'
         WHERE source_id=NEW.source_id AND workspace_name='default'; END",
            crate::operations::jj::content_hash::hex(&replacement),
            checksum(&replacement)
        ))
        .unwrap();
    let old = sql_snapshot(&case.db, true);
    connection
        .execute_batch("SAVEPOINT calibrated_replacement")
        .unwrap();
    let id = "a".repeat(64);
    connection.execute(
        "INSERT INTO jj_native_admissions(source_id,admission_id,generation,record,checksum) VALUES(?1,?2,1,X'00',?2)",
        params![case.registered.source_id(),id]
    ).unwrap();
    connection.execute(
        "INSERT INTO jj_native_admission_states(source_id,admission_id,state,checksum) VALUES(?1,?2,X'00',?2)",
        params![case.registered.source_id(),id]
    ).unwrap();
    let changed: Vec<u8> = connection
        .query_row(
            "SELECT record FROM jj_native_workspaces WHERE workspace_name='default'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(changed, replacement);
    connection
        .execute_batch("ROLLBACK TO calibrated_replacement; RELEASE calibrated_replacement")
        .unwrap();
    drop(connection);
    assert!(sql_snapshot(&case.db, true) == old);
}

#[test]
fn reconciliation_target_private_selected_readback_rewrite_rolls_back_new_admission() {
    let (case, mut journal, expected) = fixture(Branch::Changed);
    install_readback_replacement(&case);
    let sql = sql_snapshot(&case.db, true);
    let files = filesystem(&case.fixture);
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::PriorSnapshotVerified {
            writer_reservation(&case.db, true);
            assert_eq!(admission_counts(&case.db), [0, 0]);
        }
    });
    refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
    assert_eq!(
        hooks.phases,
        [
            AdmissionPhase::HistoryCollected,
            AdmissionPhase::PriorSnapshotVerified
        ]
    );
    assert!(sql_snapshot(&case.db, true) == sql);
    assert!(filesystem(&case.fixture) == files);
    assert_eq!(
        status(&case).registration().attachment_id(),
        case.registered.attachment_id()
    );
    assert_eq!(admission_counts(&case.db), [0, 0]);
    writer_reservation(&case.db, false);
}
