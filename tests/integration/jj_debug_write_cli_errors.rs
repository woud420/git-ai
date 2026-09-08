use super::super::super::support::ADMISSION_TABLES;
use super::*;

pub fn early(case: &Case) {
    let kind = case.name.rsplit(':').next().unwrap();
    let missing = case.test_home.join("missing-write-parent");
    let path = missing.join("journal.sqlite");
    if kind == "empty" || kind == "malformed" {
        fs::write(case.root.join(".jj/working_copy/type"), b"unsupported").unwrap();
    } else if kind == "git" {
        fs::rename(case.root.join(".jj"), case.root.join("saved-jj-layout")).unwrap();
    }
    let code = match kind {
        "empty" => "collection_disabled",
        "git" => "not_jj_workspace",
        "malformed" => "context_unavailable",
        "paths" => "journal_unavailable",
        _ => unreachable!(),
    };
    if kind == "paths" {
        let uri = format!("file:{}?mode=rwc", path.display());
        for requested in [Path::new(":memory:"), Path::new(&uri)] {
            for args in [initialize_args(requested), fake_args(requested, 1)] {
                no_creation(case, command(case, &args, &case.root), code, &missing);
            }
        }
        let regular = case.test_home.join("private-regular-parent");
        fs::write(&regular, b"keep this file\n").unwrap();
        let requested = regular.join("journal.sqlite");
        for args in [initialize_args(&requested), fake_args(&requested, 1)] {
            let before = manifest(&case.repository);
            let value = crate::jj_debug_cli::error(
                command(case, &args, &case.root).output().unwrap(),
                "journal_unavailable",
            );
            assert!(!value.to_string().contains(case.test_home.to_str().unwrap()));
            assert!(!value.to_string().contains("private-regular-parent"));
            assert_eq!(fs::read(&regular).unwrap(), b"keep this file\n");
            assert!(!requested.exists());
            assert!(!case.namespace().exists());
            assert_eq!(manifest(&case.repository), before);
        }
    } else {
        for args in [initialize_args(&path), fake_args(&path, 1)] {
            no_creation(case, command(case, &args, &case.root), code, &missing);
        }
    }
}

pub fn created_empty(case: &Case) {
    let denied = case.name.ends_with(":denied");
    let variants = if denied {
        vec![("initialize", 1), ("capture", 1)]
    } else {
        vec![("capture", 1), ("capture", 32)]
    };
    for (index, (action, heads)) in variants.into_iter().enumerate() {
        let directory = case.test_home.join(format!("created-write-parent-{index}"));
        let path = directory.join("journal.sqlite");
        assert!(!directory.exists());
        let before = manifest(&case.repository);
        let args = if action == "initialize" {
            initialize_args(&path)
        } else {
            fake_args(&path, heads)
        };
        let code = if action == "initialize" {
            "registration_unavailable"
        } else {
            "admission_unavailable"
        };
        crate::jj_debug_cli::error(command(case, &args, &case.root).output().unwrap(), code);
        assert!(path.is_file());
        // Read-only opening is the control: the assertion cannot create or migrate the database.
        let journal = JjObservationJournal::open_read_only_at_path(&path).unwrap();
        assert_eq!(journal.status(&"0".repeat(64)).unwrap().generation, 0);
        let conn = git_ai::model::repository::sqlite::open_with_flags_and_memory_limits(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        for table in TABLES.into_iter().chain(ADMISSION_TABLES).chain([
            "jj_sources",
            "jj_operations",
            "jj_views",
            "jj_batches",
        ]) {
            let count: usize = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "failure populated {table}");
        }
        assert!(!case.namespace().exists());
        assert_eq!(manifest(&case.repository), before);
    }
}

