use super::*;
use crate::unix_directory::open_directory_at;
use std::error::Error;
use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::os::fd::RawFd;

#[derive(Default)]
struct FailOneOpen {
    actual_calls: usize,
    successful_calls: usize,
    injected_calls: usize,
}

impl CaptureHooks for FailOneOpen {
    fn open_directory(&mut self, parent: RawFd, name: &CStr) -> io::Result<File> {
        self.actual_calls += 1;
        let parent = if name.to_bytes() == b"op_store" {
            self.injected_calls += 1;
            -1
        } else {
            parent
        };
        // This still executes the real primitive once. The relative name makes
        // the invalid descriptor produce EBADF after the caller's valid stat.
        let result = open_directory_at(parent, name);
        self.successful_calls += usize::from(result.is_ok());
        result
    }
}

#[test]
fn capture_keeps_actual_failed_open_charge_and_releases_live_descriptors() {
    let fixture = Fixture::new();
    let context = fixture.context();
    assert!(fixture.repo.join("op_store").is_dir());
    let mut hooks = FailOneOpen::default();
    let mut budget = CaptureBudget::new(deadline());
    let error = require_error(attempt(&context, &mut budget, &mut hooks));
    assert_eq!(hooks.injected_calls, 1);
    assert!(hooks.successful_calls > 0);
    assert_eq!(hooks.actual_calls, hooks.successful_calls + 1);
    assert_eq!(
        budget.counters().directory_open_attempts,
        hooks.actual_calls
    );
    assert_eq!(budget.counters().live_directory_descriptors, 0);

    let mut source: &dyn Error = &error;
    loop {
        if let Some(io) = source.downcast_ref::<io::Error>() {
            assert_eq!(io.raw_os_error(), Some(libc::EBADF));
            break;
        }
        source = source
            .source()
            .expect("failed open must retain its I/O cause");
    }
}
