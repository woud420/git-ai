use super::*;
use crate::operations::jj::capture::budget::{CaptureHooks, CapturePhase};
use crate::operations::jj::capture::registration::seal::{PublicationPhase, SealHooks, SealSample};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;

pub(super) const SOURCE: &str = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
pub(super) const ATTACHMENT: &str =
    "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

pub(super) fn expected_seal(source: &str) -> Vec<u8> {
    format!(
        "git-ai/jj/source-seal/v1\nsource_id={source}\nreader_profile=jj-simple-op-store/0.45.1\n"
    )
    .into_bytes()
}

pub(super) fn canonical_policy(session: &mut RetainedCapture<'_>) {
    let paths = session.policy_paths();
    let canonical = SampledPolicyPaths {
        workspace_root: paths.workspace_root.canonicalize().unwrap(),
        git_dir: paths.git_dir.canonicalize().unwrap(),
        git_common_dir: paths.git_common_dir.canonicalize().unwrap(),
    };
    session.validate_policy_paths(canonical).unwrap();
}

pub(super) fn create_seal(fixture: &Fixture) {
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    drop(initial.publish_new(SOURCE).unwrap());
}

pub(super) fn require_error<T>(result: Result<T, JjCaptureError>) -> JjCaptureError {
    match result {
        Ok(_) => panic!("accepted invalid retained capture or seal publication"),
        Err(error) => error,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Capture(CapturePhase),
    Publication(PublicationPhase),
    Sample(SealSample),
}

pub(super) struct Hooks<F> {
    pub events: Vec<Event>,
    callback: F,
    pub writes: usize,
    pub syncs: usize,
    pub write_interrupts: usize,
    pub sync_interrupts: usize,
    pub zero_write: bool,
    pub fail_sync: Option<usize>,
    pub chunk: Option<usize>,
    pub now: Instant,
    pub expire_at: Option<Event>,
    pub fchmods: usize,
    pub interrupt_fchmod: bool,
    pub acl_checks: usize,
}

impl<F: FnMut(Event)> Hooks<F> {
    pub fn new(callback: F) -> Self {
        Self {
            events: Vec::new(),
            callback,
            writes: 0,
            syncs: 0,
            write_interrupts: 0,
            sync_interrupts: 0,
            zero_write: false,
            fail_sync: None,
            chunk: None,
            now: Instant::now(),
            expire_at: None,
            fchmods: 0,
            interrupt_fchmod: false,
            acl_checks: 0,
        }
    }

    fn event(&mut self, event: Event) {
        self.events.push(event);
        (self.callback)(event);
        if self.expire_at == Some(event) {
            self.now += Duration::from_secs(120);
        }
    }
}

impl<F: FnMut(Event)> CaptureHooks for Hooks<F> {
    fn phase(&mut self, phase: CapturePhase) {
        self.event(Event::Capture(phase));
    }

    fn now(&mut self) -> Instant {
        self.now
    }
}

impl<F: FnMut(Event)> SealHooks for Hooks<F> {
    fn publication_phase(&mut self, phase: PublicationPhase) {
        self.event(Event::Publication(phase));
    }

    fn seal_sample(&mut self, phase: SealSample) {
        self.event(Event::Sample(phase));
    }

    fn fchmod(&mut self, file: &File) -> io::Result<()> {
        self.fchmods += 1;
        if self.interrupt_fchmod {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn check_acl(&mut self, file: &File) -> io::Result<()> {
        self.acl_checks += 1;
        crate::unix_publication::reject_any_acl(file)
    }

    fn write(&mut self, file: &File, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        if self.writes <= self.write_interrupts {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.zero_write {
            return Ok(0);
        }
        let length = self.chunk.unwrap_or(bytes.len()).min(bytes.len());
        crate::unix_publication::write_once(file, &bytes[..length])
    }

    fn sync(&mut self, file: &File) -> io::Result<()> {
        self.syncs += 1;
        if self.syncs <= self.sync_interrupts {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.fail_sync == Some(self.syncs) {
            return Err(io::Error::other("injected single sync failure"));
        }
        crate::unix_publication::sync_once(file)
    }
}

pub(super) fn no_hooks() -> Hooks<impl FnMut(Event)> {
    Hooks::new(|_| {})
}

pub(super) fn occupied_namespace(fixture: &Fixture) -> std::path::PathBuf {
    fixture.repo.join("git-ai")
}
