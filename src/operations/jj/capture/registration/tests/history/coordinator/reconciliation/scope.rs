use super::*;
use crate::model::repository::jj_observation_journal::registration::{
    ByteString, RegistrationRecord, WorkspaceRecord, workspace_locator_guard, workspace_root_guard,
};
use rusqlite::params;
use serde::Serialize;
use sha2::{Digest, Sha256};

fn encode(value: &impl Serialize) -> Vec<u8> {
    let mut raw = Vec::new();
    ciborium::into_writer(value, &mut raw).unwrap();
    raw
}

fn checksum(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn load_records(path: &Path) -> (RegistrationRecord, WorkspaceRecord) {
    let connection = open_with_memory_limits(path).unwrap();
    let source: Vec<u8> = connection
        .query_row("SELECT record FROM jj_native_registrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    let workspace: Vec<u8> = connection
        .query_row(
            "SELECT record FROM jj_native_workspaces WHERE workspace_name='default'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    (
        ciborium::from_reader(source.as_slice()).unwrap(),
        ciborium::from_reader(workspace.as_slice()).unwrap(),
    )
}

fn save_source(connection: &rusqlite::Connection, source: &RegistrationRecord) {
    let raw = encode(source);
    assert_eq!(
        connection
            .execute(
                "UPDATE jj_native_registrations SET record=?1,checksum=?2",
                params![raw, checksum(&raw)]
            )
            .unwrap(),
        1
    );
}

fn install_selected_fixture(case: &Case) -> NativeAdmissionCursor {
    let (mut source, selected) = load_records(&case.db);
    let (_, mut original) = load_records(&case.db);
    let name = "workspace-工";
    original.workspace_name = name.to_owned();
    original
        .locator
        .workspace_root
        .0
        .extend_from_slice(b"/historical-original");
    original.workspace_binding.directories[0].inode ^= 1;
    assert_eq!(
        original.selected_checkout.operation_id,
        fixtures::merge().operation_id
    );
    let checkout_path = case.fixture.root.join(".jj/working_copy/checkout");
    let physical_checkout = fs::read(&checkout_path).unwrap();
    case.fixture.write_checkout(name);
    let checkout = fs::read(&checkout_path).unwrap();
    fs::write(&checkout_path, &physical_checkout).unwrap();
    original.selected_checkout.raw_checkout_bytes = ByteString(checkout);
    let original_raw = encode(&original);
    source.initial_workspace_name = name.to_owned();
    source.initial_workspace_record_id = checksum(&original_raw);
    let connection = open_with_memory_limits(&case.db).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(connection.execute(
        "UPDATE jj_native_workspaces SET workspace_name=?1,locator_key=?2,workspace_root_key=?3,record=?4,checksum=?5",
        params![name,workspace_locator_guard(&original.locator).unwrap(),
            workspace_root_guard(&original.locator,&original.workspace_binding).unwrap(),
            original_raw,checksum(&original_raw)]
    ).unwrap(), 1);
    let selected_raw = encode(&selected);
    assert_eq!(connection.execute(
        "INSERT INTO jj_native_workspaces(source_id,workspace_name,locator_key,workspace_root_key,record,checksum)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![selected.source_id,selected.workspace_name,workspace_locator_guard(&selected.locator).unwrap(),
            workspace_root_guard(&selected.locator,&selected.workspace_binding).unwrap(),
            selected_raw,checksum(&selected_raw)]
    ).unwrap(), 1);
    save_source(&connection, &source);
    connection.execute_batch("COMMIT").unwrap();
    drop(connection);
    // Only the selected workspace is physically sampled. The independent saved
    // original has valid checkout/native evidence and deliberately historical locators.
    let checked = status(case);
    assert_eq!(checked.registration().workspace_name(), "default");
    assert_eq!(checked.cursor().generation(), 0);
    assert_ne!(
        checked.cursor().initialization_receipt_id(),
        case.registered.initialization_receipt_id()
    );
    checked.cursor().clone()
}

#[test]
fn reconciliation_private_selected_attachment_cannot_change_under_an_unchanged_source_receipt() {
    let (case, mut journal, _) = Case::new();
    let expected = install_selected_fixture(&case);
    let files = filesystem(&case.fixture);
    let old_attachment = status(&case).registration().attachment_id().to_owned();
    let mut injected_sql = None;
    let mut hooks = Hook::new(|phase| {
        assert_eq!(phase, AdmissionPhase::UnchangedCandidate);
        let (_, mut selected) = load_records(&case.db);
        selected.attachment_id = if old_attachment == "d".repeat(64) {
            "e"
        } else {
            "d"
        }
        .repeat(64);
        let raw = encode(&selected);
        let connection = open_with_memory_limits(&case.db).unwrap();
        assert_eq!(connection.execute(
            "UPDATE jj_native_workspaces SET record=?1,checksum=?2 WHERE workspace_name='default'",
            params![raw,checksum(&raw)]
        ).unwrap(), 1);
        drop(connection);
        let checked = status(&case);
        assert_eq!(checked.cursor(), &expected);
        assert_ne!(checked.registration().attachment_id(), old_attachment);
        injected_sql = Some(sql_snapshot(&case.db, true));
    });
    refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
    assert_eq!(hooks.phases, [AdmissionPhase::UnchangedCandidate]);
    drop(hooks);
    assert!(sql_snapshot(&case.db, true) == injected_sql.unwrap());
    assert!(filesystem(&case.fixture) == files);
    writer_reservation(&case.db, false);
}

#[test]
fn reconciliation_private_candidate_reloads_source_and_baseline_integrity() {
    for field in [
        "source",
        "profile",
        "baseline",
        "baseline_generation",
        "selected_name",
    ] {
        let (case, mut journal, expected, _) = setup(false);
        let files = filesystem(&case.fixture);
        let mut injected_sql = None;
        let mut hooks = Hook::new(|phase| {
            assert_eq!(phase, AdmissionPhase::UnchangedCandidate);
            let (mut source, mut selected) = load_records(&case.db);
            match field {
                "source" => source.source_id = "f".repeat(64),
                "profile" => {
                    source.reader_profile = "jj-simple-op-store/future".to_owned();
                    selected.reader_profile.clone_from(&source.reader_profile);
                }
                "baseline" => {
                    source.baseline_id = "a".repeat(64);
                    selected.baseline_id.clone_from(&source.baseline_id);
                }
                "baseline_generation" => {
                    source.baseline_generation = 2;
                    selected.baseline_generation = 2;
                }
                "selected_name" => selected.workspace_name = "renamed".to_owned(),
                _ => unreachable!(),
            }
            let selected_raw = encode(&selected);
            source.initial_workspace_record_id = checksum(&selected_raw);
            let connection = open_with_memory_limits(&case.db).unwrap();
            connection.execute_batch("BEGIN IMMEDIATE").unwrap();
            assert_eq!(
                connection
                    .execute(
                        "UPDATE jj_native_workspaces SET record=?1,checksum=?2",
                        params![selected_raw, checksum(&selected_raw)]
                    )
                    .unwrap(),
                1
            );
            save_source(&connection, &source);
            connection.execute_batch("COMMIT").unwrap();
            drop(connection);
            injected_sql = Some(sql_snapshot(&case.db, true));
        });
        refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
        assert_eq!(hooks.phases, [AdmissionPhase::UnchangedCandidate]);
        drop(hooks);
        assert!(sql_snapshot(&case.db, true) == injected_sql.unwrap());
        assert!(filesystem(&case.fixture) == files);
        assert!(
            journal
                .read_native_admission_snapshot(
                    case.registered.source_id(),
                    Some("default"),
                    None,
                    &mut reads()
                )
                .is_err()
        );
        writer_reservation(&case.db, false);
    }
}

mod target;
