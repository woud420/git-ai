use super::*;

pub fn baseline(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));
    assert!(!op_path(case, LEFT_ID).exists());
    assert!(!op_path(case, RIGHT_ID).exists());
    if case.name.ends_with(":zero") {
        return;
    }
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    assert_admission(
        case,
        first.admission(),
        &saved,
        &zero,
        &[MERGE_ID],
        &[],
        (&[MERGE_ID], false),
    );
    assert_cursor(first.current_cursor(), &saved, 1, &[MERGE_ID]);
    let before = total_state(case);
    let retry = outcome(admit_now(case, &mut journal, config, &zero), true);
    assert_eq!(
        retry.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    assert!(
        total_state(case) == before,
        "exact retry changed durable state"
    );
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    assert_admission(
        case,
        second.admission(),
        &saved,
        first.current_cursor(),
        &[MERGE_ID],
        &[],
        (&[MERGE_ID], false),
    );
    assert_ne!(
        second.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    assert_cursor(second.current_cursor(), &saved, 2, &[MERGE_ID]);
    let a = native::rich_parent();
    let heads = [MERGE_ID, a.operation_id.as_str()];
    select(case, std::slice::from_ref(&a), &heads, None);
    let third = outcome(
        admit_now(case, &mut journal, config, second.current_cursor()),
        false,
    );
    assert_cursor(third.current_cursor(), &saved, 3, &heads);
    let mut reversed = third.current_cursor().admitted_head_ids().to_vec();
    assert_eq!(reversed.len(), 2);
    reversed.reverse();
    let mut expected = third.current_cursor().expectation();
    expected.admitted_head_ids = &reversed;
    let fourth = outcome(
        admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            expected,
            deadline(),
            &mut admission_budget(),
        )
        .unwrap(),
        false,
    );
    assert_admission(
        case,
        fourth.admission(),
        &saved,
        third.current_cursor(),
        &heads,
        std::slice::from_ref(&a),
        (&[MERGE_ID], false),
    );
    assert_cursor(fourth.current_cursor(), &saved, 4, &heads);
    let before = total_state(case);
    let retry = outcome(
        admit_now(case, &mut journal, config, third.current_cursor()),
        true,
    );
    assert_eq!(
        retry.admission().receipt().admission_id(),
        fourth.admission().receipt().admission_id()
    );
    assert!(
        total_state(case) == before,
        "head presentation order changed request identity"
    );
}

pub fn chain(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(
        case,
        std::slice::from_ref(&a),
        &[&a.operation_id],
        Some(&a.operation_id),
    );
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    same_receipt(first.registration(), &saved);
    assert_admission(
        case,
        first.admission(),
        &saved,
        &zero,
        &[&a.operation_id],
        std::slice::from_ref(&a),
        (&[MERGE_ID], false),
    );
    select(
        case,
        std::slice::from_ref(&b),
        &[&b.operation_id],
        Some(&a.operation_id),
    );
    let records = [a.clone(), b.clone()];
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    assert_admission(
        case,
        second.admission(),
        &saved,
        first.current_cursor(),
        &[&b.operation_id],
        &records,
        (&[MERGE_ID], false),
    );
    assert_eq!(
        second.registration().checkout().operation_id,
        a.operation_id
    );
    assert_eq!(
        second.registration().checkout_relation(),
        JjRegisteredCheckoutRelation::OutsideBaseline
    );
    let id = second.admission().receipt().admission_id().to_owned();
    drop(journal);
    let journal = case.open();
    let observed = status(case, &journal, config);
    assert_cursor(observed.cursor(), &saved, 2, &[&b.operation_id]);
    assert_eq!(observed.latest_receipt().unwrap().admission_id(), id);
    assert_eq!(
        packet(case, &journal, saved.source_id(), &id).ordered_operations(),
        records
    );
    assert!(
        checked_known(
            case,
            &journal,
            saved.source_id(),
            &"ab".repeat(32),
            deadline(),
            &mut admission_budget()
        )
        .unwrap()
        .is_none()
    );
    let opaque = journal.status(saved.source_id()).unwrap();
    assert_eq!(opaque.generation, 0);
    assert_eq!(opaque.pending_operations, 0);
    assert!(opaque.observed_heads.is_empty() && opaque.applied_heads.is_empty());
}

