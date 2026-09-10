use super::*;
use crate::operations::jj::capture::budget::{CaptureLimits, CapturePhase};
use crate::operations::jj::capture::registration::seal::SealSample;

#[test]
fn two_sessions_keep_the_created_inode_and_borrow_native_evidence() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut first = budget.open_initial(&context).unwrap();
    canonical_policy(&mut first);
    let source_binding = first.source_binding();
    let workspace_binding = first.workspace_binding();
    let locator = first.workspace_locator().unwrap();
    let created = first.publish_new(SOURCE).unwrap();
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    let mut hooks = no_hooks();
    let mut second = budget
        .open_created_with(&context, created, &mut hooks)
        .unwrap();
    canonical_policy(&mut second);
    assert!(second.source_binding() == source_binding);
    assert!(second.workspace_binding() == workspace_binding);
    assert!(second.workspace_locator().unwrap() == locator);
    let metadata = second.registration_metadata(ATTACHMENT).unwrap();
    assert_eq!(metadata.seal_bytes, expected_seal(SOURCE));
    assert!(metadata.source_binding == source_binding);
    assert_eq!(metadata.workspace.attachment_id, ATTACHMENT);
    assert_eq!(
        metadata.workspace.workspace_name,
        second.captured().checkout().workspace_name
    );
    assert_eq!(
        metadata.workspace.selected_checkout.operation_id,
        second.captured().checkout().operation_id
    );
    assert_eq!(
        metadata.workspace.selected_checkout.view_id,
        second.captured().checkout_evidence().view_id
    );
    assert_eq!(
        metadata.workspace.selected_checkout.raw_checkout_bytes.0,
        second.captured().checkout_bytes()
    );
    second.final_recheck_with(&mut hooks).unwrap();
    assert_eq!(hooks.acl_checks, 4);
    assert_eq!(
        hooks.events,
        vec![
            Event::Sample(SealSample::Initial),
            Event::Capture(CapturePhase::InitialSamplesRead),
            Event::Capture(CapturePhase::EvidenceVerified),
            Event::Capture(CapturePhase::FinalSamplesRead),
            Event::Sample(SealSample::Final),
        ]
    );
    let owned = second.into_captured();
    assert_eq!(owned.prepare_baseline().unwrap().anchors().len(), 1);
    assert_eq!(budget.session_counters(1).live_directory_descriptors, 0);
}

#[test]
fn existing_seal_reopen_has_two_bracket_reads_and_no_publication() {
    let fixture = Fixture::new();
    create_seal(&fixture);
    let path = occupied_namespace(&fixture).join("registration");
    let before = fs::metadata(&path).unwrap();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut hooks = no_hooks();
    let mut session = budget
        .open_initial_with(&fixture.context(), &mut hooks)
        .unwrap();
    canonical_policy(&mut session);
    assert_eq!(session.seal().unwrap().source_id(), SOURCE);
    session.final_recheck_with(&mut hooks).unwrap();
    let error = session.final_recheck_with(&mut hooks).unwrap_err();
    assert!(error.to_string().contains("already"));
    drop(session);
    assert_eq!(hooks.syncs, 0);
    assert_eq!(hooks.writes, 0);
    assert_eq!(hooks.acl_checks, 4);
    assert_eq!(
        hooks
            .events
            .iter()
            .filter(|event| matches!(event, Event::Sample(_)))
            .count(),
        2
    );
    let after = fs::metadata(path).unwrap();
    assert_eq!(
        (before.dev(), before.ino(), before.len()),
        (after.dev(), after.ino(), after.len())
    );
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    assert_eq!(budget.session_counters(1).directory_open_attempts, 0);
}

#[test]
fn new_publication_requires_validated_policy_paths() {
    let fixture = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let initial = budget.open_initial(&fixture.context()).unwrap();
    assert!(initial.workspace_locator().is_err());
    require_error(initial.publish_new(SOURCE));
    assert!(!occupied_namespace(&fixture).exists());
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
}

#[test]
fn canonical_policy_paths_must_reach_the_sampled_directories() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    let wrong = SampledPolicyPaths {
        workspace_root: other.root.clone(),
        git_dir: fixture.root.join(".git"),
        git_common_dir: fixture.root.join(".git"),
    };
    require_error(initial.validate_policy_paths(wrong));
    assert!(initial.workspace_locator().is_err());
    require_error(initial.publish_new(SOURCE));
    assert!(!occupied_namespace(&fixture).exists());
}

