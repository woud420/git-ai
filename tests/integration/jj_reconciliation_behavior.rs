use super::*;

pub(super) fn unchanged(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    if case.name.ends_with(":zero") {
        for _ in 0..3 {
            let result = checks::unchanged(now(case, &mut journal, config, &target, &zero));
            same_receipt(result.registration(), &saved);
            assert_cursor(result.cursor(), &saved, 0, &[MERGE_ID]);
            assert!(result.latest_receipt().is_none());
            assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));
        }
        return;
    }
    let a = native::rich_parent();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    let first = admitted(now(case, &mut journal, config, &target, &zero), false);
    assert_admission(
        case,
        first.admission(),
        &saved,
        &zero,
        &[&a.operation_id],
        std::slice::from_ref(&a),
        (&[MERGE_ID], false),
    );
    assert_eq!(saved.baseline().receipt().captured_head_ids(), &[MERGE_ID]);
    case.write_checkout(&a.operation_id);
    for _ in 0..3 {
        let result = checks::unchanged(now(
            case,
            &mut journal,
            config,
            &target,
            first.current_cursor(),
        ));
        same_receipt(result.registration(), &saved);
        assert_cursor(result.cursor(), &saved, 1, &[&a.operation_id]);
        assert_eq!(
            result.latest_receipt().unwrap().admission_id(),
            first.admission().receipt().admission_id()
        );
        assert_eq!(
            result.registration().checkout().operation_id,
            a.operation_id
        );
        assert_eq!(
            result.registration().checkout_relation(),
            JjRegisteredCheckoutRelation::OutsideBaseline
        );
    }
    assert_eq!(
        admission_rows(case)
            .iter()
            .map(|(_, rows)| rows.len())
            .collect::<Vec<_>>(),
        [1, 1]
    );
}

pub(super) fn no_walk(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&b.operation_id], None);
    let first = admitted(now(case, &mut journal, config, &target, &zero), false);
    assert_admission(
        case,
        first.admission(),
        &saved,
        &zero,
        &[&b.operation_id],
        &[a.clone(), b.clone()],
        (&[MERGE_ID], false),
    );
    fs::remove_file(op_path(case, &a.operation_id)).unwrap();
    assert!(!op_path(case, &a.operation_id).exists());
    let current = capture_current_state(&case.context(), deadline()).unwrap();
    assert_eq!(current.checkout().operation_id, MERGE_ID);
    assert!(
        collect_registered_history(
            &journal,
            &case.context(),
            config,
            deadline(),
            &mut admission_budget()
        )
        .is_err()
    );
    let result = checks::unchanged(now(
        case,
        &mut journal,
        config,
        &target,
        first.current_cursor(),
    ));
    assert_cursor(result.cursor(), &saved, 1, &[&b.operation_id]);
    assert_eq!(
        packet(
            case,
            &journal,
            saved.source_id(),
            first.admission().receipt().admission_id()
        )
        .ordered_operations(),
        [a, b.clone()]
    );
    case.heads(&[MERGE_ID, &b.operation_id]);
    refuse(case, &mut journal, config, &target, first.current_cursor());
}

pub(super) fn manual(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    assert_cursor(first.current_cursor(), &saved, 1, &[MERGE_ID]);
    let manual_retry = outcome(admit_now(case, &mut journal, config, &zero), true);
    assert_eq!(
        manual_retry.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    refuse(case, &mut journal, config, &target, &zero);
    checks::unchanged(now(
        case,
        &mut journal,
        config,
        &target,
        first.current_cursor(),
    ));
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    assert_cursor(second.current_cursor(), &saved, 2, &[MERGE_ID]);
    assert_ne!(
        second.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    refuse(case, &mut journal, config, &target, first.current_cursor());
    checks::unchanged(now(
        case,
        &mut journal,
        config,
        &target,
        second.current_cursor(),
    ));
}

pub(super) fn retry(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&a.operation_id], None);
    let first = admitted(now(case, &mut journal, config, &target, &zero), false);
    let retry = admitted(now(case, &mut journal, config, &target, &zero), true);
    assert_eq!(
        retry.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    case.heads(&[&b.operation_id]);
    let second = admitted(
        now(case, &mut journal, config, &target, first.current_cursor()),
        false,
    );
    assert_admission(
        case,
        second.admission(),
        &saved,
        first.current_cursor(),
        &[&b.operation_id],
        &[a.clone(), b.clone()],
        (&[MERGE_ID], false),
    );
    case.heads(&[&a.operation_id]);
    let historical = admitted(now(case, &mut journal, config, &target, &zero), true);
    assert_eq!(
        historical.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    assert_cursor(historical.current_cursor(), &saved, 2, &[&b.operation_id]);
    assert_eq!(historical.admission().receipt().generation(), 1);
    let third = admitted(
        now(case, &mut journal, config, &target, second.current_cursor()),
        false,
    );
    assert_cursor(third.current_cursor(), &saved, 3, &[&a.operation_id]);
    assert_ne!(
        third.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    refuse(case, &mut journal, config, &target, first.current_cursor());
    checks::unchanged(now(
        case,
        &mut journal,
        config,
        &target,
        third.current_cursor(),
    ));
}

pub(super) fn restart(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&a.operation_id], None);
    let id = admitted(now(case, &mut journal, config, &target, &zero), false)
        .admission()
        .receipt()
        .admission_id()
        .to_owned();
    drop(journal);
    let mut journal = case.open();
    let retry = admitted(now(case, &mut journal, config, &target, &zero), true);
    assert_eq!(retry.admission().receipt().admission_id(), id);
    case.heads(&[&b.operation_id]);
    refuse(case, &mut journal, config, &target, &zero);
    let current = status(case, &journal, config);
    assert_cursor(current.cursor(), &saved, 1, &[&a.operation_id]);
    let second = admitted(
        now(case, &mut journal, config, &target, current.cursor()),
        false,
    );
    assert_cursor(second.current_cursor(), &saved, 2, &[&b.operation_id]);
    assert_eq!(
        packet(case, &journal, saved.source_id(), &id)
            .receipt()
            .generation(),
        1
    );
}

pub(super) fn heads(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    let records = native::chain(32);
    let heads: Vec<_> = records
        .iter()
        .map(|record| record.operation_id.as_str())
        .collect();
    select(case, &records, &heads, None);
    let first = admitted(now(case, &mut journal, config, &target, &zero), false);
    assert_admission(
        case,
        first.admission(),
        &saved,
        &zero,
        &heads,
        &records,
        (&[MERGE_ID], false),
    );
    let mut reversed = first.current_cursor().admitted_head_ids().to_vec();
    assert_eq!(reversed.len(), 32);
    reversed.reverse();
    let mut expected = first.current_cursor().expectation();
    expected.admitted_head_ids = &reversed;
    let result = checks::unchanged(
        reconcile(
            case,
            &mut journal,
            config,
            target.with(expected),
            deadline(),
            &mut admission_budget(),
        )
        .unwrap(),
    );
    assert_cursor(result.cursor(), &saved, 1, &heads);
}
