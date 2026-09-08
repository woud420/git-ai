use super::*;

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    let Value::Map(fields) = value else {
        panic!("expected map")
    };
    &fields
        .iter()
        .find(|(key, _)| key.as_text() == Some(name))
        .unwrap()
        .1
}

fn pair(key: &str, value: Value) -> (Value, Value) {
    (Value::Text(key.to_owned()), value)
}

fn guards(record: &Value) -> (String, String) {
    let locator = field(record, "locator");
    let platform = field(locator, "platform").clone();
    let locator_guard = Value::Map(vec![
        pair(
            "domain",
            Value::Text("git-ai/jj/workspace-locator/v1".to_owned()),
        ),
        pair("platform", platform.clone()),
        pair("workspace_root", field(locator, "workspace_root").clone()),
    ]);
    let Value::Array(identities) = field(field(record, "workspace_binding"), "directories") else {
        panic!("expected directories")
    };
    let root_guard = Value::Map(vec![
        pair(
            "domain",
            Value::Text("git-ai/jj/workspace-root-guard/v1".to_owned()),
        ),
        pair("platform", platform),
        pair("device", field(&identities[0], "device").clone()),
        pair("inode", field(&identities[0], "inode").clone()),
    ]);
    (hash(&encoded(&locator_guard)), hash(&encoded(&root_guard)))
}

pub(super) fn different_id(original: &str) -> String {
    if original == "d".repeat(64) { "e" } else { "d" }.repeat(64)
}

pub(super) fn select_nonoriginal(
    case: &Case,
    journal: &JjObservationJournal,
    config: &Config,
) -> RegisteredNativeAdmissionState {
    let conn = case.sql();
    let source_raw: Vec<u8> = conn
        .query_row("SELECT record FROM jj_native_registrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    let workspace_raw: Vec<u8> = conn
        .query_row(
            "SELECT record FROM jj_native_workspaces WHERE workspace_name='default'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut source = decoded(&source_raw);
    let selected = decoded(&workspace_raw);
    let mut original = selected.clone();
    let name = "workspace-工";
    *field_mut(&mut original, "workspace_name") = Value::Text(name.to_owned());
    let Value::Bytes(root) = field_mut(field_mut(&mut original, "locator"), "workspace_root")
    else {
        panic!("expected locator bytes")
    };
    root.extend_from_slice(b"/historical-original");
    let Value::Array(identities) =
        field_mut(field_mut(&mut original, "workspace_binding"), "directories")
    else {
        panic!("expected directories")
    };
    let inode = field_mut(&mut identities[0], "inode");
    let number = u64::try_from(inode.as_integer().unwrap()).unwrap();
    *inode = Value::Integer((number ^ 1).into());
    let selected_checkout = field_mut(&mut original, "selected_checkout");
    assert_eq!(
        field(selected_checkout, "operation_id").as_text(),
        Some(MERGE_ID)
    );
    *field_mut(selected_checkout, "raw_checkout_bytes") =
        Value::Bytes(crate::jj_capture::support::checkout_bytes(MERGE_ID, name));
    let original_raw = encoded(&original);
    *field_mut(&mut source, "initial_workspace_name") = Value::Text(name.to_owned());
    *field_mut(&mut source, "initial_workspace_record_id") = Value::Text(hash(&original_raw));
    let new_source = encoded(&source);
    let (original_locator, original_root) = guards(&original);
    let (selected_locator, selected_root) = guards(&selected);
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(conn.execute("UPDATE jj_native_workspaces SET workspace_name=?1,locator_key=?2,workspace_root_key=?3,record=?4,checksum=?5", rusqlite::params![name,original_locator,original_root,original_raw,hash(&original_raw)]).unwrap(), 1);
    assert_eq!(conn.execute("INSERT INTO jj_native_workspaces(source_id,workspace_name,locator_key,workspace_root_key,record,checksum) VALUES (?1,'default',?2,?3,?4,?5)", rusqlite::params![field(&selected,"source_id").as_text().unwrap(),selected_locator,selected_root,workspace_raw,hash(&workspace_raw)]).unwrap(), 1);
    assert_eq!(
        conn.execute(
            "UPDATE jj_native_registrations SET record=?1,checksum=?2",
            rusqlite::params![new_source, hash(&new_source)]
        )
        .unwrap(),
        1
    );
    conn.execute_batch("COMMIT").unwrap();
    drop(conn);
    let selected = status(case, journal, config);
    assert_eq!(selected.registration().workspace_name(), "default");
    assert_eq!(selected.cursor().generation(), 0);
    assert_ne!(
        selected.registration().initialization_receipt_id(),
        hash(&source_raw)
    );
    selected
}

pub(super) fn replace_attachment(case: &Case, old: &str) {
    let conn = case.sql();
    let raw: Vec<u8> = conn
        .query_row(
            "SELECT record FROM jj_native_workspaces WHERE workspace_name='default'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut selected = decoded(&raw);
    assert_eq!(field(&selected, "attachment_id").as_text(), Some(old));
    *field_mut(&mut selected, "attachment_id") = Value::Text(different_id(old));
    let raw = encoded(&selected);
    assert_eq!(
        conn.execute(
            "UPDATE jj_native_workspaces SET record=?1,checksum=?2 WHERE workspace_name='default'",
            rusqlite::params![raw, hash(&raw)]
        )
        .unwrap(),
        1
    );
}