#[test]
fn current_heads_and_checkout_do_not_rewrite_the_saved_sample() {
    let fixture = Fixture::with_shared_view_heads();
    create_seal(&fixture);
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut session = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut session);
    let before_heads = session.captured().head_ids().to_vec();
    let before_checkout = session.captured().checkout_bytes().to_vec();
    let own = session.captured().checkout().operation_id.clone();
    fixture.set_heads(&[&own]);
    fs::write(
        fixture.root.join(".jj/working_copy/checkout"),
        b"later checkout bytes",
    )
    .unwrap();
    session.final_recheck().unwrap();
    assert_eq!(session.captured().head_ids(), before_heads);
    assert_eq!(session.captured().checkout_bytes(), before_checkout);
    assert_ne!(session.captured().head_ids(), &[own]);
}

#[test]
fn operational_pointer_changes_still_invalidate_final_recheck() {
    let fixture = Fixture::new();
    create_seal(&fixture);
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut session = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut session);
    fs::write(fixture.repo.join("store/git_target"), b"changed later").unwrap();
    assert!(session.final_recheck().is_err());
    drop(session);
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    assert_eq!(
        fs::read(occupied_namespace(&fixture).join("registration")).unwrap(),
        expected_seal(SOURCE)
    );
}

#[test]
fn ancestor_replacement_is_rejected_with_descriptors_still_open() {
    let fixture = Fixture::new();
    create_seal(&fixture);
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut session = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut session);
    let moved = fixture.root.with_extension("moved");
    fs::rename(&fixture.root, &moved).unwrap();
    fs::create_dir(&fixture.root).unwrap();
    assert!(session.final_recheck().is_err());
    drop(session);
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    assert_eq!(
        fs::read(moved.join(".jj/repo/git-ai/registration")).unwrap(),
        expected_seal(SOURCE)
    );
}

#[test]
fn created_token_cannot_be_used_for_another_source() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let created = initial.publish_new(SOURCE).unwrap();
    require_error(budget.open_created(&other.context(), created));
    assert!(!occupied_namespace(&other).exists());
    assert_eq!(budget.session_counters(1).live_directory_descriptors, 0);
}

#[test]
fn replacing_a_seal_with_identical_bytes_does_not_replace_its_identity() {
    for after_capture in [false, true] {
        let fixture = Fixture::new();
        let mut budget = RegistrationCaptureBudget::new(deadline());
        let mut initial = budget.open_initial(&fixture.context()).unwrap();
        canonical_policy(&mut initial);
        let created = initial.publish_new(SOURCE).unwrap();
        let path = occupied_namespace(&fixture).join("registration");
        let replace = || {
            fs::rename(&path, path.with_extension("saved")).unwrap();
            fs::write(&path, expected_seal(SOURCE)).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        };
        if after_capture {
            let mut session = budget.open_created(&fixture.context(), created).unwrap();
            canonical_policy(&mut session);
            replace();
            assert!(session.final_recheck().is_err());
        } else {
            replace();
            require_error(budget.open_created(&fixture.context(), created));
        }
        assert_eq!(fs::read(&path).unwrap(), expected_seal(SOURCE));
        assert_eq!(budget.session_counters(1).live_directory_descriptors, 0);
    }
}

#[test]
fn changed_bytes_modes_and_links_are_checked_on_final_seal_read() {
    for change in ["bytes", "mode", "link"] {
        let fixture = Fixture::new();
        create_seal(&fixture);
        let mut budget = RegistrationCaptureBudget::new(deadline());
        let mut session = budget.open_initial(&fixture.context()).unwrap();
        canonical_policy(&mut session);
        let path = occupied_namespace(&fixture).join("registration");
        match change {
            "bytes" => fs::write(&path, expected_seal(&"a".repeat(64))).unwrap(),
            "mode" => fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap(),
            "link" => fs::hard_link(&path, path.with_extension("linked")).unwrap(),
            _ => unreachable!(),
        }
        assert!(session.final_recheck().is_err());
        drop(session);
        assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    }
}

