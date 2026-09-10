use super::*;

pub fn baseline(case: &Case, config: &Config) {
    let (journal, saved) = install(case, config);
    assert!(!op_path(case, LEFT_ID).exists());
    assert!(!op_path(case, RIGHT_ID).exists());
    let result = collect(case, &journal, config);
    assert_result(&result, &saved, &[MERGE_ID], &[], &[MERGE_ID], false);
    assert!(matches!(
        result.registration().checkout_relation(),
        JjRegisteredCheckoutRelation::BaselineAnchor
    ));
    assert_eq!(result.registration().checkout().operation_id, MERGE_ID);
}

pub fn chain(case: &Case, config: &Config, stale: bool) {
    let (journal, saved) = install(case, config);
    let records = [native::rich_parent(), native::rich_child()];
    assert_eq!(records[0], super::super::vectors::successor());
    assert_eq!(records[1].parent_ids, [records[0].operation_id.clone()]);
    write_records(case, &records);
    let head = &records[1].operation_id;
    case.heads(&[head]);
    let checkout = &records[usize::from(!stale)].operation_id;
    case.write_checkout(checkout);
    for _ in 0..2 {
        let result = collect(case, &journal, config);
        assert_result(&result, &saved, &[head], &records, &[MERGE_ID], false);
        assert_eq!(&result.registration().checkout().operation_id, checkout);
        assert!(matches!(
            result.registration().checkout_relation(),
            JjRegisteredCheckoutRelation::OutsideBaseline
        ));
    }
    assert_eq!(journal.status(saved.source_id()).unwrap().generation, 0);
}

pub fn redundant(case: &Case, config: &Config) {
    let (journal, saved) = install(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    let merged = native::converged_merge();
    assert_eq!(
        merged.parent_ids,
        [a.operation_id.clone(), b.operation_id.clone()]
    );
    let records = [a, b, merged];
    write_records(case, &records);
    let heads = [MERGE_ID, &records[0].operation_id, &records[2].operation_id];
    case.heads(&heads);
    case.write_checkout(&records[2].operation_id);
    let result = collect(case, &journal, config);
    assert_result(&result, &saved, &heads, &records, &[MERGE_ID], false);
}

pub fn late(case: &Case, config: &Config) {
    let (mut journal, saved) = install(case, config);
    let branch = native::late_branch();
    assert_eq!(branch.parent_ids, [LEFT_ID]);
    assert!(
        saved.baseline().anchors()[0]
            .parent_ids
            .contains(&LEFT_ID.to_owned())
    );
    write_records(case, std::slice::from_ref(&branch));
    case.heads(&[&branch.operation_id]);
    opaque(case, &mut journal, saved.source_id());
    let old_status = journal.status(saved.source_id()).unwrap();
    rejected(case, &journal, config);
    case.write_evidence(&left());
    assert!(!op_path(case, FIRST_ID).exists());
    rejected(case, &journal, config);
    case.write_evidence(&first());
    let result = collect(case, &journal, config);
    assert_result(
        &result,
        &saved,
        &[&branch.operation_id],
        &[first(), left(), branch.clone()],
        &[],
        true,
    );
    assert_eq!(journal.status(saved.source_id()).unwrap(), old_status);
    assert_eq!(result.registration().baseline().receipt().generation(), 1);
}

pub fn mixed(case: &Case, config: &Config) {
    let (journal, saved) = install(case, config);
    let branch = native::late_branch();
    let mixed = native::mixed_merge();
    assert_eq!(mixed.parent_ids, [MERGE_ID, &branch.operation_id]);
    let records = [first(), left(), branch, mixed];
    write_records(case, &records);
    let head = &records[3].operation_id;
    case.heads(&[head]);
    let result = collect(case, &journal, config);
    assert_result(&result, &saved, &[head], &records, &[MERGE_ID], true);
}

pub fn unrelated(case: &Case, config: &Config) {
    let (mut journal, saved) = install(case, config);
    opaque(case, &mut journal, saved.source_id());
    let a = native::rich_parent();
    write_records(case, std::slice::from_ref(&a));
    case.write_checkout(&a.operation_id);
    fs::write(
        op_path(case, LEFT_ID),
        b"malformed unrelated native operation",
    )
    .unwrap();
    fs::write(
        view_path(case, &"ee".repeat(64)),
        b"malformed unrelated view",
    )
    .unwrap();
    let result = collect(case, &journal, config);
    assert_result(&result, &saved, &[MERGE_ID], &[], &[MERGE_ID], false);
    assert_eq!(
        result.registration().checkout().operation_id,
        a.operation_id
    );
    assert!(matches!(
        result.registration().checkout_relation(),
        JjRegisteredCheckoutRelation::OutsideBaseline
    ));
}
