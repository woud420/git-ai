use super::*;

#[test]
fn retained_reader_returns_the_exact_opened_descriptor_and_charged_bytes() {
    struct Observe<'a>(&'a Cell<RawFd>);
    impl FileRead for Observe<'_> {
        fn opened(&mut self, file: &File) {
            self.0.set(file.as_raw_fd());
        }
    }
    let fixture = Fixture::new();
    let mut remaining = budget();
    let descriptor = Cell::new(-1);
    let (file, bytes) = read_retained_with(
        &fixture.directory,
        OsStr::new("record"),
        32,
        &mut remaining,
        &mut Observe(&descriptor),
    )
    .unwrap();
    assert_eq!(file.as_raw_fd(), descriptor.get());
    assert_eq!(bytes, b"original");
    assert_eq!(remaining.remaining_bytes(), 0);
    assert_eq!(remaining.remaining_file_attempts(), 1);
    let identity = file.metadata().unwrap();
    fs::rename(fixture.path(), fixture.path().with_extension("moved")).unwrap();
    fs::write(fixture.path(), b"new file").unwrap();
    let retained = file.metadata().unwrap();
    let replacement = fs::metadata(fixture.path()).unwrap();
    assert_eq!(
        (identity.dev(), identity.ino()),
        (retained.dev(), retained.ino())
    );
    assert_ne!(
        (retained.dev(), retained.ino()),
        (replacement.dev(), replacement.ino())
    );
}

#[test]
fn retained_public_facade_uses_the_same_one_read_budget() {
    let fixture = Fixture::new();
    let mut remaining = budget();
    let (file, bytes) = crate::regular_file::read_regular_at_retained(
        &fixture.directory,
        OsStr::new("record"),
        8,
        &mut remaining,
    )
    .unwrap();
    assert_eq!(bytes, b"original");
    assert!(file.metadata().unwrap().is_file());
    assert_eq!(remaining.remaining_bytes(), 0);
    assert_eq!(remaining.remaining_file_attempts(), 1);
}

#[test]
fn retained_and_owned_reads_preserve_mutation_errors_and_charges() {
    for mutation in [
        Mutation::Replace,
        Mutation::Grow,
        Mutation::Truncate,
        Mutation::Rewrite,
    ] {
        let mut results = Vec::new();
        for retain in [false, true] {
            let fixture = Fixture::new();
            let mut remaining = budget();
            let mut reader = AfterOpen {
                path: fixture.path(),
                mutation,
            };
            let result = if retain {
                read_retained_with(
                    &fixture.directory,
                    OsStr::new("record"),
                    32,
                    &mut remaining,
                    &mut reader,
                )
                .map(|(_file, bytes)| bytes)
            } else {
                fixture.read(&mut remaining, &mut reader)
            };
            let error = result.unwrap_err();
            assert!(matches!(error, E::Changed));
            results.push((
                error.to_string(),
                remaining.remaining_bytes(),
                remaining.remaining_file_attempts(),
            ));
        }
        assert_eq!(results[0], results[1]);
    }
}

// Invoked by the existing isolated child watchdog, so descriptor reuse by an
// unrelated test thread cannot invalidate the post-Drop EBADF assertions.
pub(super) fn isolated_descriptor_checks() {
    struct Failed<'a>(&'a Cell<RawFd>);
    impl FileRead for Failed<'_> {
        fn opened(&mut self, file: &File) {
            self.0.set(file.as_raw_fd());
        }
        fn read(&mut self, _: &mut File, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("retained read failure"))
        }
    }
    let fixture = Fixture::new();
    for _ in 0..16 {
        let descriptor = Cell::new(-1);
        let error = read_retained_with(
            &fixture.directory,
            OsStr::new("record"),
            32,
            &mut budget(),
            &mut Failed(&descriptor),
        )
        .unwrap_err();
        assert!(matches!(error, E::Io(_)));
        assert!(descriptor.get() >= 0);
        assert_eq!(unsafe { libc::fcntl(descriptor.get(), libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
        let (file, _) = read_retained_with(
            &fixture.directory,
            OsStr::new("record"),
            32,
            &mut budget(),
            &mut DirectRead,
        )
        .unwrap();
        let descriptor = file.as_raw_fd();
        drop(file);
        assert_eq!(unsafe { libc::fcntl(descriptor, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    }
}
