use super::*;

#[test]
fn staged_registration_leaves_commit_and_rollback_to_its_caller() {
    let mut fixture = Fixture::new(false);
    let observer = fixture.independent();
    let original = snapshot(&observer);
    for commit in [false, true] {
        let prepared = prepared(&fixture.seed);
        let mut budget = ReadBudget::new(LIMIT);
        let staged = fixture
            .journal
            .stage_registration_install(prepared, &mut budget)
            .unwrap();
        assert_eq!(staged.snapshot().registration.raw, SOURCE_RAW);
        assert_eq!(staged.snapshot().original_workspace.raw, WORKSPACE_RAW);
        assert_eq!(staged.snapshot().native.state, fixture.seed.state);
        assert_eq!(native_counts(&observer), [0; 4]);
        assert_eq!(snapshot(&observer), original);
        assert!(budget.consumed() > SOURCE_RAW.len() + WORKSPACE_RAW.len());
        if commit {
            let saved = staged.commit().unwrap();
            assert_eq!(saved.registration.checksum, digest(SOURCE_RAW));
            assert_eq!(saved.native.record.anchors, fixture.seed.record.anchors);
        } else {
            drop(staged);
            assert_eq!(snapshot(&observer), original);
        }
        assert_eq!(
            native_counts(&observer),
            if commit { [1; 4] } else { [0; 4] }
        );
    }
    let reopened = JjObservationJournal::open_at_path(&fixture.path).unwrap();
    let saved = reopened
        .read_registration_snapshot(SOURCE, "default", &mut ReadBudget::new(LIMIT))
        .unwrap()
        .unwrap();
    assert_eq!(saved.registration.raw, SOURCE_RAW);
    assert_eq!(
        snapshot(&observer)[..4],
        original[..4],
        "opaque journal must remain exact"
    );
}

#[test]
fn registration_install_refuses_identical_standalone_or_registered_sources() {
    for registered in [false, true] {
        let mut fixture = if registered {
            Fixture::complete()
        } else {
            Fixture::new(true)
        };
        let before = snapshot(&fixture.journal.conn);
        let prepared = prepared(&fixture.seed);
        let mut budget = ReadBudget::new(0);
        assert!(
            !error_text(
                fixture
                    .journal
                    .stage_registration_install(prepared, &mut budget)
            )
            .is_empty()
        );
        assert_eq!(snapshot(&fixture.journal.conn), before);
        assert_eq!(
            budget.consumed(),
            0,
            "absence refusal must not select payloads"
        );
    }
}

#[test]
fn registration_install_checks_source_and_locator_negative_guards_in_transaction() {
    for source_collision in [true, false] {
        let mut fixture = Fixture::complete();
        let before = snapshot(&fixture.journal.conn);
        let source = "02".repeat(32);
        let mut details = metadata();
        if source_collision {
            details
                .workspace
                .locator
                .workspace_root
                .0
                .extend_from_slice(b"/new");
            details.workspace.workspace_binding.directories[0].inode += 100;
        } else {
            details.source_binding.directories[0].inode += 100;
        }
        let anchors: Vec<_> = fixture.seed.record.anchors.iter().collect();
        let prepared = PreparedRegistrationInstall::new(
            &source,
            &fixture.seed.record.reader_profile,
            &fixture.seed.record.captured_head_ids,
            &anchors,
            details,
        )
        .unwrap();
        assert!(
            !error_text(
                fixture
                    .journal
                    .stage_registration_install(prepared, &mut ReadBudget::new(0))
            )
            .is_empty()
        );
        assert_eq!(snapshot(&fixture.journal.conn), before);
    }
}

#[test]
fn registration_install_requires_all_four_affected_row_counts() {
    for table in [
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
    ] {
        let mut fixture = Fixture::new(false);
        fixture.journal.conn.execute_batch(&format!(
            "CREATE TRIGGER suppress_insert BEFORE INSERT ON {table} BEGIN SELECT RAISE(IGNORE); END;"
        )).unwrap();
        let before = snapshot(&fixture.journal.conn);
        let prepared = prepared(&fixture.seed);
        assert!(
            !error_text(
                fixture
                    .journal
                    .stage_registration_install(prepared, &mut ReadBudget::new(LIMIT))
            )
            .is_empty(),
            "{table}"
        );
        assert_eq!(native_counts(&fixture.journal.conn), [0; 4]);
        assert_eq!(snapshot(&fixture.journal.conn), before);
    }
}

