use super::*;

#[path = "jj_reconciliation_target_fixture.rs"]
mod fixture;

#[test]
fn jj_reconciliation_saved_target_is_required_before_unchanged_changed_and_retry() {
    for colocated in [true, false] {
        run_case(
            "history:admission:reconcile:target:branches",
            colocated,
            Policy::Root,
        );
    }
}

#[test]
fn jj_reconciliation_target_shape_is_bounded_before_loading_registered_payloads() {
    run_case(
        "history:admission:reconcile:target:bounds",
        true,
        Policy::Root,
    );
}

#[test]
fn jj_reconciliation_status_refresh_cannot_replace_a_remembered_attachment() {
    run_case(
        "history:admission:reconcile:target:refresh",
        false,
        Policy::Root,
    );
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    match case.name.rsplit(':').next().unwrap() {
        "branches" => branches(case, config),
        "bounds" => bounds(case, config),
        "refresh" => refresh(case, config),
        name => panic!("unknown target fixture {name}"),
    }
}

fn reject(
    case: &Case,
    journal: &mut JjObservationJournal,
    config: &Config,
    cursor: &NativeAdmissionCursor,
    target: &SavedTarget,
    loaded: bool,
) {
    let mut reads = admission_budget();
    failed(reconcile(
        case,
        journal,
        config,
        target.with(cursor.expectation()),
        deadline(),
        &mut reads,
    ));
    if loaded {
        assert!(
            reads.consumed() > 0,
            "valid-shaped target never reached the saved registration join"
        );
    } else {
        assert_eq!(
            reads.consumed(),
            0,
            "malformed target loaded saved payloads"
        );
    }
}

fn incorrect(target: &SavedTarget) -> [SavedTarget; 2] {
    let mut name = target.clone();
    assert_eq!(name.workspace_name, "default");
    name.workspace_name = "Default".to_owned();
    let mut attachment = target.clone();
    attachment.attachment_id = fixture::different_id(&attachment.attachment_id);
    [name, attachment]
}

fn branches(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    checks::unchanged(now(case, &mut journal, config, &target, &zero));
    for wrong in incorrect(&target) {
        reject(case, &mut journal, config, &zero, &wrong, true);
    }
    let a = native::rich_parent();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    for wrong in incorrect(&target) {
        reject(case, &mut journal, config, &zero, &wrong, true);
    }
    assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));
    let first = admitted(now(case, &mut journal, config, &target, &zero), false);
    for wrong in incorrect(&target) {
        reject(case, &mut journal, config, &zero, &wrong, true);
        reject(
            case,
            &mut journal,
            config,
            first.current_cursor(),
            &wrong,
            true,
        );
    }
    let retry = admitted(now(case, &mut journal, config, &target, &zero), true);
    assert_eq!(
        retry.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    assert_cursor(retry.current_cursor(), &saved, 1, &[&a.operation_id]);
    checks::unchanged(now(
        case,
        &mut journal,
        config,
        &target,
        first.current_cursor(),
    ));
}

fn bounds(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    for name in [
        String::new(),
        "a".repeat(16 * 1024 + 1),
        format!("{}a", "é".repeat(8192)),
    ] {
        let mut wrong = target.clone();
        wrong.workspace_name = name;
        reject(case, &mut journal, config, &zero, &wrong, false);
    }
    for attachment in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        let mut wrong = target.clone();
        wrong.attachment_id = attachment;
        reject(case, &mut journal, config, &zero, &wrong, false);
    }
    for name in [
        "a".repeat(16 * 1024),
        "é".repeat(8192),
        "workspace-工".to_owned(),
    ] {
        let mut wrong = target.clone();
        wrong.workspace_name = name;
        // The 16 KiB expectation is shape-valid, not a claim that a physical
        // checkout with all those bytes fits its separate envelope cap.
        reject(case, &mut journal, config, &zero, &wrong, true);
    }
    let mut wrong = target.clone();
    wrong.attachment_id = fixture::different_id(&target.attachment_id);
    assert_eq!(wrong.attachment_id.len(), 64);
    reject(case, &mut journal, config, &zero, &wrong, true);
    checks::unchanged(now(case, &mut journal, config, &target, &zero));
}

fn refresh(case: &Case, config: &Config) {
    let (journal, _original, _zero) = initial(case, config);
    let remembered = fixture::select_nonoriginal(case, &journal, config);
    let target = SavedTarget::from_registered(remembered.registration());
    let expected = remembered.cursor().clone();
    drop(journal);
    let mut journal = case.open();
    checks::unchanged(now(case, &mut journal, config, &target, &expected));
    fixture::replace_attachment(case, &target.attachment_id);
    let refreshed = status(case, &journal, config);
    assert_eq!(refreshed.cursor(), &expected);
    assert_eq!(
        refreshed.registration().workspace_name(),
        target.workspace_name
    );
    assert_ne!(
        refreshed.registration().attachment_id(),
        target.attachment_id
    );
    assert_eq!(
        refreshed.registration().baseline().anchors(),
        remembered.registration().baseline().anchors()
    );
    drop(journal);
    let mut journal = case.open();
    reject(
        case,
        &mut journal,
        config,
        refreshed.cursor(),
        &target,
        true,
    );
    let a = native::rich_parent();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    let moved = status(case, &journal, config);
    assert_eq!(moved.cursor(), &expected);
    reject(case, &mut journal, config, moved.cursor(), &target, true);
    assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));

    // This is a deliberate fixture retarget control, not a worker refresh rule.
    let selected_target = SavedTarget::from_registered(refreshed.registration());
    let first = admitted(
        now(case, &mut journal, config, &selected_target, &expected),
        false,
    );
    reject(case, &mut journal, config, &expected, &target, true);
    let retry = admitted(
        now(case, &mut journal, config, &selected_target, &expected),
        true,
    );
    assert_eq!(
        retry.admission().receipt().admission_id(),
        first.admission().receipt().admission_id()
    );
    assert_eq!(retry.current_cursor().generation(), 1);
    assert_eq!(
        target.attachment_id,
        remembered.registration().attachment_id()
    );
}
