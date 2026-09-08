use super::*;

#[test]
fn history_outside_checkout_enters_only_by_parent_reachability_and_is_read_again() {
    let records = vec![fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    set_checkout(&fixture, &records[0]);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = trace();
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    assert_eq!(
        current.captured().checkout().operation_id,
        records[0].operation_id
    );
    let before = current.read_remaining();
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    let after = current.read_remaining();
    assert_eq!(before.1 - after.1, 2);
    assert_eq!(
        before.0 - after.0,
        records[0].operation_bytes.len() + records[0].view_bytes.len() + 2
    );
    assert_eq!(
        hooks
            .events
            .iter()
            .filter(
                |event| matches!(event, HistoryEvent::Before(id) if id == &records[0].operation_id)
            )
            .count(),
        1
    );
    current.final_recheck_with(&mut hooks).unwrap();
    let history = current.into_history().unwrap();
    assert_eq!(
        ids(&history),
        records
            .iter()
            .map(|r| r.operation_id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        !baseline
            .receipt()
            .captured_head_ids()
            .contains(&records[0].operation_id)
    );

    // The diagnostic copy cannot silently serve as the separately budgeted union read.
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    fs::remove_file(operation_path(&fixture, &records[0].operation_id)).unwrap();
    require_error(current.collect_history(&baseline));
}

#[test]
fn history_unrelated_checkout_and_extra_store_files_do_not_seed_the_graph() {
    let records = fixtures::chain(2);
    let (fixture, baseline) = fixture(&records);
    let unrelated = fixtures::rich_parent();
    write_records(&fixture, std::slice::from_ref(&unrelated));
    set_checkout(&fixture, &unrelated);
    fs::write(
        operation_path(&fixture, &"ee".repeat(64)),
        b"malformed unrelated",
    )
    .unwrap();
    let history = collect(&fixture, &baseline);
    assert_eq!(
        ids(&history),
        records
            .iter()
            .map(|r| r.operation_id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(!ids(&history).contains(&unrelated.operation_id.as_str()));
}

#[test]
fn history_raw_head_and_checkout_advancement_after_capture_keeps_original_roots() {
    let records = vec![fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = Trace::new(|event| {
        if *event == HistoryEvent::Phase(HistoryPhase::BeforeWalk) {
            fixture.set_heads(&[&records[0].operation_id]);
            set_checkout(&fixture, &records[0]);
        }
    });
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    current.final_recheck_with(&mut hooks).unwrap();
    let history = current.into_history().unwrap();
    assert_eq!(
        history.head_ids(),
        std::slice::from_ref(&records[1].operation_id)
    );
    let captures: Vec<_> = hooks
        .inner
        .events
        .iter()
        .filter_map(|event| {
            if let Event::Capture(phase) = event {
                Some(*phase)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        captures,
        vec![
            CapturePhase::InitialSamplesRead,
            CapturePhase::EvidenceVerified,
            CapturePhase::FinalSamplesRead
        ]
    );
    let seals: Vec<_> = hooks
        .inner
        .events
        .iter()
        .filter_map(|event| {
            if let Event::Sample(phase) = event {
                Some(*phase)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(seals, vec![SealSample::Initial, SealSample::Final]);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_missing_demanded_operation_or_view_is_a_gap_without_partial_handoff() {
    for remove_view in [false, true] {
        let records = fixtures::chain(3);
        let (fixture, baseline) = fixture(&records);
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut current = budget.open(&fixture.context()).unwrap();
        canonical_policy(&mut current);
        let path = if remove_view {
            view_path(&fixture, &records[1].view_id)
        } else {
            operation_path(&fixture, &records[1].operation_id)
        };
        fs::remove_file(path).unwrap();
        require_error(current.collect_history(&baseline));
        require_error(current.into_history());
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
}

#[test]
fn history_gc_after_verified_ancestor_read_does_not_require_final_object_existence() {
    let records = vec![fixtures::rich_parent(), fixtures::rich_child()];
    let (fixture, baseline) = fixture(&records);
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut hooks = Trace::new(|event| {
        if *event == HistoryEvent::Verified(records[0].operation_id.clone()) {
            fs::remove_file(operation_path(&fixture, &records[0].operation_id)).unwrap();
            fs::remove_file(view_path(&fixture, &records[0].view_id)).unwrap();
        }
    });
    let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
    canonical_policy(&mut current);
    current.collect_history_with(&baseline, &mut hooks).unwrap();
    current.final_recheck_with(&mut hooks).unwrap();
    let history = current.into_history().unwrap();
    assert_eq!(history.ordered_operations(), records);
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
}

#[test]
fn history_retained_pointer_git_head_seal_and_ancestor_changes_refuse_final_handoff() {
    for fault in ["pointer", "git_head", "seal", "ancestor"] {
        let (fixture, baseline) = fixture(&fixtures::chain(2));
        let mut budget = HistoryCaptureBudget::new(deadline());
        let mut hooks = Trace::new(|event| {
            if *event != HistoryEvent::Phase(HistoryPhase::EvidenceComplete) {
                return;
            }
            match fault {
                "pointer" => {
                    fs::write(fixture.repo.join("store/git_target"), b"../../../.git/.").unwrap()
                }
                "git_head" => {
                    fs::write(fixture.root.join(".git/HEAD"), b"ref: refs/heads/changed\n").unwrap()
                }
                "seal" => {
                    let seal = fixture.repo.join("git-ai/registration");
                    let bytes = fs::read(&seal).unwrap();
                    fs::rename(&seal, seal.with_extension("original")).unwrap();
                    fs::write(&seal, bytes).unwrap();
                    fs::set_permissions(&seal, fs::Permissions::from_mode(0o600)).unwrap();
                }
                "ancestor" => {
                    fs::rename(&fixture.ancestor, fixture.ancestor.with_extension("moved")).unwrap()
                }
                _ => unreachable!(),
            }
        });
        let mut current = budget.open_with(&fixture.context(), &mut hooks).unwrap();
        canonical_policy(&mut current);
        current.collect_history_with(&baseline, &mut hooks).unwrap();
        require_error(current.final_recheck_with(&mut hooks));
        require_error(current.into_history());
        assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    }
}
