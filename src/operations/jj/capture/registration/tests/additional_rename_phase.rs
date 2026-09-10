use super::*;
use crate::operations::jj::capture::budget::CaptureHooks;
use crate::operations::jj::capture::registration::seal::{PublicationPhase, SealHooks};
use std::ffi::CStr;
use std::io;
use std::os::fd::RawFd;

struct ExpireAfterRename {
    now: Instant,
    phases: Vec<PublicationPhase>,
    renames: usize,
}

impl CaptureHooks for ExpireAfterRename {
    fn now(&mut self) -> Instant {
        self.now
    }
}

impl SealHooks for ExpireAfterRename {
    fn publication_phase(&mut self, phase: PublicationPhase) {
        self.phases.push(phase);
    }

    fn rename(&mut self, parent: RawFd, source: &CStr, destination: &CStr) -> io::Result<()> {
        self.renames += 1;
        crate::unix_publication::rename_no_replace(parent, source, destination)?;
        self.now += Duration::from_secs(120);
        Ok(())
    }
}

#[test]
fn successful_rename_is_published_before_its_postcall_deadline_error() {
    let fixture = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let mut hooks = ExpireAfterRename {
        now: Instant::now(),
        phases: Vec::new(),
        renames: 0,
    };
    let error = require_error(initial.publish_new_with(SOURCE, &mut hooks));
    assert!(error.to_string().contains("deadline"), "{error}");
    assert_eq!(hooks.renames, 1);
    let namespace = occupied_namespace(&fixture);
    assert_eq!(
        fs::read(namespace.join("registration")).unwrap(),
        expected_seal(SOURCE)
    );
    assert!(!namespace.join(".registration.tmp").exists());
    assert_eq!(budget.session_counters(0).live_directory_descriptors, 0);
    assert!(hooks.phases.contains(&PublicationPhase::SealPublished));
    assert!(!hooks.phases.contains(&PublicationPhase::NamespaceSynced));
    assert!(!hooks.phases.contains(&PublicationPhase::RepositorySynced));
}
