use super::*;
use crate::operations::jj::capture::registration::seal::{PublicationPhase, SealSample};

#[test]
fn publisher_creates_only_the_closed_seal_with_private_modes() {
    let fixture = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    assert!(initial.seal().is_none());
    canonical_policy(&mut initial);
    let mut hooks = no_hooks();
    let token = initial.publish_new_with(SOURCE, &mut hooks).unwrap();
    let namespace = occupied_namespace(&fixture);
    let path = namespace.join("registration");
    assert_eq!(fs::read(&path).unwrap(), expected_seal(SOURCE));
    let directory = fs::symlink_metadata(&namespace).unwrap();
    let leaf = fs::symlink_metadata(&path).unwrap();
    assert!(directory.is_dir());
    assert!(leaf.is_file());
    assert_eq!(directory.mode() & 0o7777, 0o700);
    assert_eq!(leaf.mode() & 0o7777, 0o600);
    assert_eq!(leaf.nlink(), 1);
    assert_eq!(leaf.uid(), unsafe { libc::geteuid() });
    assert_eq!(directory.uid(), leaf.uid());
    assert_eq!(hooks.syncs, 3);
    assert_eq!(hooks.writes, 1);
    assert_eq!(hooks.acl_checks, 4);
    assert_eq!(
        hooks.events,
        vec![
            Event::Publication(PublicationPhase::BeforeMutation),
            Event::Publication(PublicationPhase::NamespaceCreated),
            Event::Publication(PublicationPhase::TemporaryCreated),
            Event::Publication(PublicationPhase::LeafWritten),
            Event::Publication(PublicationPhase::LeafSynced),
            Event::Publication(PublicationPhase::SealPublished),
            Event::Publication(PublicationPhase::NamespaceSynced),
            Event::Publication(PublicationPhase::RepositorySynced),
            Event::Sample(SealSample::Publication),
        ]
    );
    assert_eq!(fs::read_dir(namespace).unwrap().count(), 1);
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    drop(token);
}

#[test]
fn occupied_namespace_is_never_adopted_or_cleaned() {
    for kind in ["empty", "temporary", "regular", "symlink", "malformed"] {
        let fixture = Fixture::new();
        let namespace = occupied_namespace(&fixture);
        match kind {
            "regular" => fs::write(&namespace, b"existing").unwrap(),
            "symlink" => symlink(&fixture.root, &namespace).unwrap(),
            _ => {
                fs::create_dir(&namespace).unwrap();
                fs::set_permissions(&namespace, fs::Permissions::from_mode(0o700)).unwrap();
                if kind == "temporary" {
                    fs::write(namespace.join(".registration.tmp"), b"owned elsewhere").unwrap();
                } else if kind == "malformed" {
                    fs::write(namespace.join("registration"), b"incomplete").unwrap();
                }
            }
        }
        let before = fs::symlink_metadata(&namespace).unwrap();
        let mut budget = RegistrationCaptureBudget::new(deadline());
        require_error(budget.open_initial(&fixture.context()));
        let after = fs::symlink_metadata(&namespace).unwrap();
        assert_eq!(
            (before.dev(), before.ino(), before.mode()),
            (after.dev(), after.ino(), after.mode())
        );
        if kind == "temporary" {
            assert_eq!(
                fs::read(namespace.join(".registration.tmp")).unwrap(),
                b"owned elsewhere"
            );
        }
        assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    }
}

#[test]
fn interrupted_short_writes_use_the_finite_shared_cap() {
    for (interrupts, chunk, expected_calls, accepted) in [
        (115, 1, 256, true),
        (116, 1, 256, false),
        (256, 141, 256, false),
    ] {
        let fixture = Fixture::new();
        let mut budget = RegistrationCaptureBudget::new(deadline());
        let mut initial = budget.open_initial(&fixture.context()).unwrap();
        canonical_policy(&mut initial);
        let mut hooks = no_hooks();
        hooks.write_interrupts = interrupts;
        hooks.chunk = Some(chunk);
        let result = initial.publish_new_with(SOURCE, &mut hooks);
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(hooks.writes, expected_calls);
        let namespace = occupied_namespace(&fixture);
        assert_eq!(namespace.join("registration").exists(), accepted);
        assert_eq!(namespace.join(".registration.tmp").exists(), !accepted);
        if accepted {
            assert_eq!(
                fs::read(namespace.join("registration")).unwrap(),
                expected_seal(SOURCE)
            );
        }
    }
}

#[test]
fn zero_write_stops_and_retains_unpublished_state() {
    let fixture = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let mut hooks = no_hooks();
    hooks.zero_write = true;
    require_error(initial.publish_new_with(SOURCE, &mut hooks));
    assert_eq!(hooks.writes, 1);
    assert_eq!(hooks.syncs, 0);
    assert_eq!(
        fs::read(occupied_namespace(&fixture).join(".registration.tmp")).unwrap(),
        b""
    );
    let mut retry = RegistrationCaptureBudget::new(deadline());
    require_error(retry.open_initial(&fixture.context()));
}

