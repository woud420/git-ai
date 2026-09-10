use super::*;

#[test]
fn registration_snapshot_joins_frozen_records_without_charging_original_twice() {
    let fixture = Fixture::complete();
    let mut budget = ReadBudget::new(LIMIT);
    let result = read(&fixture, "default", &mut budget).unwrap().unwrap();
    assert_eq!(result.registration.raw, SOURCE_RAW);
    assert_eq!(result.registration.checksum, digest(SOURCE_RAW));
    assert_eq!(result.original_workspace.raw, WORKSPACE_RAW);
    assert!(std::ptr::eq(
        result.selected_workspace(),
        &result.original_workspace
    ));
    assert_eq!(result.native.state, fixture.seed.state);
    assert_eq!(result.native.record.anchors, fixture.seed.record.anchors);
    assert_eq!(
        budget.consumed(),
        charged_lengths(&fixture.journal.conn).iter().sum::<usize>()
    );
}

#[test]
fn registration_snapshot_reads_only_original_and_exact_selected_workspace() {
    let fixture = Fixture::complete();
    let mut linked = workspace_record();
    linked.workspace_name = "linked".into();
    linked.attachment_id = "09".repeat(32);
    linked
        .locator
        .workspace_root
        .0
        .extend_from_slice(b"/linked");
    linked.workspace_binding.directories[0].inode += 100;
    let linked_raw = encode(&linked);
    insert_workspace(&fixture.journal.conn, &linked_raw);
    // Unselected attachment corruption is deliberately outside the bounded join.
    let mut unused = workspace_record();
    unused.workspace_name = "unselected".into();
    unused
        .locator
        .workspace_root
        .0
        .extend_from_slice(b"/unused");
    unused.workspace_binding.directories[0].inode += 200;
    insert_workspace(&fixture.journal.conn, &encode(&unused));
    fixture
        .journal
        .conn
        .execute(
            "UPDATE jj_native_workspaces SET record=x'ff' WHERE workspace_name='unselected'",
            [],
        )
        .unwrap();
    let mut budget = ReadBudget::new(LIMIT);
    let result = read(&fixture, "linked", &mut budget).unwrap().unwrap();
    assert_eq!(result.original_workspace.raw, WORKSPACE_RAW);
    assert_eq!(result.selected_workspace().raw, linked_raw);
    assert!(!std::ptr::eq(
        result.selected_workspace(),
        &result.original_workspace
    ));
    let native = charged_lengths(&fixture.journal.conn);
    assert_eq!(
        budget.consumed(),
        SOURCE_RAW.len() + WORKSPACE_RAW.len() + linked_raw.len() + native[2] + native[3]
    );
}

#[test]
fn registration_snapshot_absence_is_distinct_from_every_orphan_family() {
    let fixture = Fixture::new(false);
    let mut budget = ReadBudget::new(0);
    assert!(read(&fixture, "default", &mut budget).unwrap().is_none());
    assert_eq!(budget.consumed(), 0);
    for orphan in ["baseline", "state", "workspace"] {
        let fixture = Fixture::new(true);
        fixture
            .journal
            .conn
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        match orphan {
            "baseline" => {
                fixture
                    .journal
                    .conn
                    .execute("DELETE FROM jj_native_sources", [])
                    .unwrap();
            }
            "state" => {
                fixture
                    .journal
                    .conn
                    .execute("DELETE FROM jj_native_baselines", [])
                    .unwrap();
            }
            "workspace" => {
                fixture
                    .journal
                    .conn
                    .execute_batch(
                        "DELETE FROM jj_native_sources; DELETE FROM jj_native_baselines;",
                    )
                    .unwrap();
                insert_workspace(&fixture.journal.conn, WORKSPACE_RAW);
            }
            _ => unreachable!(),
        }
        let mut budget = ReadBudget::new(0);
        assert!(
            !error_text(read(&fixture, "default", &mut budget)).is_empty(),
            "{orphan}"
        );
        assert_eq!(
            budget.consumed(),
            0,
            "orphan probes must not select payloads"
        );
    }
}

#[test]
fn registration_snapshot_requires_original_selected_and_native_rows() {
    for table in [
        "jj_native_workspaces",
        "jj_native_sources",
        "jj_native_baselines",
    ] {
        let fixture = Fixture::complete();
        fixture
            .journal
            .conn
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        fixture
            .journal
            .conn
            .execute(&format!("DELETE FROM {table}"), [])
            .unwrap();
        assert!(
            !error_text(read(&fixture, "default", &mut ReadBudget::new(LIMIT))).is_empty(),
            "{table}"
        );
    }
    let fixture = Fixture::complete();
    assert!(!error_text(read(&fixture, "unknown", &mut ReadBudget::new(LIMIT))).is_empty());
}

#[test]
fn registration_snapshot_joins_the_different_selected_workspace_to_the_saved_epoch() {
    let fixture = Fixture::complete();
    let mut linked = workspace_record();
    linked.workspace_name = "linked".into();
    linked.attachment_id = "09".repeat(32);
    linked
        .locator
        .workspace_root
        .0
        .extend_from_slice(b"/linked");
    linked.workspace_binding.directories[0].inode += 100;
    linked.baseline_id = "0a".repeat(32);
    insert_workspace(&fixture.journal.conn, &encode(&linked));
    // Original and native rows still form a complete valid source.
    assert!(
        read(&fixture, "default", &mut ReadBudget::new(LIMIT))
            .unwrap()
            .is_some()
    );
    assert!(!error_text(read(&fixture, "linked", &mut ReadBudget::new(LIMIT))).is_empty());
    let mut budget = ReadBudget::new(0);
    assert!(
        fixture
            .journal
            .read_registration_snapshot(&"0b".repeat(32), "default", &mut budget)
            .unwrap()
            .is_none()
    );
    assert_eq!(budget.consumed(), 0);
}