#[test]
fn registration_install_rolls_back_an_after_insert_fail_with_all_four_rows_present() {
    let mut fixture = Fixture::new(false);
    fixture
        .journal
        .conn
        .execute_batch(
            "CREATE TRIGGER fail_after_four AFTER INSERT ON jj_native_workspaces BEGIN
         SELECT CASE WHEN (SELECT count(*) FROM jj_native_baselines)=1
           AND (SELECT count(*) FROM jj_native_sources)=1
           AND (SELECT count(*) FROM jj_native_registrations)=1
           AND (SELECT count(*) FROM jj_native_workspaces)=1
         THEN RAISE(FAIL, 'injected failure after four rows')
         ELSE RAISE(FAIL, 'injected failure before four rows') END; END;",
        )
        .unwrap();
    let before = snapshot(&fixture.journal.conn);
    let prepared = prepared(&fixture.seed);
    let message = error_text(
        fixture
            .journal
            .stage_registration_install(prepared, &mut ReadBudget::new(LIMIT)),
    );
    assert!(
        message.contains("injected failure after four rows"),
        "{message}"
    );
    assert_eq!(snapshot(&fixture.journal.conn), before);
}

#[test]
fn registration_install_readback_rejects_deleted_or_rewritten_rows() {
    for mutation in [
        "DELETE FROM jj_native_workspaces;",
        "UPDATE jj_native_sources SET state=x'ff';",
        "UPDATE jj_native_registrations SET record=x'ff';",
        "UPDATE jj_native_baselines SET record=x'ff';",
    ] {
        let mut fixture = Fixture::new(false);
        fixture.journal.conn.execute_batch(&format!(
            "CREATE TRIGGER change_after_four AFTER INSERT ON jj_native_workspaces BEGIN {mutation} END;"
        )).unwrap();
        let before = snapshot(&fixture.journal.conn);
        let prepared = prepared(&fixture.seed);
        assert!(
            !error_text(
                fixture
                    .journal
                    .stage_registration_install(prepared, &mut ReadBudget::new(LIMIT))
            )
            .is_empty(),
            "{mutation}"
        );
        assert_eq!(snapshot(&fixture.journal.conn), before);
    }
}

#[test]
fn registration_install_readback_rejects_a_coherently_rewritten_complete_receipt() {
    let mut source = source_record();
    let mut workspace = workspace_record();
    workspace.attachment_id = "08".repeat(32);
    let workspace_raw = encode(&workspace);
    source.initial_attachment_id = workspace.attachment_id.clone();
    source.initial_workspace_record_id = digest(&workspace_raw);
    let source_raw = encode(&source);
    // Calibrate that this is a complete structural join; only request equality rejects it.
    let control = Fixture::complete();
    replace_record(
        &control.journal.conn,
        "jj_native_workspaces",
        &workspace_raw,
    );
    replace_record(
        &control.journal.conn,
        "jj_native_registrations",
        &source_raw,
    );
    assert!(
        read(&control, "default", &mut ReadBudget::new(LIMIT))
            .unwrap()
            .is_some()
    );

    let mut fixture = Fixture::new(false);
    fixture
        .journal
        .conn
        .execute_batch(
            "CREATE TABLE replacement_source(record BLOB, checksum TEXT);
        CREATE TABLE replacement_workspace(record BLOB, checksum TEXT);",
        )
        .unwrap();
    fixture
        .journal
        .conn
        .execute(
            "INSERT INTO replacement_source VALUES (?1, ?2)",
            params![source_raw, digest(&source_raw)],
        )
        .unwrap();
    fixture
        .journal
        .conn
        .execute(
            "INSERT INTO replacement_workspace VALUES (?1, ?2)",
            params![workspace_raw, digest(&workspace_raw)],
        )
        .unwrap();
    fixture.journal.conn.execute_batch(
        "CREATE TRIGGER rewrite_receipt AFTER INSERT ON jj_native_workspaces BEGIN
         UPDATE jj_native_registrations SET record=(SELECT record FROM replacement_source), checksum=(SELECT checksum FROM replacement_source);
         UPDATE jj_native_workspaces SET record=(SELECT record FROM replacement_workspace), checksum=(SELECT checksum FROM replacement_workspace);
         END;"
    ).unwrap();
    let before = snapshot(&fixture.journal.conn);
    let prepared = prepared(&fixture.seed);
    assert!(
        !error_text(
            fixture
                .journal
                .stage_registration_install(prepared, &mut ReadBudget::new(LIMIT))
        )
        .is_empty()
    );
    assert_eq!(snapshot(&fixture.journal.conn), before);
}

