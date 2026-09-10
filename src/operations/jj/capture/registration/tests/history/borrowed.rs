use super::*;
use crate::operations::jj::capture::registration::BorrowedJjHistoryEvidence;

fn work(current: &RetainedCapture<'_>) -> [usize; 10] {
    let counts = current.budget.counters();
    let (bytes, files) = current.read_remaining();
    [
        counts.directory_components,
        counts.retained_edges,
        counts.directory_open_attempts,
        counts.live_directory_descriptors,
        counts.peak_live_directory_descriptors,
        counts.head_calls[0],
        counts.head_calls[1],
        counts.retained_anchor_bytes,
        bytes,
        files,
    ]
}

fn raw_pointers(view: &BorrowedJjHistoryEvidence<'_>) -> Vec<(*const u8, *const u8)> {
    let records: &[&JjOperationEvidence] = view.ordered_operations();
    records
        .iter()
        .map(|record| {
            assert!(!record.operation_bytes.is_empty() && !record.view_bytes.is_empty());
            (record.operation_bytes.as_ptr(), record.view_bytes.as_ptr())
        })
        .collect()
}

fn assert_owned(
    owned: &CapturedJjHistoryEvidence,
    expected: &[JjOperationEvidence],
    pointers: &[(*const u8, *const u8)],
) {
    assert_eq!(owned.ordered_operations(), expected);
    assert_eq!(owned.ordered_operations().len(), pointers.len());
    for (record, &(operation, view)) in owned.ordered_operations().iter().zip(pointers) {
        assert_eq!(record.operation_bytes.as_ptr(), operation);
        assert_eq!(record.view_bytes.as_ptr(), view);
    }
}

