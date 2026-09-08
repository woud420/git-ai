use super::*;

#[test]
fn capture_default_limits_and_completed_descriptor_lifetime_are_explicit() {
    let limits = CaptureLimits::default();
    assert_eq!(limits.directory_components, 256);
    assert_eq!(limits.retained_edges, 256);
    assert_eq!(limits.directory_open_attempts, 256);
    assert_eq!(limits.live_directory_descriptors, 256);
    assert_eq!(limits.head_calls_per_pass, 36);
    assert_eq!(limits.head_calls_total, 72);
    assert_eq!(limits.retained_anchor_bytes, 8 * 1024 * 1024);

    let fixture = Fixture::new();
    let mut budget = CaptureBudget::new(deadline());
    assert_eq!(budget.metadata.remaining_bytes(), 10 * 1024 * 1024);
    assert_eq!(budget.metadata.remaining_file_attempts(), 192);
    let captured = attempt(&fixture.context(), &mut budget, &mut no_hooks()).unwrap();
    let counts = budget.counters();
    assert!(counts.directory_components > 0);
    assert!(counts.retained_edges > 0);
    assert!(counts.peak_live_directory_descriptors > 0);
    assert!(counts.directory_open_attempts > counts.retained_edges);
    assert_eq!(counts.head_calls, [4, 4]);
    assert_eq!(
        counts.retained_anchor_bytes,
        support::raw_size(&captured.anchors()[0])
    );
    // The returned capture remains alive while the descriptor count is zero.
    assert_eq!(captured.head_ids(), &[MERGE_ID.to_owned()]);
}

#[test]
fn capture_counts_explicit_parent_traversal_even_when_source_is_unchanged() {
    let fixture = Fixture::new();
    let mut original_budget = CaptureBudget::new(deadline());
    let original = attempt(&fixture.context(), &mut original_budget, &mut no_hooks()).unwrap();
    fs::create_dir(fixture.repo.join("store/walk")).unwrap();
    fs::write(
        fixture.repo.join("store/git_target"),
        "walk/../../../../.git",
    )
    .unwrap();
    let mut alias_budget = CaptureBudget::new(deadline());
    let alias = attempt(&fixture.context(), &mut alias_budget, &mut no_hooks()).unwrap();
    assert_eq!(original.source_binding(), alias.source_binding());
    let before = original_budget.counters();
    let after = alias_budget.counters();
    assert!(after.directory_components >= before.directory_components + 2);
    assert!(after.directory_open_attempts >= before.directory_open_attempts + 2);
}

#[test]
fn capture_enforces_each_directory_budget_and_releases_partial_binding() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut control_budget = CaptureBudget::new(deadline());
    attempt(&context, &mut control_budget, &mut no_hooks()).unwrap();
    let control = control_budget.counters();
    let limits = [
        CaptureLimits {
            directory_components: control.directory_components - 1,
            ..Default::default()
        },
        CaptureLimits {
            retained_edges: control.retained_edges - 1,
            ..Default::default()
        },
        CaptureLimits {
            directory_open_attempts: control.directory_open_attempts - 1,
            ..Default::default()
        },
        CaptureLimits {
            live_directory_descriptors: control.peak_live_directory_descriptors - 1,
            ..Default::default()
        },
    ];
    for limit in limits {
        let mut budget = CaptureBudget::with_limits(deadline(), limit);
        let error = require_error(attempt(&context, &mut budget, &mut no_hooks()));
        assert!(
            error.to_string().to_ascii_lowercase().contains("limit"),
            "{error}"
        );
    }

    for limit in [
        CaptureLimits {
            directory_open_attempts: 0,
            ..Default::default()
        },
        CaptureLimits {
            live_directory_descriptors: 0,
            ..Default::default()
        },
    ] {
        let mut budget = CaptureBudget::with_limits(deadline(), limit);
        assert!(attempt(&context, &mut budget, &mut no_hooks()).is_err());
        assert_eq!(budget.counters().directory_open_attempts, 0);
        assert_eq!(budget.counters().peak_live_directory_descriptors, 0);
    }

    // A refused component must not count as an attempted component open; opening
    // the root anchor before that refusal is permitted.
    let limit = CaptureLimits {
        directory_components: 0,
        ..Default::default()
    };
    let mut budget = CaptureBudget::with_limits(deadline(), limit);
    assert!(attempt(&context, &mut budget, &mut no_hooks()).is_err());
    assert_eq!(budget.counters().directory_components, 0);
    assert!(budget.counters().directory_open_attempts <= 1);

    fs::rename(
        fixture.repo.join("op_store"),
        fixture.repo.join("old-op-store"),
    )
    .unwrap();
    fs::write(fixture.repo.join("op_store"), []).unwrap();
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut no_hooks()).is_err());
    assert!(budget.counters().directory_open_attempts > 0);
}