#[test]
fn namespace_headroom_is_refused_before_any_mutation() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut control = RegistrationCaptureBudget::new(deadline());
    let mut session = control.open_initial(&context).unwrap();
    canonical_policy(&mut session);
    drop(session);
    let used = control.session_counters(0);
    for limits in [
        CaptureLimits {
            directory_components: used.directory_components,
            ..Default::default()
        },
        CaptureLimits {
            retained_edges: used.retained_edges,
            ..Default::default()
        },
        CaptureLimits {
            directory_open_attempts: used.directory_open_attempts,
            ..Default::default()
        },
        CaptureLimits {
            live_directory_descriptors: used.peak_live_directory_descriptors,
            ..Default::default()
        },
    ] {
        let mut budget =
            RegistrationCaptureBudget::with_limits(deadline(), [limits, CaptureLimits::default()]);
        let mut initial = budget.open_initial(&context).unwrap();
        canonical_policy(&mut initial);
        let mut hooks = no_hooks();
        require_error(initial.publish_new_with(SOURCE, &mut hooks));
        assert!(!occupied_namespace(&fixture).exists());
        assert_eq!(hooks.writes, 0);
        assert_eq!(hooks.syncs, 0);
        assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    }
}

#[test]
fn retained_sessions_preserve_read_charges_and_cannot_restart_a_pass() {
    let fixture = Fixture::new();
    create_seal(&fixture);
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let before = initial.read_remaining();
    initial.final_recheck().unwrap();
    let after = initial.read_remaining();
    assert!(after.0 < before.0);
    assert!(after.1 < before.1);
    drop(initial);
    let counters = budget.session_counters(0);
    require_error(budget.open_initial(&fixture.context()));
    assert_eq!(
        budget.session_counters(0).directory_open_attempts,
        counters.directory_open_attempts
    );
}

#[test]
fn final_recheck_cannot_reset_an_exhausted_session_read_budget() {
    use crate::regular_file::MetadataReadBudget;
    let fixture = Fixture::new();
    create_seal(&fixture);
    let mut control = RegistrationCaptureBudget::new(deadline());
    let mut initial = control.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    initial.final_recheck().unwrap();
    let remaining = initial.read_remaining();
    let consumed_bytes = 10 * 1024 * 1024 - remaining.0;
    let consumed_attempts = 192 - remaining.1;
    drop(initial);
    for (bytes, attempts) in [
        (consumed_bytes - 1, 192),
        (10 * 1024 * 1024, consumed_attempts - 1),
    ] {
        let mut budget = RegistrationCaptureBudget::new(deadline());
        budget.sessions[0].metadata = MetadataReadBudget::new(bytes, attempts, deadline());
        let mut initial = budget.open_initial(&fixture.context()).unwrap();
        canonical_policy(&mut initial);
        let before = initial.read_remaining();
        assert!(initial.final_recheck().is_err());
        let after = initial.read_remaining();
        assert!(after.0 <= before.0);
        assert!(after.1 < before.1);
    }
}

#[test]
fn final_seal_deadline_keeps_prior_metadata_reads_charged() {
    let fixture = Fixture::new();
    create_seal(&fixture);
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut session = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut session);
    let before = session.read_remaining();
    let mut hooks = no_hooks();
    hooks.expire_at = Some(Event::Sample(SealSample::Final));
    let error = session.final_recheck_with(&mut hooks).unwrap_err();
    assert!(error.to_string().contains("deadline"));
    let after = session.read_remaining();
    assert!(after.0 < before.0);
    assert!(after.1 < before.1);
}

#[test]
fn absent_seal_reopen_rechecks_absence_without_a_seal_read() {
    for appears in [false, true] {
        let fixture = Fixture::new();
        let mut budget = RegistrationCaptureBudget::new(deadline());
        let mut hooks = no_hooks();
        let mut session = budget
            .open_initial_with(&fixture.context(), &mut hooks)
            .unwrap();
        canonical_policy(&mut session);
        assert!(session.seal().is_none());
        if appears {
            fs::create_dir(occupied_namespace(&fixture)).unwrap();
        }
        assert_eq!(session.final_recheck_with(&mut hooks).is_ok(), !appears);
        assert!(
            !hooks
                .events
                .iter()
                .any(|event| matches!(event, Event::Sample(_)))
        );
        assert_eq!(hooks.writes, 0);
        assert_eq!(hooks.syncs, 0);
        drop(session);
        assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
        assert_eq!(occupied_namespace(&fixture).exists(), appears);
    }
}
