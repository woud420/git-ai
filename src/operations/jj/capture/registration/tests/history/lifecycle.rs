use super::*;

#[test]
fn history_collects_parent_first_without_moving_live_capture_anchors() {
    let records = vec![fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    set_checkout(&fixture, records.last().unwrap());
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    let anchor_pointer = current.captured().anchors()[0].operation_bytes.as_ptr();
    let view_pointer = current.captured().anchors()[0].view_bytes.as_ptr();
    let checkout_id = current.captured().checkout_evidence().operation_id.clone();
    current.collect_history(&baseline).unwrap();
    assert_eq!(
        current.captured().anchors()[0].operation_bytes.as_ptr(),
        anchor_pointer
    );
    assert_eq!(
        current.captured().checkout_evidence().operation_id,
        checkout_id
    );
    current.final_recheck().unwrap();
    let history = current.into_history().unwrap();
    assert_eq!(
        ids(&history),
        records
            .iter()
            .map(|r| r.operation_id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        history.head_ids(),
        std::slice::from_ref(&records[1].operation_id)
    );
    assert_eq!(
        history.reached_baseline_ids(),
        baseline.receipt().captured_head_ids()
    );
    assert!(!history.reaches_root());
    assert_eq!(
        history.ordered_operations()[1].operation_bytes.as_ptr(),
        anchor_pointer
    );
    assert_eq!(
        history.ordered_operations()[1].view_bytes.as_ptr(),
        view_pointer
    );
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_baseline_only_has_no_extension_or_parent_reads() {
    let records = [fixtures::merge()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    current.final_recheck_with(&mut hooks).unwrap();
    let history = current.into_history().unwrap();
    assert!(history.ordered_operations().is_empty());
    assert_eq!(history.head_ids(), baseline.receipt().captured_head_ids());
    assert!(
        !hooks
            .events
            .iter()
            .any(|event| matches!(event, HistoryEvent::Before(_)))
    );
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_mode_requires_policy_scope_and_present_seal_before_walking() {
    let records = [fixtures::rich_parent()];
    let (fixture, good) = fixture(&records);
    let wrong = baseline(&fixture, &"a1".repeat(32));
    for canonical in [false, true] {
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut hooks = trace();
        let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
        if canonical {
            canonical_policy(&mut current);
        }
        require_error(
            current.collect_history_with(if canonical { &wrong } else { &good }, &mut hooks),
        );
        assert!(hooks.events.is_empty());
        if !canonical {
            canonical_policy(&mut current);
        }
        require_error(current.collect_history_with(&good, &mut hooks));
        assert!(hooks.events.is_empty());
        drop(current);
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
    let unsealed = Fixture::new();
    let cutoff = baseline(&unsealed, SOURCE);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&unsealed.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    require_error(current.collect_history_with(&cutoff, &mut hooks));
    assert!(hooks.events.is_empty());
}

#[test]
fn history_mode_cannot_publish_and_old_registration_mode_cannot_collect() {
    let unsealed = Fixture::new();
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&unsealed.context()).unwrap();
    canonical_policy(&mut current);
    let error = require_error(current.publish_new(SOURCE));
    assert!(error.to_string().contains("history"), "{error}");
    assert!(!unsealed.repo.join("git-ai").exists());
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    let (fixture, baseline) = fixture(&[fixtures::rich_parent()]);
    let mut old = RegistrationCaptureBudget::new(deadline());
    let mut current = old.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    let error = require_error(current.collect_history(&baseline));
    assert!(error.to_string().contains("history"), "{error}");
    drop(current);
    assert_eq!(old.session_counters(0).live_directory_descriptors, 0);
}

#[test]
fn history_requires_successful_collection_and_final_check_for_handoff() {
    let (fixture, baseline) = fixture(&[fixtures::rich_parent()]);
    for collect_first in [false, true] {
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut current = budget.open(&fixture.context()).unwrap();
        canonical_policy(&mut current);
        if collect_first {
            current.collect_history(&baseline).unwrap();
        }
        require_error(current.into_history());
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    current.collect_history(&baseline).unwrap();
    fs::write(fixture.root.join(".git/HEAD"), "ref: refs/heads/changed\n").unwrap();
    require_error(current.final_recheck());
    // final_checked is now true as an attempt; it is not success authority.
    require_error(current.into_history());
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_repeated_collect_and_collection_after_final_attempt_reject() {
    let (fixture, baseline) = fixture(&[fixtures::rich_parent()]);
    for first_is_collect in [false, true] {
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut current = budget.open(&fixture.context()).unwrap();
        canonical_policy(&mut current);
        if first_is_collect {
            current.collect_history(&baseline).unwrap();
        } else {
            current.final_recheck().unwrap();
        }
        let before = current.read_remaining();
        require_error(current.collect_history(&baseline));
        assert_eq!(current.read_remaining(), before);
    }
}

#[test]
fn history_failed_collection_cannot_resume_or_handoff_in_same_session() {
    let records = vec![fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    fs::remove_file(operation_path(&fixture, &records[0].operation_id)).unwrap();
    require_error(current.collect_history(&baseline));
    let after_failure = current.read_remaining();
    write_records(&fixture, &records[..1]);
    require_error(current.collect_history(&baseline));
    assert_eq!(current.read_remaining(), after_failure);
    current.final_recheck().unwrap();
    require_error(current.into_history());
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    assert_eq!(collect(&fixture, &baseline).ordered_operations().len(), 2);
}

#[test]
fn history_budget_is_one_shot_even_after_open_failure() {
    let fixture = Fixture::new();
    let context = fixture.context();
    fs::remove_file(fixture.repo.join("op_store/type")).unwrap();
    let mut budget = HistoryCaptureBudget::new(deadline());
    require_error(budget.open(&context));
    let after = remaining(&budget);
    fs::write(fixture.repo.join("op_store/type"), b"simple_op_store").unwrap();
    require_error(budget.open(&context));
    assert_eq!(remaining(&budget), after);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}
