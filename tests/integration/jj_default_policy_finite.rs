use super::*;

pub(super) fn run(case: &Case, config: &Config) {
    let (journal, saved, zero) = initial(case, config);
    let records = records();
    write_records(case, &records);
    select(case, &[], &heads(&records, false), None);
    let remembered = Remembered::from_status(&status(case, &journal, config));
    let mut expected = zero.clone();
    let mut source = state(case);
    let home = observe_home(&case.test_home);
    let mut process = observer(
        case,
        &finite_args(&case.journal_path, &remembered, 11, 2_000),
    );
    let mut first = None;
    for generation in 1..=10 {
        let value = process.next();
        assert!(
            value.get("error").is_none(),
            "default finite observation refused: {value}"
        );
        let paused = process.pause();
        let both = generation % 2 == 0;
        let current = status(case, &journal, config);
        let admission = next_packet(case, &journal, saved.source_id(), &value);
        let raw = assert_packet(case, &admission, &saved, &expected, &records, both);
        assert_eq!(
            value,
            changed_json(&admission, &current, generation, "admitted")
        );
        assert_cursor(
            current.cursor(),
            &saved,
            generation as u64,
            &heads(&records, both),
        );
        assert_eq!(state(case), source);
        assert_eq!(observe_home(&case.test_home), home);
        require_rows(case, generation);
        idle_transaction(case);
        if generation == 1 {
            first = Some((admission.receipt().admission_id().to_owned(), raw));
        }
        expected = current.cursor().clone();
        if generation < 10 {
            select(case, &[], &heads(&records, !both), None);
            source = state(case);
        }
        drop(paused);
    }
    let current = status(case, &journal, config);
    let before = total_state(case);
    assert_eq!(process.next(), unchanged_json(&current, 11));
    process.finish(true);
    assert!(total_state(case) == before);
    assert_cursor(current.cursor(), &saved, 10, &heads(&records, true));

    let (first_id, original) = first.unwrap();
    select(case, &[], &heads(&records, false), None);
    let before = total_state(case);
    let mut retry = observer(
        case,
        &finite_args(&case.journal_path, &remembered, 2, 2_000),
    );
    let value = retry.next();
    let paused = retry.pause();
    let admission = next_packet(case, &journal, saved.source_id(), &value);
    assert_eq!(admission.receipt().admission_id(), first_id);
    assert_eq!(
        assert_packet(case, &admission, &saved, &zero, &records, false),
        original
    );
    assert_eq!(
        value,
        changed_json(&admission, &current, 1, "already_admitted")
    );
    assert!(total_state(case) == before);
    select(case, &[], &heads(&records, true), None);
    let before = total_state(case);
    drop(paused);
    assert_eq!(retry.next(), unchanged_json(&current, 2));
    retry.finish(true);
    assert!(total_state(case) == before);
    require_rows(case, 10);
}