pub fn incomplete(case: &Case, config: &Config) {
    let kind = case.name.rsplit(':').next().unwrap();
    // These helpers already calibrate valid native-only/seal-only fixtures and API refusal.
    match kind {
        "directory_only" => super::super::super::super::super::faults::directory_only(case, config),
        "native_only" => super::super::super::super::super::faults::native_only(case, config),
        "seal_only" => super::super::super::super::super::faults::seal_only(case, config),
        _ => unreachable!(),
    }
    checked(
        case,
        command(case, &initialize_args(&case.journal_path), &case.root),
        Some("registration_unavailable"),
    );
    checked(
        case,
        command(case, &fake_args(&case.journal_path, 1), &case.root),
        Some("admission_unavailable"),
    );
}

pub fn init_abort(case: &Case) {
    let _journal = case.open();
    case.sql().execute_batch("CREATE TRIGGER write_cli_init_abort BEFORE INSERT ON jj_native_workspaces BEGIN SELECT RAISE(ABORT,'write-cli-init-injected'); END;").unwrap();
    let mut before = manifest(&case.repository);
    let failure = crate::jj_debug_cli::error(
        command(case, &initialize_args(&case.journal_path), &case.root)
            .output()
            .unwrap(),
        "registration_unavailable",
    );
    assert!(
        failure["error"]["message"]
            .as_str()
            .unwrap()
            .contains("write-cli-init-injected")
    );
    assert_eq!(counts(case), [0; 4]);
    assert!(case.seal().is_file());
    before.insert(
        case.namespace()
            .strip_prefix(&case.repository)
            .unwrap()
            .to_owned(),
        Entry::Directory,
    );
    before.insert(
        case.seal()
            .strip_prefix(&case.repository)
            .unwrap()
            .to_owned(),
        Entry::File(fs::read(case.seal()).unwrap()),
    );
    assert_eq!(manifest(&case.repository), before);
    case.sql()
        .execute_batch("DROP TRIGGER write_cli_init_abort")
        .unwrap();
    checked(
        case,
        command(case, &initialize_args(&case.journal_path), &case.root),
        Some("registration_unavailable"),
    );
    assert_eq!(counts(case), [0; 4]);
}

pub fn capture_failure(case: &Case, config: &Config) {
    if case.name.ends_with(":native_packet") {
        super::super::super::storage::corrupt(case, config);
        let journal = JjObservationJournal::open_read_only_at_path(&case.journal_path).unwrap();
        let saved = case.reopen(&journal, config).unwrap().unwrap();
        let generation: u64 = case
            .sql()
            .query_row(
                "SELECT a.generation FROM jj_native_admissions a JOIN jj_native_admission_states s ON s.source_id=a.source_id AND s.admission_id=a.admission_id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(generation, 2);
        let heads = vec![native::rich_child().operation_id];
        assert_eq!(
            capture_current_state(&case.context(), deadline())
                .unwrap()
                .head_ids(),
            heads
        );
        let args = expect_args(
            &case.journal_path,
            NativeAdmissionExpectation {
                source_id: saved.source_id(),
                initialization_receipt_id: saved.initialization_receipt_id(),
                baseline_id: saved.baseline().receipt().baseline_id(),
                generation,
                admitted_head_ids: &heads,
            },
        );
        let failure = checked(
            case,
            command(case, &args, &case.root),
            Some("admission_unavailable"),
        );
        assert!(
            failure["error"]["message"]
                .as_str()
                .unwrap()
                .contains("operation")
        );
        return;
    }
    let (journal, _) = initialize_cli(case, config);
    let zero = status(case, &journal, config).cursor().clone();
    let a = native::rich_parent();
    select(
        case,
        std::slice::from_ref(&a),
        &[&a.operation_id],
        Some(&a.operation_id),
    );
    case.sql().execute_batch("CREATE TRIGGER write_cli_capture_abort BEFORE INSERT ON jj_native_admission_states BEGIN SELECT RAISE(ABORT,'write-cli-capture-injected'); END;").unwrap();
    let failure = checked(
        case,
        command(
            case,
            &expect_args(&case.journal_path, zero.expectation()),
            &case.root,
        ),
        Some("admission_unavailable"),
    );
    assert!(
        failure["error"]["message"]
            .as_str()
            .unwrap()
            .contains("write-cli-capture-injected")
    );
    for (_, rows) in admission_rows(case) {
        assert!(rows.is_empty());
    }
}
