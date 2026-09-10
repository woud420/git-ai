use super::*;

pub(super) fn unchanged(case: &Case, config: &Config) {
    let (journal, _, _) = initial(case, config);
    let current = status(case, &journal, config);
    let remembered = Remembered::from_status(&current);
    fs::write(
        case.root.join("observer-dirty.txt"),
        b"uncommitted observer input\n",
    )
    .unwrap();
    let before = total_state(case);
    let cases = [
        observe_args(&case.journal_path, &remembered),
        finite_args(&case.journal_path, &remembered, 1, 60_000),
        finite_args(&case.journal_path, &remembered, 3, 250),
    ];
    for (index, args) in cases.iter().enumerate() {
        let count = if index == 2 { 3 } else { 1 };
        let mut process = observer(case, args);
        for attempt in 1..=count {
            let value = process.next();
            assert_eq!(value, unchanged_json(&current, attempt));
            check_metadata_paths(case, &value);
        }
        process.finish(true);
        assert!(
            total_state(case) == before,
            "unchanged observation modified stored or working state"
        );
    }
}

pub(super) fn changed(case: &Case, config: &Config) {
    let (journal, saved, _) = initial(case, config);
    let zero = status(case, &journal, config);
    let remembered = Remembered::from_status(&zero);
    let a = native::rich_parent();
    write_records(case, std::slice::from_ref(&a));
    let mut process = observer(
        case,
        &finite_args(&case.journal_path, &remembered, 3, 2_000),
    );
    let before = total_state(case);
    assert_eq!(process.next(), unchanged_json(&zero, 1));
    process.running();
    assert!(total_state(case) == before);
    idle_transaction(case);
    select(case, &[], &[&a.operation_id], Some(&a.operation_id));
    fs::write(
        case.root.join("observer-dirty.txt"),
        b"still dirty after changed observation\n",
    )
    .unwrap();
    let before = state(case);
    let home_before = observe_home(&case.test_home);
    let second = process.next();
    let current = status(case, &journal, config);
    let packet = next_packet(case, &journal, saved.source_id(), &second);
    assert_eq!(packet.ordered_operations(), std::slice::from_ref(&a));
    assert_eq!(second, changed_json(&packet, &current, 2, "admitted"));
    assert_cursor(current.cursor(), &saved, 1, &[&a.operation_id]);
    assert_eq!(state(case), before);
    assert_eq!(observe_home(&case.test_home), home_before);
    idle_transaction(case);
    let after = total_state(case);
    assert_eq!(process.next(), unchanged_json(&current, 3));
    process.finish(true);
    assert!(total_state(case) == after);
    same_receipt(current.registration(), &saved);
}

pub(super) fn retry(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let remembered = Remembered::from_status(&status(case, &journal, config));
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    select(case, std::slice::from_ref(&b), &[&b.operation_id], None);
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    select(case, &[], &[&a.operation_id], None);
    let current = status(case, &journal, config);
    assert_cursor(current.cursor(), &saved, 2, &[&b.operation_id]);
    let before = total_state(case);
    let mut process = observer(
        case,
        &finite_args(&case.journal_path, &remembered, 2, 2_000),
    );
    assert_eq!(
        process.next(),
        changed_json(first.admission(), &current, 1, "already_admitted")
    );
    assert!(total_state(case) == before);
    assert_ne!(
        first.admission().receipt().generation(),
        second.current_cursor().generation()
    );
    process.running();
    select(case, &[], &[&b.operation_id], None);
    let before = total_state(case);
    assert_eq!(process.next(), unchanged_json(&current, 2));
    process.finish(true);
    assert!(
        total_state(case) == before,
        "retry carried its historical generation instead of the current cursor"
    );
}