#[test]
fn capture_head_call_limits_include_dots_eof_and_both_scans() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let cases = [(3, 72, [3, 0]), (36, 7, [4, 3])];
    for (per_pass, total, expected_calls) in cases {
        let limit = CaptureLimits {
            head_calls_per_pass: per_pass,
            head_calls_total: total,
            ..Default::default()
        };
        let mut budget = CaptureBudget::with_limits(deadline(), limit);
        assert!(attempt(&context, &mut budget, &mut no_hooks()).is_err());
        assert_eq!(budget.counters().head_calls, expected_calls);
    }
    let limit = CaptureLimits {
        head_calls_per_pass: 4,
        head_calls_total: 8,
        ..Default::default()
    };
    let mut budget = CaptureBudget::with_limits(deadline(), limit);
    attempt(&context, &mut budget, &mut no_hooks()).unwrap();
    assert_eq!(budget.counters().head_calls, [4, 4]);
}

#[test]
fn capture_physical_file_budget_is_shared_across_all_samples() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut control = CaptureBudget::new(deadline());
    let captured = attempt(&context, &mut control, &mut no_hooks()).unwrap();
    let consumed_bytes = 10 * 1024 * 1024 - control.metadata.remaining_bytes();
    let consumed_attempts = 192 - control.metadata.remaining_file_attempts();
    assert!(consumed_bytes > support::raw_size(&captured.anchors()[0]));
    assert!(consumed_attempts > 2);

    for (bytes, attempts, succeeds) in [
        (consumed_bytes, consumed_attempts, true),
        (consumed_bytes - 1, consumed_attempts, false),
        (consumed_bytes, consumed_attempts - 1, false),
    ] {
        let at = deadline();
        let mut budget = CaptureBudget::new(at);
        budget.metadata = MetadataReadBudget::new(bytes, attempts, at);
        let result = attempt(&context, &mut budget, &mut no_hooks());
        assert_eq!(result.is_ok(), succeeds);
        if succeeds {
            assert_eq!(budget.metadata.remaining_bytes(), 0);
            assert_eq!(budget.metadata.remaining_file_attempts(), 0);
        } else {
            assert!(budget.metadata.remaining_bytes() < bytes);
            assert!(budget.metadata.remaining_file_attempts() < attempts);
        }
    }
}

#[test]
fn capture_retained_anchor_budget_charges_each_shared_view_copy() {
    let fixture = Fixture::with_shared_view_heads();
    let context = fixture.context();
    let required = (LEFT_HEX.len() + RIGHT_HEX.len() + 2 * MINIMAL_HEX.len()) / 2;
    let limit = CaptureLimits {
        retained_anchor_bytes: required,
        ..Default::default()
    };
    let mut budget = CaptureBudget::with_limits(deadline(), limit);
    let captured = attempt(&context, &mut budget, &mut no_hooks()).unwrap();
    assert_eq!(captured.anchors().len(), 2);
    assert_eq!(
        captured
            .anchors()
            .iter()
            .map(support::raw_size)
            .sum::<usize>(),
        required
    );
    assert_eq!(budget.counters().retained_anchor_bytes, required);
    assert_eq!(captured.checkout_evidence().operation_id, MERGE_ID);
    assert!(
        captured
            .anchors()
            .iter()
            .all(|anchor| anchor.operation_id != MERGE_ID)
    );

    let limit = CaptureLimits {
        retained_anchor_bytes: required - 1,
        ..Default::default()
    };
    let mut budget = CaptureBudget::with_limits(deadline(), limit);
    assert!(attempt(&context, &mut budget, &mut no_hooks()).is_err());
    assert!(budget.counters().retained_anchor_bytes < required);
}

#[test]
fn capture_rechecks_one_absolute_deadline_after_each_phase() {
    let fixture = Fixture::new();
    let context = fixture.context();
    for phase in [
        CapturePhase::InitialSamplesRead,
        CapturePhase::EvidenceVerified,
        CapturePhase::FinalSamplesRead,
    ] {
        let at = deadline();
        let mut hooks = no_hooks();
        hooks.expire_at = Some((phase, at));
        let mut budget = CaptureBudget::new(at);
        let error = require_error(attempt(&context, &mut budget, &mut hooks));
        assert!(
            error.to_string().to_ascii_lowercase().contains("deadline"),
            "{error}"
        );
        assert_eq!(hooks.phases.last(), Some(&phase));
    }
}
