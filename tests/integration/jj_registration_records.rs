use super::*;
use ciborium::value::Value as Cbor;
use git_ai::model::repository::jj_observation_journal::ReadBudget;
use sha2::{Digest, Sha256};
use std::fs;

#[path = "jj_registration_records_budgets.rs"]
mod budgets;
#[path = "jj_registration_records_errors.rs"]
mod errors;
#[path = "jj_registration_records_support.rs"]
mod record_support;
#[path = "jj_registration_record_vectors.rs"]
mod vectors;
use record_support::*;
use vectors::{RECORDS, RecordVector};

#[test]
fn jj_registration_records_absence_does_not_adopt_existing_unregistered_state() {
    let fixture = frozen_v2();
    let mut journal = fixture.open();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = complete_snapshot(&conn);
    let mut budget = ReadBudget::new(0);
    for source in [&fixture.source, &source_id(999)] {
        assert!(
            journal
                .read_native_registration_record(source, &mut budget)
                .unwrap()
                .is_none()
        );
        assert!(
            journal
                .read_native_workspace_record(source, "default", &mut budget)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(budget.consumed(), 0);
    assert_existing_native_and_opaque(&fixture, &mut journal);
    assert_eq!(complete_snapshot(&conn), before);
}

#[test]
fn jj_registration_records_all_independent_canonical_vectors_roundtrip_exactly() {
    let (fixture, conn) = record_fixture();
    let journal = fixture.open();
    let valid: Vec<_> = RECORDS.iter().filter(|record| record.valid).collect();
    assert_eq!(valid.len(), 15);
    for record in valid {
        assert_eq!(digest(record.raw), record.checksum);
        reset_record(&conn, record);
        let mut budget = ReadBudget::new(record.raw.len());
        assert_eq!(
            checked_read(&journal, &conn, record, &mut budget).unwrap(),
            Some(record.raw.to_vec()),
            "{}",
            record.label
        );
        assert_eq!(budget.consumed(), record.raw.len());
        assert_eq!(budget.remaining(), 0);
    }
}

#[test]
fn jj_registration_records_all_independent_malformed_vectors_are_rejected() {
    let (fixture, conn) = record_fixture();
    let journal = fixture.open();
    let invalid: Vec<_> = RECORDS.iter().filter(|record| !record.valid).collect();
    assert_eq!(invalid.len(), 40);
    for record in invalid {
        assert_eq!(digest(record.raw), record.checksum);
        reset_record(&conn, record);
        let selected = if record.raw.len() <= RECORD_LIMIT {
            record.raw.len()
        } else {
            0
        };
        assert_charged_error(&journal, &conn, record, selected);
    }
}

#[test]
fn jj_registration_records_reopen_preserves_both_platforms_and_exact_raw_bytes() {
    for (source, workspace) in [
        ("linux_source", "linux_workspace"),
        ("macos_source", "macos_workspace"),
    ] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, vector(source));
        insert_record(&conn, vector(workspace));
        let before = complete_snapshot(&conn);
        for _ in 0..2 {
            let journal = fixture.open();
            for record in [vector(source), vector(workspace)] {
                let mut budget = ReadBudget::new(record.raw.len());
                assert_eq!(
                    checked_read(&journal, &conn, record, &mut budget).unwrap(),
                    Some(record.raw.to_vec())
                );
                assert_eq!(budget.consumed(), record.raw.len());
            }
        }
        assert_eq!(complete_snapshot(&conn), before);
    }
}

#[test]
fn jj_registration_records_individual_orphans_are_inspectable_without_native_join() {
    for record in [vector("linux_source"), vector("linux_workspace")] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, record);
        conn.execute("DELETE FROM jj_native_sources", []).unwrap();
        conn.execute("DELETE FROM jj_native_baselines", []).unwrap();
        let journal = fixture.open();
        let mut budget = ReadBudget::new(record.raw.len());
        assert_eq!(
            checked_read(&journal, &conn, record, &mut budget).unwrap(),
            Some(record.raw.to_vec())
        );
        fixture.assert_only_first(&journal);
        assert_native_tables_empty(&conn);
    }
}