#[test]
fn sync_interrupts_are_cumulative_across_all_three_stages() {
    for (interrupts, accepted) in [(5, true), (6, false), (8, false)] {
        let fixture = Fixture::new();
        let mut budget = RegistrationCaptureBudget::new(deadline());
        let mut initial = budget.open_initial(&fixture.context()).unwrap();
        canonical_policy(&mut initial);
        let mut hooks = no_hooks();
        hooks.sync_interrupts = interrupts;
        let result = initial.publish_new_with(SOURCE, &mut hooks);
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(hooks.syncs, 8);
        if interrupts == 6 {
            // Leaf and namespace sync succeeded, but the shared budget cannot
            // acknowledge parent durability; the published seal still remains.
            assert_eq!(
                fs::read(occupied_namespace(&fixture).join("registration")).unwrap(),
                expected_seal(SOURCE)
            );
        }
    }
}

#[test]
fn sync_failures_preserve_the_exact_publication_phase() {
    for stage in 1..=3 {
        let fixture = Fixture::new();
        let mut budget = RegistrationCaptureBudget::new(deadline());
        let mut initial = budget.open_initial(&fixture.context()).unwrap();
        canonical_policy(&mut initial);
        let mut hooks = no_hooks();
        hooks.fail_sync = Some(stage);
        require_error(initial.publish_new_with(SOURCE, &mut hooks));
        assert_eq!(hooks.syncs, stage);
        let namespace = occupied_namespace(&fixture);
        assert_eq!(namespace.join("registration").exists(), stage > 1);
        assert_eq!(namespace.join(".registration.tmp").exists(), stage == 1);
        assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    }
}

#[test]
fn no_replace_collision_never_acknowledges_or_deletes_another_seal() {
    let fixture = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let path = occupied_namespace(&fixture).join("registration");
    let mut hooks = Hooks::new(|event| {
        if event == Event::Publication(PublicationPhase::LeafSynced) {
            fs::write(&path, b"another publisher").unwrap();
        }
    });
    require_error(initial.publish_new_with(SOURCE, &mut hooks));
    assert_eq!(hooks.syncs, 1);
    assert_eq!(fs::read(&path).unwrap(), b"another publisher");
    assert!(
        occupied_namespace(&fixture)
            .join(".registration.tmp")
            .exists()
    );
}

#[test]
fn created_namespace_permissions_are_observed_without_path_repair() {
    let fixture = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let namespace = occupied_namespace(&fixture);
    let mut hooks = Hooks::new(|event| {
        if event == Event::Publication(PublicationPhase::NamespaceCreated) {
            fs::set_permissions(&namespace, fs::Permissions::from_mode(0o500)).unwrap();
        }
    });
    require_error(initial.publish_new_with(SOURCE, &mut hooks));
    assert_eq!(fs::metadata(&namespace).unwrap().mode() & 0o7777, 0o500);
    assert_eq!(hooks.writes, 0);
}

#[test]
fn interrupted_fchmod_is_one_attempt_without_writes_or_cleanup() {
    let fixture = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let mut hooks = no_hooks();
    hooks.interrupt_fchmod = true;
    require_error(initial.publish_new_with(SOURCE, &mut hooks));
    assert_eq!(hooks.fchmods, 1);
    assert_eq!(hooks.writes, 0);
    assert_eq!(hooks.syncs, 0);
    assert!(
        occupied_namespace(&fixture)
            .join(".registration.tmp")
            .exists()
    );
}

#[test]
fn publication_deadline_stops_before_the_next_mutation_and_never_unlinks() {
    for phase in [
        PublicationPhase::BeforeMutation,
        PublicationPhase::TemporaryCreated,
        PublicationPhase::SealPublished,
    ] {
        let fixture = Fixture::new();
        let mut budget = RegistrationCaptureBudget::new(deadline());
        let mut initial = budget.open_initial(&fixture.context()).unwrap();
        canonical_policy(&mut initial);
        let mut hooks = no_hooks();
        hooks.expire_at = Some(Event::Publication(phase));
        let error = require_error(initial.publish_new_with(SOURCE, &mut hooks));
        assert!(error.to_string().contains("deadline"));
        let namespace = occupied_namespace(&fixture);
        match phase {
            PublicationPhase::BeforeMutation => assert!(!namespace.exists()),
            PublicationPhase::TemporaryCreated => {
                assert!(namespace.join(".registration.tmp").exists());
                assert_eq!(hooks.writes, 0);
            }
            PublicationPhase::SealPublished => {
                assert_eq!(
                    fs::read(namespace.join("registration")).unwrap(),
                    expected_seal(SOURCE)
                );
                assert_eq!(hooks.syncs, 1);
            }
            _ => unreachable!(),
        }
    }
}
