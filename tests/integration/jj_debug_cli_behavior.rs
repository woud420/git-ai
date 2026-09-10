use super::*;

pub fn status_case(case: &Case, config: &Config) {
    let (mut journal, _, zero) = initial(case, config);
    fs::write(case.root.join("cli-dirty.txt"), b"leave uncommitted\n").unwrap();
    let before = status(case, &journal, config);
    assert_eq!(before.cursor().generation(), 0);
    assert!(before.latest_receipt().is_none());
    let args = status_args(&case.journal_path);
    let output = checked(case, command(case, &args, &case.root), None);
    assert_eq!(output, status_json(&before));
    let spaced = case.test_home.join("journal path with spaces");
    fs::create_dir(&spaced).unwrap();
    let spaced_path = spaced.join("../registration.sqlite");
    assert_eq!(
        checked(
            case,
            command(case, &status_args(&spaced_path), &case.root),
            None
        ),
        output
    );
    let nested = case.root.join("CLI directory with spaces/subdir");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(checked(case, command(case, &args, &nested), None), output);
    let record = native::rich_parent();
    select(
        case,
        std::slice::from_ref(&record),
        &[&record.operation_id],
        Some(&record.operation_id),
    );
    let admitted = outcome(admit_now(case, &mut journal, config, &zero), false);
    let observed = status(case, &journal, config);
    assert_eq!(observed.cursor(), admitted.current_cursor());
    assert_eq!(
        observed.registration().checkout_relation(),
        JjRegisteredCheckoutRelation::OutsideBaseline
    );
    let args = vec![
        "status".into(),
        "--journal".into(),
        case.journal_path.as_os_str().to_owned(),
        "--json".into(),
    ];
    assert_eq!(
        checked(case, command(case, &args, &case.root), None),
        status_json(&observed)
    );
}

pub fn receipts(case: &Case, config: &Config, detached: bool) {
    let (journal, saved, first, second) = two(case, config);
    if detached {
        fs::rename(case.root.join(".jj"), case.root.join("retained-jj-fixture")).unwrap();
        fs::write(case.test_home.join(".gitconfig"), b"[invalid-policy\n").unwrap();
    }
    for expected in [first.admission(), second.admission()] {
        let args = receipt_args(
            &case.journal_path,
            saved.source_id(),
            expected.receipt().admission_id(),
        );
        let mut invocation = command(case, &args, &case.test_home);
        if detached {
            invocation.env("GIT_AI_TEST_CONFIG_PATCH", r#"{"allowed_repositories":[]}"#);
        }
        assert_eq!(
            checked(case, invocation, None),
            receipt_json(Some(expected))
        );
    }
    let args = receipt_args(&case.journal_path, saved.source_id(), &"0".repeat(64));
    assert!(
        read_native_admission(
            &journal,
            saved.source_id(),
            &"0".repeat(64),
            deadline(),
            &mut admission_budget()
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(
        checked(case, command(case, &args, &case.test_home), None),
        receipt_json(None)
    );
    let args = receipt_args(
        &case.journal_path,
        &"0".repeat(64),
        first.admission().receipt().admission_id(),
    );
    checked(
        case,
        command(case, &args, &case.test_home),
        Some("admission_unavailable"),
    );
}

pub fn real(case: &Case, config: &Config) {
    let (mut journal, saved) = install(case, config);
    let before = read_registered_admission_state(
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap();
    assert_eq!(
        before.cursor().admitted_head_ids(),
        saved.baseline().receipt().captured_head_ids()
    );
    let admitted = outcome(
        admit_registered_history(
            &mut journal,
            &case.context(),
            config,
            before.cursor().expectation(),
            deadline(),
            &mut admission_budget(),
        )
        .unwrap(),
        false,
    );
    assert!(admitted.admission().ordered_operations().is_empty());
    let now = read_registered_admission_state(
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap();
    assert_eq!(
        checked(
            case,
            command(case, &status_args(&case.journal_path), &case.root),
            None
        ),
        status_json(&now)
    );
    let args = receipt_args(
        &case.journal_path,
        saved.source_id(),
        admitted.admission().receipt().admission_id(),
    );
    assert_eq!(
        checked(case, command(case, &args, &case.root), None),
        receipt_json(Some(admitted.admission()))
    );
    assert_eq!(
        fs::read(case.root.join("dirty-cli.txt")).unwrap(),
        b"not snapshotted by CLI\n"
    );
}
