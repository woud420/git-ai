use super::*;

pub fn unregistered(case: &Case, config: &Config) {
    let mut journal = case.open();
    if case.name.ends_with("empty_heads") {
        installed(case.register(&mut journal, config).unwrap());
        case.heads(&[]);
    } else {
        capture_current_state(&case.context(), deadline()).unwrap();
    }
    rejected(case, &journal, config);
    if case.name.ends_with("unregistered") {
        assert_eq!(counts(case), [0; 4]);
        assert!(!case.namespace().exists());
    }
}

pub fn evidence(case: &Case, config: &Config) {
    let (journal, _) = install(case, config);
    if case.name.ends_with("missing_view") {
        let head = native::late_branch();
        write_records(case, &[first(), left(), head.clone()]);
        assert_ne!(head.view_id, MINIMAL_ID);
        fs::remove_file(view_path(case, MINIMAL_ID)).unwrap();
        assert!(op_path(case, LEFT_ID).is_file());
        case.heads(&[&head.operation_id]);
        capture_current_state(&case.context(), deadline()).unwrap();
    } else {
        let a = native::rich_parent();
        let b = native::rich_child();
        write_records(case, &[a.clone(), b.clone()]);
        case.heads(&[&b.operation_id]);
        if case.name.ends_with("missing_operation") {
            fs::remove_file(op_path(case, &a.operation_id)).unwrap();
        } else if case.name.ends_with("unsupported") {
            let mut raw = a.operation_bytes.clone();
            raw.extend_from_slice(&bytes_field(99, b"future field"));
            fs::write(op_path(case, &a.operation_id), raw).unwrap();
        } else {
            fs::write(op_path(case, &a.operation_id), [0]).unwrap();
        }
        capture_current_state(&case.context(), deadline()).unwrap();
    }
    rejected(case, &journal, config);
    rejected(case, &journal, config);
}

pub fn row(case: &Case, config: &Config) {
    let (journal, _) = install(case, config);
    let pieces: Vec<_> = case.name.split(':').collect();
    let kind = pieces[2];
    let table = pieces[3];
    assert!(TABLES.contains(&table));
    let conn = case.sql();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    let count = if kind == "gap" {
        conn.execute(&format!("DELETE FROM {table}"), []).unwrap()
    } else {
        conn.execute(
            &format!("UPDATE {table} SET checksum = ?1"),
            ["00".repeat(32)],
        )
        .unwrap()
    };
    assert_eq!(count, 1);
    drop(conn);
    capture_current_state(&case.context(), deadline()).unwrap();
    rejected(case, &journal, config);
}

pub fn binding(case: &Case, config: &Config) {
    let (journal, saved) = install(case, config);
    let mut context = case.context();
    match case.name.rsplit(':').next().unwrap() {
        "missing_seal" => {
            fs::remove_file(case.seal()).unwrap();
            fs::remove_dir(case.namespace()).unwrap();
        }
        "invalid_seal" => {
            let mut raw = seal_bytes(saved.source_id());
            raw.push(b'\n');
            fs::write(case.seal(), raw).unwrap();
        }
        "backend" => fs::write(case.repo_dir.join("op_store/type"), b"unsupported").unwrap(),
        "context" => context.capability = "native_verified",
        other => panic!("unknown binding case {other}"),
    }
    failure(checked(
        case,
        &journal,
        &context,
        config,
        deadline(),
        &mut budget(),
    ));
}

pub fn native_only(case: &Case, config: &Config) {
    let (mut journal, saved) = install(case, config);
    let conn = case.sql();
    conn.execute_batch("DELETE FROM jj_native_workspaces; DELETE FROM jj_native_registrations;")
        .unwrap();
    drop(conn);
    assert_eq!(counts(case), [1, 1, 0, 0]);
    assert_eq!(
        fs::read(case.seal()).unwrap(),
        seal_bytes(saved.source_id())
    );
    let captured = capture_current_state(&case.context(), deadline()).unwrap();
    assert!(matches!(
        persist_current_state_baseline(
            &mut journal,
            saved.source_id(),
            0,
            &captured.prepare_baseline().unwrap()
        )
        .unwrap(),
        BaselinePersistenceOutcome::AlreadyInstalled(_)
    ));
    rejected(case, &journal, config);
}

pub fn sql_budget(case: &Case, config: &Config) {
    let (journal, saved) = install(case, config);
    let mut probe = budget();
    checked(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut probe,
    )
    .unwrap();
    let required = probe.consumed();
    assert!(required > 0);
    let mut shared = ReadBudget::new(required);
    let result = checked(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut shared,
    )
    .unwrap();
    assert_result(&result, &saved, &[MERGE_ID], &[], &[MERGE_ID], false);
    assert_eq!(shared.consumed(), required);
    assert_eq!(shared.remaining(), 0);
    failure(checked(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut shared,
    ));
    assert_eq!(shared.consumed(), required);
    let mut short = ReadBudget::new(required - 1);
    failure(checked(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut short,
    ));
    assert!(short.consumed() > 0);
    assert!(short.consumed() < required);
}

pub fn expired(case: &Case, config: &Config) {
    let (journal, _) = install(case, config);
    let records = [native::rich_parent(), native::rich_child()];
    write_records(case, &records);
    case.heads(&[&records[1].operation_id]);
    let expired = Instant::now() - Duration::from_secs(1);
    let error = failure(checked(
        case,
        &journal,
        &case.context(),
        config,
        expired,
        &mut budget(),
    ));
    assert!(
        error.to_string().to_ascii_lowercase().contains("deadline"),
        "{error}"
    );
}