pub fn retry(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&a.operation_id], None);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    case.heads(&[&b.operation_id]);
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    case.heads(&[&a.operation_id]);
    let before = total_state(case);
    let retried = outcome(admit_now(case, &mut journal, config, &zero), true);
    assert_eq!(
        retried.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    assert_cursor(retried.current_cursor(), &saved, 2, &[&b.operation_id]);
    assert!(
        total_state(case) == before,
        "historical retry rewound current cursor"
    );
    let third = outcome(
        admit_now(case, &mut journal, config, second.current_cursor()),
        false,
    );
    assert_cursor(third.current_cursor(), &saved, 3, &[&a.operation_id]);
    assert_ne!(
        third.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        first.current_cursor().expectation(),
        deadline(),
        &mut admission_budget(),
    ));
    assert_eq!(status(case, &journal, config).cursor().generation(), 3);
}

pub fn lost_reply(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&a.operation_id], None);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    let original_id = first.admission().receipt().admission_id().to_owned();
    drop(first);
    case.heads(&[&b.operation_id]);
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        zero.expectation(),
        deadline(),
        &mut admission_budget(),
    ));
    let recovered = status(case, &journal, config);
    assert_eq!(
        recovered.latest_receipt().unwrap().admission_id(),
        original_id
    );
    assert_eq!(
        packet(case, &journal, saved.source_id(), &original_id)
            .receipt()
            .captured_head_ids(),
        [a.operation_id]
    );
    let second = outcome(
        admit_now(case, &mut journal, config, recovered.cursor()),
        false,
    );
    assert_eq!(second.current_cursor().generation(), 2);
    assert_eq!(
        second.admission().receipt().captured_head_ids(),
        [b.operation_id]
    );
}

pub fn closure(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let branch = native::late_branch();
    opaque(case, &mut journal, saved.source_id());
    select(
        case,
        std::slice::from_ref(&branch),
        &[&branch.operation_id],
        None,
    );
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        zero.expectation(),
        deadline(),
        &mut admission_budget(),
    ));
    let mut records = vec![first(), left(), branch];
    let mixed = case.name.ends_with(":mixed");
    if mixed {
        records.push(native::mixed_merge());
    }
    let head = records.last().unwrap().operation_id.clone();
    select(case, &records, &[&head], None);
    let current = outcome(admit_now(case, &mut journal, config, &zero), false);
    let reached = if mixed { vec![MERGE_ID] } else { vec![] };
    assert_admission(
        case,
        current.admission(),
        &saved,
        &zero,
        &[&head],
        &records,
        (&reached, true),
    );
    assert_eq!(current.registration().baseline().receipt().generation(), 1);
    assert_eq!(journal.status(saved.source_id()).unwrap().generation, 1);
}

pub fn isolation(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    let other_root = case.root.join("independent source");
    let repo_dir = layout(&other_root, false);
    write_evidence_to(&repo_dir, &merge());
    fs::write(repo_dir.join("op_heads/heads").join(MERGE_ID), []).unwrap();
    fs::write(
        other_root.join(".jj/working_copy/checkout"),
        checkout_bytes(MERGE_ID, "default"),
    )
    .unwrap();
    let context = discover(&other_root).unwrap();
    let other = installed(
        register_current_state(
            &mut journal,
            &context,
            config,
            deadline(),
            &mut admission_budget(),
        )
        .unwrap(),
    );
    assert_ne!(saved.source_id(), other.source_id());
    assert_eq!(saved.baseline().anchors(), other.baseline().anchors());
    failed(admit_checked(
        case,
        &mut journal,
        &context,
        config,
        zero.expectation(),
        deadline(),
        &mut admission_budget(),
    ));
    let observed = checked_status(
        case,
        &journal,
        &context,
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap();
    assert_eq!(observed.cursor().generation(), 0);
    assert_eq!(observed.cursor().source_id(), other.source_id());
    assert!(
        checked_known(
            case,
            &journal,
            other.source_id(),
            first.admission().receipt().admission_id(),
            deadline(),
            &mut admission_budget()
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(status(case, &journal, config).cursor().generation(), 1);
}

pub fn readonly(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    outcome(admit_now(case, &mut journal, config, &zero), false);
    let before = total_state(case);
    let collected = collect(case, &journal, config);
    assert_eq!(collected.ordered_operations(), [a]);
    same_receipt(collected.registration(), &saved);
    assert!(
        total_state(case) == before,
        "read-only collector changed admission tables"
    );
}
