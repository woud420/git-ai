use super::*;

#[test]
fn history_union_accepts_256_pairs_and_reuses_the_sampled_head() {
    let records = fixtures::chain(256);
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    // The root was already read; walking must use its retained bytes.
    fs::remove_file(operation_path(&fixture, &records[255].operation_id)).unwrap();
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    current.final_recheck_with(&mut hooks).unwrap();
    let history = current.into_history().unwrap();
    assert_eq!(history.ordered_operations().len(), 256);
    assert_eq!(
        ids(&history),
        records
            .iter()
            .map(|r| r.operation_id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        hooks
            .events
            .iter()
            .filter(|event| matches!(event, HistoryEvent::Before(_)))
            .count(),
        255
    );
    assert!(640 - remaining(&budget).1 > 192);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_union_counts_sampled_baseline_heads_before_reading_257th_pair() {
    for (count, succeeds) in [(255, true), (256, false)] {
        let records = fixtures::chain(count);
        let (fixture, baseline) = fixture(&records);
        let anchor = fixtures::merge();
        fixture.set_heads(&[&anchor.operation_id, &records[count - 1].operation_id]);
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut hooks = trace();
        let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
        canonical_policy(&mut current);
        let result = current.collect_history_with(&baseline, &mut hooks);
        if succeeds {
            result.unwrap();
            current.final_recheck_with(&mut hooks).unwrap();
            let history = current.into_history().unwrap();
            assert_eq!(history.ordered_operations().len(), 255);
            assert_eq!(history.head_ids().len(), 2);
        } else {
            let error = require_error(result);
            assert!(error.to_string().contains("limit"), "{error}");
            require_error(current.into_history());
        }
        // Two seeded pairs leave exactly254 additional read slots in both cases.
        assert_eq!(
            hooks
                .events
                .iter()
                .filter(|event| matches!(event, HistoryEvent::Before(_)))
                .count(),
            254
        );
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
}

#[test]
fn history_raw_union_exact_and_one_over_includes_filtered_baseline_bytes() {
    for sampled_baseline in [false, true] {
        for over in [false, true] {
            let records = if sampled_baseline {
                fixtures::raw_with_baseline(over)
            } else {
                fixtures::raw_chain(over)
            };
            let anchor = fixtures::merge();
            let union = fixtures::raw_size(&records)
                + if sampled_baseline {
                    fixtures::raw_size(std::slice::from_ref(&anchor))
                } else {
                    0
                };
            assert_eq!(union, 8 * 1024 * 1024 + usize::from(over));
            assert_eq!(records.len() + usize::from(sampled_baseline), 256);
            let (fixture, baseline) = fixture(&records);
            if sampled_baseline {
                fixture.set_heads(&[&anchor.operation_id, &records.last().unwrap().operation_id]);
            }
            let mut budget = HistoryCaptureBudget::new(deadline());
            let mut current = budget.open(&fixture.context()).unwrap();
            canonical_policy(&mut current);
            let result = current.collect_history(&baseline);
            if over {
                let error = require_error(result);
                assert!(error.to_string().contains("byte"), "{error}");
                require_error(current.into_history());
            } else {
                result.unwrap();
                current.final_recheck().unwrap();
                let history = current.into_history().unwrap();
                assert_eq!(history.ordered_operations().len(), records.len());
            }
            assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
        }
    }
}

#[test]
fn history_uses_one_finite_budget_through_final_seal_read() {
    let (fixture, baseline) = fixture(&fixtures::chain(4));
    let mut control = HistoryCaptureBudget::new(deadline());
    assert_eq!(remaining(&control), (10 * 1024 * 1024, 640));
    let mut current = control.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    current.collect_history(&baseline).unwrap();
    current.final_recheck().unwrap();
    drop(current.into_history().unwrap());
    let left = remaining(&control);
    let used = (10 * 1024 * 1024 - left.0, 640 - left.1);
    for (bytes, attempts, succeeds) in [
        (used.0, used.1, true),
        (used.0 - 1, used.1, false),
        (used.0, used.1 - 1, false),
    ] {
        let until = deadline();
        let mut budget = HistoryCaptureBudget::new(until);
        budget.budget.metadata = MetadataReadBudget::new(bytes, attempts, until);
        let mut current = budget.open(&fixture.context()).unwrap();
        canonical_policy(&mut current);
        current.collect_history(&baseline).unwrap();
        if succeeds {
            current.final_recheck().unwrap();
            drop(current.into_history().unwrap());
            assert_eq!(remaining(&budget), (0, 0));
        } else {
            require_error(current.final_recheck());
            require_error(current.into_history());
            assert!(remaining(&budget).0 < bytes);
            assert!(remaining(&budget).1 < attempts);
        }
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
}

#[test]
fn history_failed_read_stays_charged_and_cannot_be_retried_in_place() {
    let records = vec![fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    let before = current.read_remaining();
    fs::remove_file(operation_path(&fixture, &records[0].operation_id)).unwrap();
    require_error(current.collect_history_with(&baseline, &mut hooks));
    let after = current.read_remaining();
    assert_eq!(before.1 - after.1, 1);
    assert_eq!(before.0, after.0);
    require_error(current.collect_history_with(&baseline, &mut hooks));
    assert_eq!(current.read_remaining(), after);
    assert_eq!(
        hooks
            .events
            .iter()
            .filter(|event| matches!(event, HistoryEvent::Before(_)))
            .count(),
        1
    );
    drop(current);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_private_deadline_stops_before_next_ancestor_read() {
    let (fixture, baseline) = fixture(&fixtures::chain(3));
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    let before = current.read_remaining();
    hooks.expire_at = Some(HistoryPhase::BeforeWalk);
    require_error(current.collect_history_with(&baseline, &mut hooks));
    assert_eq!(current.read_remaining(), before);
    assert!(
        !hooks
            .events
            .iter()
            .any(|event| matches!(event, HistoryEvent::Before(_)))
    );
    drop(current);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_zero_directory_capacity_refuses_before_an_open_attempt() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let limits = CaptureLimits {
        live_directory_descriptors: 0,
        ..CaptureLimits::default()
    };
    let mut budget = HistoryCaptureBudget::with_limits(deadline(), limits);
    let before = remaining(&budget);
    require_error(budget.open(&context));
    assert_eq!(remaining(&budget), before);
    assert_eq!(budget.budget.counters().directory_open_attempts, 0);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_expiry_after_complete_proof_is_not_successful_collection() {
    let (fixture, baseline) = fixture(&fixtures::chain(2));
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    hooks.expire_at = Some(HistoryPhase::EvidenceComplete);
    require_error(current.collect_history_with(&baseline, &mut hooks));
    assert!(
        hooks
            .events
            .contains(&HistoryEvent::Phase(HistoryPhase::EvidenceComplete))
    );
    // A later real-clock final check cannot turn the failed collection into success.
    current.final_recheck().unwrap();
    require_error(current.into_history());
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_corrupt_requested_bytes_remain_charged_after_native_rejection() {
    let records = vec![fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    let before = current.read_remaining();
    fs::write(
        operation_path(&fixture, &records[0].operation_id),
        b"invalid",
    )
    .unwrap();
    require_error(current.collect_history(&baseline));
    let after = current.read_remaining();
    assert_eq!(before.0 - after.0, b"invalid".len() + 1);
    assert_eq!(before.1 - after.1, 1);
    require_error(current.into_history());
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}
