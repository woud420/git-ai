use super::*;
use std::os::unix::fs::PermissionsExt;

fn erase_native_rows(case: &Case) {
    case.sql().execute_batch("DELETE FROM jj_native_workspaces; DELETE FROM jj_native_registrations; DELETE FROM jj_native_sources; DELETE FROM jj_native_baselines;").unwrap();
    assert_eq!(counts(case), [0; 4]);
}

pub fn native_only(case: &Case, config: &Config) {
    let mut journal = case.open();
    let registered = installed(case.register(&mut journal, config).unwrap());
    let copied_seal = fs::read(case.seal()).unwrap();
    assert_eq!(copied_seal, seal_bytes(registered.source_id()));
    erase_native_rows(case);
    let captured = capture_current_state(&case.context(), deadline()).unwrap();
    let result = persist_current_state_baseline(
        &mut journal,
        registered.source_id(),
        0,
        &captured.prepare_baseline().unwrap(),
    )
    .unwrap();
    let BaselinePersistenceOutcome::Installed(receipt) = result else {
        panic!("namespace-only fixture did not install")
    };
    assert_eq!(&receipt, registered.baseline().receipt());
    assert_eq!(counts(case), [1, 1, 0, 0]);
    assert_eq!(fs::read(case.seal()).unwrap(), copied_seal);
    reject_unchanged(case, &mut journal, config);
}

pub fn row_gap(case: &Case, config: &Config) {
    let table = case.name.strip_prefix("row_gap:").unwrap();
    assert!(TABLES.contains(&table));
    let mut journal = case.open();
    installed(case.register(&mut journal, config).unwrap());
    let conn = case.sql();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    assert_eq!(
        conn.execute(&format!("DELETE FROM {table}"), []).unwrap(),
        1
    );
    drop(conn);
    reject_unchanged(case, &mut journal, config);
}

pub fn missing_seal(case: &Case, config: &Config) {
    let mut journal = case.open();
    installed(case.register(&mut journal, config).unwrap());
    // Remove the entire owned namespace so an empty-directory rule cannot mask the guard.
    fs::remove_file(case.seal()).unwrap();
    fs::remove_dir(case.namespace()).unwrap();
    assert!(!case.namespace().exists());
    assert_eq!(counts(case), [1; 4]);
    reject_unchanged(case, &mut journal, config);
}

pub fn invalid_seal(case: &Case, config: &Config) {
    let mut journal = case.open();
    let registered = installed(case.register(&mut journal, config).unwrap());
    let valid = seal_bytes(registered.source_id());
    let mut trailing = valid.clone();
    trailing.push(b'\n');
    let malformed = [
        Vec::new(),
        vec![0xff],
        trailing,
        valid[..valid.len() - 1].to_vec(),
        format!("git-ai/jj/source-seal/v1\nreader_profile={JJ_OBSERVATION_READER_PROFILE}\nsource_id={}\n", registered.source_id()).into_bytes(),
        valid.iter().copied().map(|b| if b == b'=' { b' ' } else { b }).collect(),
        format!("git-ai/jj/source-seal/v1\nsource_id={}\nreader_profile=unknown\n", registered.source_id()).into_bytes(),
        vec![b'x'; 1025],
    ];
    for bytes in malformed {
        fs::write(case.seal(), &bytes).unwrap();
        reject_unchanged(case, &mut journal, config);
    }
}

pub fn seal_only(case: &Case, config: &Config) {
    let mut journal = case.open();
    let registered = installed(case.register(&mut journal, config).unwrap());
    erase_native_rows(case);
    assert_eq!(
        fs::read(case.seal()).unwrap(),
        seal_bytes(registered.source_id())
    );
    reject_unchanged(case, &mut journal, config);
}

pub fn directory_only(case: &Case, config: &Config) {
    let mut journal = case.open();
    fs::create_dir(case.namespace()).unwrap();
    fs::set_permissions(case.namespace(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read_dir(case.namespace()).unwrap().count(), 0);
    reject_unchanged(case, &mut journal, config);
    assert_eq!(counts(case), [0; 4]);
}

pub fn sql_abort(case: &Case, config: &Config) {
    let table = case.name.strip_prefix("sql_abort:").unwrap();
    assert!(TABLES.contains(&table));
    let mut journal = case.open();
    let marker = "registration-test-injected-insert-failure";
    case.sql().execute_batch(&format!("CREATE TRIGGER registration_insert_failure BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT, '{marker}'); END;")).unwrap();
    let before = manifest(&case.repository);
    let failure = error(case.register(&mut journal, config));
    let mut chain = failure.to_string();
    let mut next = std::error::Error::source(&failure);
    while let Some(cause) = next {
        chain.push_str(&cause.to_string());
        next = cause.source();
    }
    assert!(
        chain.contains(marker),
        "fixture did not reach the intended insert: {chain}"
    );
    assert_eq!(counts(case), [0; 4]);
    assert!(case.seal().is_file());
    let mut expected = before;
    expected.insert(
        case.namespace()
            .strip_prefix(&case.repository)
            .unwrap()
            .to_owned(),
        Entry::Directory,
    );
    expected.insert(
        case.seal()
            .strip_prefix(&case.repository)
            .unwrap()
            .to_owned(),
        Entry::File(fs::read(case.seal()).unwrap()),
    );
    assert_eq!(manifest(&case.repository), expected);
    case.sql()
        .execute_batch("DROP TRIGGER registration_insert_failure")
        .unwrap();
    reject_unchanged(case, &mut journal, config);
}