#[test]
fn jj_registration_records_workspace_names_keep_case_and_unicode_bytes() {
    for explicit_binary in [false, true] {
        let workspaces = if explicit_binary {
            WORKSPACES
                .replace(
                    "workspace_name TEXT NOT NULL",
                    "workspace_name TEXT COLLATE NOCASE NOT NULL",
                )
                .replace(
                    "PRIMARY KEY (source_id, workspace_name)",
                    "PRIMARY KEY (source_id, workspace_name COLLATE BINARY)",
                )
        } else {
            WORKSPACES.to_owned()
        };
        let fixture = manual_v3(REGISTRATIONS, &workspaces);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let labels = [
            "linux_workspace",
            "case_workspace",
            "unicode_workspace",
            "nfc_workspace",
            "nfd_workspace",
        ];
        for label in labels {
            insert_record(&conn, vector(label));
        }
        let journal = fixture.open();
        for label in labels {
            let record = vector(label);
            let mut budget = ReadBudget::new(record.raw.len());
            assert_eq!(
                checked_read(&journal, &conn, record, &mut budget).unwrap(),
                Some(record.raw.to_vec()),
                "{label}, explicit Binary {explicit_binary}"
            );
        }
        let before = complete_snapshot(&conn);
        let mut budget = ReadBudget::new(0);
        assert!(
            journal
                .read_native_workspace_record(
                    vector("linux_workspace").source_id,
                    "DEFAULT",
                    &mut budget
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(budget.consumed(), 0);
        assert_eq!(complete_snapshot(&conn), before);
    }
}

#[test]
fn jj_registration_records_unrelated_sources_and_corrupt_native_payloads_are_not_read() {
    for (selected, unrelated) in [
        (vector("linux_source"), vector("other_source")),
        (vector("linux_workspace"), vector("other_workspace")),
    ] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, selected);
        insert_record(&conn, unrelated);
        conn.execute(
            &format!(
                "UPDATE {} SET record = X'00', checksum = 'invalid' WHERE source_id = ?1",
                table(unrelated)
            ),
            [unrelated.source_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE jj_native_baselines SET record = X'00', checksum = 'invalid'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE jj_native_sources SET state = X'00', checksum = 'invalid'",
            [],
        )
        .unwrap();
        let journal = fixture.open();
        let mut budget = ReadBudget::new(selected.raw.len());
        assert_eq!(
            checked_read(&journal, &conn, selected, &mut budget).unwrap(),
            Some(selected.raw.to_vec())
        );
        assert_eq!(budget.consumed(), selected.raw.len());
        assert_charged_error(&journal, &conn, unrelated, 1);
    }
}

#[test]
fn jj_registration_records_unrelated_workspace_sibling_is_not_decoded() {
    let selected = vector("linux_workspace");
    let sibling = vector("case_workspace");
    let (fixture, conn) = record_fixture();
    insert_record(&conn, selected);
    insert_record(&conn, sibling);
    conn.execute("UPDATE jj_native_workspaces SET record = X'00', checksum = 'invalid' WHERE workspace_name COLLATE BINARY = ?1", [sibling.workspace_name]).unwrap();
    let journal = fixture.open();
    let mut budget = ReadBudget::new(selected.raw.len());
    assert_eq!(
        checked_read(&journal, &conn, selected, &mut budget).unwrap(),
        Some(selected.raw.to_vec())
    );
    assert_charged_error(&journal, &conn, sibling, 1);
}

#[test]
fn jj_registration_record_fixtures_survive_git_crlf_checkout_byte_exactly() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let fixture_dir = repo.path().join("records");
    fs::create_dir(&fixture_dir).unwrap();
    let attributes = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jj-registration-records/.gitattributes");
    match fs::read(attributes) {
        Ok(raw) => fs::write(fixture_dir.join(".gitattributes"), raw).unwrap(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("cannot read fixture attributes: {error}"),
    }
    for record in RECORDS {
        fs::write(
            fixture_dir.join(format!("{}.cbor", record.label)),
            record.raw,
        )
        .unwrap();
    }
    let exact_name = b"workspace\nwith\r\nline breaks";
    fs::write(fixture_dir.join("checkout.name.txt"), exact_name).unwrap();
    repo.git(&["-c", "core.autocrlf=false", "add", "--", "records"])
        .unwrap();
    for record in RECORDS {
        fs::remove_file(fixture_dir.join(format!("{}.cbor", record.label))).unwrap();
    }
    fs::remove_file(fixture_dir.join("checkout.name.txt")).unwrap();
    repo.git(&[
        "-c",
        "core.autocrlf=true",
        "checkout-index",
        "--all",
        "--force",
    ])
    .unwrap();
    for record in RECORDS {
        let actual = fs::read(fixture_dir.join(format!("{}.cbor", record.label))).unwrap();
        assert_eq!(digest(&actual), record.checksum, "{}", record.label);
    }
    assert_eq!(
        fs::read(fixture_dir.join("checkout.name.txt")).unwrap(),
        exact_name
    );
}
