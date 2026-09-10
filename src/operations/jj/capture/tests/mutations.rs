use super::*;

#[test]
fn capture_rejects_persistent_head_change_after_initial_sample() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::InitialSamplesRead {
            fixture.set_heads(&[LEFT_ID]);
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut hooks).is_err());
    assert!(hooks.phases.contains(&CapturePhase::InitialSamplesRead));
}

#[test]
fn capture_rejects_persistent_checkout_change_after_native_join() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::EvidenceVerified {
            // Both names belong to this same authenticated view.
            fixture.write_checkout("workspace-工");
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut hooks).is_err());
    assert!(hooks.phases.contains(&CapturePhase::EvidenceVerified));
}

#[test]
fn capture_rejects_pointer_bytes_change_even_when_target_is_unchanged() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::InitialSamplesRead {
            fs::write(fixture.repo.join("store/git_target"), "./../../../.git").unwrap();
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut hooks).is_err());
    assert!(hooks.phases.contains(&CapturePhase::InitialSamplesRead));
}

#[test]
fn capture_rejects_optional_pointer_appearance_even_for_same_common_directory() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::InitialSamplesRead {
            fs::write(fixture.root.join(".git/commondir"), ".").unwrap();
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut hooks).is_err());
    assert!(hooks.phases.contains(&CapturePhase::InitialSamplesRead));
}

#[test]
fn capture_rejects_backend_change_after_native_verification() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::EvidenceVerified {
            fs::write(fixture.repo.join("op_store/type"), "unknown_store").unwrap();
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut hooks).is_err());
    assert!(hooks.phases.contains(&CapturePhase::EvidenceVerified));
}

#[test]
fn capture_rejects_replaced_heads_directory_with_identical_marker_names() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::EvidenceVerified {
            fs::rename(fixture.heads(), fixture.repo.join("op_heads/old-heads")).unwrap();
            fs::create_dir(fixture.heads()).unwrap();
            fs::write(fixture.heads().join(MERGE_ID), []).unwrap();
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut hooks).is_err());
    assert!(hooks.phases.contains(&CapturePhase::EvidenceVerified));
}

#[test]
fn capture_checks_retained_ancestor_edges_after_final_raw_samples() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::FinalSamplesRead {
            fs::rename(&fixture.ancestor, fixture.ancestor.with_extension("old")).unwrap();
            fs::create_dir(&fixture.ancestor).unwrap();
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    assert!(attempt(&context, &mut budget, &mut hooks).is_err());
    assert!(hooks.phases.contains(&CapturePhase::FinalSamplesRead));
}

#[test]
fn capture_ignores_new_literal_lock_but_counts_its_directory_entry() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut hooks = Hooks::new(|phase| {
        if phase == CapturePhase::InitialSamplesRead {
            fs::create_dir(fixture.heads().join("lock")).unwrap();
        }
    });
    let mut budget = CaptureBudget::new(deadline());
    let captured = attempt(&context, &mut budget, &mut hooks).unwrap();
    assert_eq!(captured.head_ids(), &[MERGE_ID.to_owned()]);
    assert_eq!(budget.counters().head_calls, [4, 5]);
}