#[test]
fn registration_preparation_rejects_structural_metadata_before_database_access() {
    let fixture = Fixture::new(false);
    let anchors: Vec<_> = fixture.seed.record.anchors.iter().collect();
    for invalid in ["seal", "backend", "name", "locator", "checkout", "platform"] {
        let mut details = metadata();
        match invalid {
            "seal" => details.seal_bytes = vec![0; 1025],
            "backend" => details.source_binding.backends[0].0 = vec![1; 16385],
            "name" => details.workspace.workspace_name.clear(),
            "locator" => details.workspace.locator.workspace_root.0 = b"relative".to_vec(),
            "checkout" => details
                .workspace
                .selected_checkout
                .raw_checkout_bytes
                .0
                .clear(),
            "platform" => details.workspace.locator.platform = Platform::Macos,
            _ => unreachable!(),
        }
        assert!(
            !error_text(PreparedRegistrationInstall::new(
                SOURCE,
                &fixture.seed.record.reader_profile,
                &fixture.seed.record.captured_head_ids,
                &anchors,
                details
            ))
            .is_empty(),
            "{invalid}"
        );
        assert_eq!(native_counts(&fixture.journal.conn), [0; 4]);
    }
}

#[test]
fn registration_first_install_cannot_acknowledge_an_unrequested_extra_workspace() {
    let mut extra = workspace_record();
    extra.workspace_name = "unrequested".into();
    extra.attachment_id = "0c".repeat(32);
    extra
        .locator
        .workspace_root
        .0
        .extend_from_slice(b"/unrequested");
    extra.workspace_binding.directories[0].inode += 300;
    let raw = encode(&extra);
    let locator = workspace_locator_guard(&extra.locator).unwrap();
    let root = workspace_root_guard(&extra.locator, &extra.workspace_binding).unwrap();

    // Ordinary selected reads still permit other structurally valid attachments.
    let control = Fixture::complete();
    insert_workspace(&control.journal.conn, &raw);
    assert!(
        read(&control, "default", &mut ReadBudget::new(LIMIT))
            .unwrap()
            .is_some()
    );
    assert!(
        read(&control, "unrequested", &mut ReadBudget::new(LIMIT))
            .unwrap()
            .is_some()
    );

    let mut fixture = Fixture::new(false);
    fixture
        .journal
        .conn
        .execute_batch(
            "CREATE TABLE extra_workspace(source_id TEXT, workspace_name TEXT, locator_key TEXT,
         workspace_root_key TEXT, record BLOB, checksum TEXT);",
        )
        .unwrap();
    fixture
        .journal
        .conn
        .execute(
            "INSERT INTO extra_workspace VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                SOURCE,
                extra.workspace_name,
                locator,
                root,
                raw,
                digest(&raw)
            ],
        )
        .unwrap();
    fixture
        .journal
        .conn
        .execute_batch(
            "CREATE TRIGGER add_unrequested_workspace AFTER INSERT ON jj_native_workspaces
         WHEN NEW.workspace_name='default' BEGIN
         INSERT INTO jj_native_workspaces SELECT * FROM extra_workspace;
         END;",
        )
        .unwrap();
    let before = snapshot(&fixture.journal.conn);
    let prepared = prepared(&fixture.seed);
    assert!(
        !error_text(
            fixture
                .journal
                .stage_registration_install(prepared, &mut ReadBudget::new(LIMIT))
        )
        .is_empty()
    );
    assert_eq!(snapshot(&fixture.journal.conn), before);
    assert_eq!(native_counts(&fixture.journal.conn), [0; 4]);
}