#[test]
fn borrowed_history_requires_complete_without_poisoning_collection_or_consuming_it() {
    let (fixture, baseline) = fixture(&[fixtures::rich_parent()]);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    let before = work(&current);
    require_error(current.borrow_history());
    assert_eq!(work(&current), before);
    current.collect_history(&baseline).unwrap();
    assert_eq!(
        current.borrow_history().unwrap().ordered_operations().len(),
        1
    );
    let before = work(&current);
    require_error(current.collect_history(&baseline));
    assert_eq!(work(&current), before);
    assert_eq!(
        current.borrow_history().unwrap().ordered_operations().len(),
        1
    );
    current.final_recheck().unwrap();
    assert_eq!(
        current.into_history().unwrap().ordered_operations().len(),
        1
    );
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn borrowed_history_failed_collection_cannot_be_repaired_in_the_same_session() {
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    fs::remove_file(operation_path(&fixture, &records[0].operation_id)).unwrap();
    require_error(current.collect_history(&baseline));
    write_records(&fixture, &records[..1]);
    let before = work(&current);
    require_error(current.borrow_history());
    require_error(current.collect_history(&baseline));
    require_error(current.borrow_history());
    assert_eq!(work(&current), before);
    current.final_recheck().unwrap();
    require_error(current.borrow_history());
    require_error(current.into_history());
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn borrowed_history_registration_mode_has_no_borrowable_history() {
    let (fixture, baseline) = fixture(&[fixtures::rich_parent()]);
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut current = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    let before = work(&current);
    require_error(current.borrow_history());
    require_error(current.collect_history(&baseline));
    require_error(current.borrow_history());
    assert_eq!(work(&current), before);
    drop(current);
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
}

#[test]
fn borrowed_history_shares_head_and_ancestor_buffers_through_existing_owned_handoff() {
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    set_checkout(&fixture, &records[0]);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    let before = work(&current);
    let history_events = hooks.events.clone();
    let events = hooks.inner.events.clone();
    let pointers = {
        let first = current.borrow_history().unwrap();
        let second = current.borrow_history().unwrap();
        assert!(std::ptr::eq(
            first.head_ids(),
            current.captured().head_ids()
        ));
        assert!(std::ptr::eq(first.head_ids(), second.head_ids()));
        assert!(std::ptr::eq(
            first.reached_baseline_ids(),
            second.reached_baseline_ids()
        ));
        assert_eq!(
            first.reached_baseline_ids(),
            baseline.receipt().captured_head_ids()
        );
        assert!(!first.reaches_root());
        assert_eq!(first.ordered_operations().len(), records.len());
        for ((left, right), expected) in first
            .ordered_operations()
            .iter()
            .zip(second.ordered_operations())
            .zip(&records)
        {
            assert!(std::ptr::eq(*left, *right));
            assert_eq!(*left, expected);
        }
        assert!(std::ptr::eq(
            first.ordered_operations()[1],
            &current.captured().anchors()[0]
        ));
        assert!(!std::ptr::eq(
            first.ordered_operations()[0],
            current.captured().checkout_evidence()
        ));
        raw_pointers(&first)
    };
    assert_eq!(work(&current), before);
    assert_eq!(hooks.events, history_events);
    assert_eq!(hooks.inner.events, events);
    current.final_recheck_with(&mut hooks).unwrap();
    let owned = current.into_history().unwrap();
    assert_owned(&owned, &records, &pointers);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn borrowed_history_does_not_reopen_native_files_or_charge_work_after_gc() {
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    for record in &records {
        fs::remove_file(operation_path(&fixture, &record.operation_id)).unwrap();
    }
    let views: std::collections::BTreeSet<_> = records
        .iter()
        .map(|record| record.view_id.as_str())
        .collect();
    for view in views {
        fs::remove_file(view_path(&fixture, view)).unwrap();
    }
    let before = work(&current);
    let history_events = hooks.events.clone();
    let events = hooks.inner.events.clone();
    let pointers = {
        let view = current.borrow_history().unwrap();
        assert_eq!(view.ordered_operations().len(), 2);
        raw_pointers(&view)
    };
    assert_eq!(work(&current), before);
    assert_eq!(hooks.events, history_events);
    assert_eq!(hooks.inner.events, events);
    current.final_recheck_with(&mut hooks).unwrap();
    assert_owned(&current.into_history().unwrap(), &records, &pointers);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn borrowed_history_preserves_baseline_redundant_root_and_mixed_closure_facts() {
    for kind in ["baseline", "redundant", "root", "mixed"] {
        let mut records = match kind {
            "baseline" => vec![fixtures::merge()],
            "redundant" => vec![fixtures::rich_parent(), fixtures::rich_child()],
            _ => vec![fixtures::first(), fixtures::left(), fixtures::late_branch()],
        };
        if kind == "mixed" {
            records.push(fixtures::mixed_merge());
        }
        let (fixture, baseline) = fixture(&records);
        if kind == "redundant" {
            fixture.set_heads(&[
                fixtures::MERGE_ID,
                &records[0].operation_id,
                &records[1].operation_id,
            ]);
        }
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut current = budget.open(&fixture.context()).unwrap();
        canonical_policy(&mut current);
        current.collect_history(&baseline).unwrap();
        let before = work(&current);
        {
            let view = current.borrow_history().unwrap();
            assert_eq!(view.head_ids(), current.captured().head_ids());
            assert_eq!(view.reaches_root(), matches!(kind, "root" | "mixed"));
            assert_eq!(view.reached_baseline_ids().is_empty(), kind == "root");
            if kind == "baseline" {
                assert!(view.ordered_operations().is_empty());
            } else {
                assert_eq!(view.ordered_operations().len(), records.len());
                for (actual, expected) in view.ordered_operations().iter().zip(&records) {
                    assert_eq!(*actual, expected);
                }
            }
            if kind == "redundant" {
                assert_eq!(view.head_ids().len(), 3);
            }
        }
        assert_eq!(work(&current), before);
        current.final_recheck().unwrap();
        let owned = current.into_history().unwrap();
        assert_eq!(owned.reaches_root(), matches!(kind, "root" | "mixed"));
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
}

#[test]
fn borrowed_history_any_final_attempt_closes_views_but_preserves_owned_handoff_rules() {
    for fail_final in [false, true] {
        let records = [fixtures::rich_parent()];
        let (fixture, baseline) = fixture(&records);
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut current = budget.open(&fixture.context()).unwrap();
        canonical_policy(&mut current);
        current.collect_history(&baseline).unwrap();
        let pointers = {
            let view = current.borrow_history().unwrap();
            raw_pointers(&view)
        };
        if fail_final {
            fs::write(fixture.root.join(".git/HEAD"), b"ref: refs/heads/changed\n").unwrap();
        }
        let result = current.final_recheck();
        if fail_final {
            require_error(result);
        } else {
            result.unwrap();
        }
        let before = work(&current);
        require_error(current.borrow_history());
        require_error(current.final_recheck());
        assert_eq!(work(&current), before);
        if fail_final {
            require_error(current.into_history());
        } else {
            assert_owned(&current.into_history().unwrap(), &records, &pointers);
        }
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
}

#[test]
fn borrowed_history_view_cannot_authorize_owned_handoff_before_final_recheck() {
    let (fixture, baseline) = fixture(&[fixtures::rich_parent()]);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    current.collect_history(&baseline).unwrap();
    {
        let view = current.borrow_history().unwrap();
        assert_eq!(view.ordered_operations().len(), 1);
    }
    require_error(current.into_history());
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn borrowed_history_maximum_verified_count_is_bounded_metadata_without_rewalking() {
    let records = fixtures::chain(256);
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    let before = work(&current);
    let events = hooks.events.clone();
    let pointers = {
        let view = current.borrow_history().unwrap();
        assert_eq!(view.ordered_operations().len(), 256);
        raw_pointers(&view)
    };
    assert_eq!(work(&current), before);
    assert_eq!(hooks.events, events);
    current.final_recheck_with(&mut hooks).unwrap();
    assert_owned(&current.into_history().unwrap(), &records, &pointers);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}