#[test]
fn registration_snapshot_rejects_repaired_cross_record_identity_mismatches() {
    for field in [
        "source_id",
        "reader_profile",
        "baseline_id",
        "baseline_generation",
        "seal_digest",
        "platform",
        "attachment",
        "workspace_digest",
    ] {
        let fixture = Fixture::complete();
        let mut source = source_record();
        let mut workspace = workspace_record();
        match field {
            "source_id" => workspace.source_id = "02".repeat(32),
            "reader_profile" => workspace.reader_profile = "unsupported".into(),
            "baseline_id" => workspace.baseline_id = "03".repeat(32),
            "baseline_generation" => workspace.baseline_generation = 2,
            "seal_digest" => workspace.seal_digest = "04".repeat(32),
            "platform" => workspace.locator.platform = Platform::Macos,
            "attachment" => source.initial_attachment_id = "05".repeat(32),
            "workspace_digest" => source.initial_workspace_record_id = "06".repeat(32),
            _ => unreachable!(),
        }
        let raw = encode(&workspace);
        if field != "workspace_digest" {
            source.initial_workspace_record_id = digest(&raw);
        }
        replace_record(&fixture.journal.conn, "jj_native_workspaces", &raw);
        fixture
            .journal
            .conn
            .execute(
                "UPDATE jj_native_workspaces SET locator_key=?1, workspace_root_key=?2",
                params![
                    workspace_locator_guard(&workspace.locator).unwrap(),
                    workspace_root_guard(&workspace.locator, &workspace.workspace_binding).unwrap()
                ],
            )
            .unwrap();
        replace_record(
            &fixture.journal.conn,
            "jj_native_registrations",
            &encode(&source),
        );
        assert!(
            !error_text(read(&fixture, "default", &mut ReadBudget::new(LIMIT))).is_empty(),
            "{field}"
        );
    }
}

#[test]
fn registration_snapshot_native_digest_join_is_not_replaced_by_matching_registration_rows() {
    let fixture = Fixture::complete();
    let mut source = source_record();
    let mut workspace = workspace_record();
    source.baseline_id = "07".repeat(32);
    workspace.baseline_id = source.baseline_id.clone();
    let workspace_raw = encode(&workspace);
    source.initial_workspace_record_id = digest(&workspace_raw);
    fixture
        .journal
        .conn
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    replace_record(
        &fixture.journal.conn,
        "jj_native_workspaces",
        &workspace_raw,
    );
    replace_record(
        &fixture.journal.conn,
        "jj_native_registrations",
        &encode(&source),
    );
    fixture
        .journal
        .conn
        .execute(
            "UPDATE jj_native_registrations SET baseline_id=?1",
            [&source.baseline_id],
        )
        .unwrap();
    assert!(!error_text(read(&fixture, "default", &mut ReadBudget::new(LIMIT))).is_empty());
}

#[test]
fn registration_snapshot_rejects_duplicate_selected_keys_after_constraints_are_removed() {
    for table in [
        "jj_native_registrations",
        "jj_native_workspaces",
        "jj_native_sources",
        "jj_native_baselines",
    ] {
        let fixture = Fixture::complete();
        fixture
            .journal
            .conn
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        fixture.journal.conn.execute_batch(&format!(
            "CREATE TABLE duplicate_copy AS SELECT * FROM {table}; DROP TABLE {table};
             ALTER TABLE duplicate_copy RENAME TO {table}; INSERT INTO {table} SELECT * FROM {table};"
        )).unwrap();
        assert!(
            !error_text(read(&fixture, "default", &mut ReadBudget::new(LIMIT))).is_empty(),
            "{table}"
        );
    }
}

#[test]
fn registration_guards_are_exact_index_keys_and_never_positive_identity() {
    let fixture = Fixture::complete();
    let source = source_record();
    let workspace = workspace_record();
    let source_key = source_root_guard(&source.source_binding).unwrap();
    let locator_key = workspace_locator_guard(&workspace.locator).unwrap();
    assert_eq!(
        source_key,
        "3ed777b2ecea6a9650be8d7a790810761d020bbfe2821506347ded2c198237a9"
    );
    assert_eq!(
        locator_key,
        "7d22978a6db2cd47a59de302b053fa6fc306e6b075200fde9c624461d535786c"
    );
    assert_eq!(
        workspace_root_guard(&workspace.locator, &workspace.workspace_binding).unwrap(),
        "764dd6c241fada8f0616013c73a9c2daade9a495eaac8b0e67ab1055c16ccfdd"
    );
    let absent = "ff".repeat(32);
    assert!(
        fixture
            .journal
            .registration_guards_occupied(&source_key, &absent)
            .unwrap()
    );
    assert!(
        fixture
            .journal
            .registration_guards_occupied(&absent, &locator_key)
            .unwrap()
    );
    assert!(
        !fixture
            .journal
            .registration_guards_occupied(&absent, &absent)
            .unwrap()
    );
    fixture
        .journal
        .conn
        .execute(
            "UPDATE jj_native_registrations SET record=zeroblob(10000000)",
            [],
        )
        .unwrap();
    assert!(
        fixture
            .journal
            .registration_guards_occupied(&source_key, &absent)
            .unwrap()
    );
    assert!(!error_text(fixture.journal.registration_guards_occupied("bad", &absent)).is_empty());
}
