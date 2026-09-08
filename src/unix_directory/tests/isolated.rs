use super::*;
use std::os::fd::RawFd;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn descriptor_cleanup_and_fifo_refusal_have_a_bounded_child_watchdog() {
    let test_name = concat!(module_path!(), "::isolated_directory_descriptor_child");
    let test_name = test_name.split_once("::").unwrap().1;
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            test_name,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "isolated directory checks failed: {status}"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("directory descriptor test exceeded its watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_closed(descriptor: RawFd) {
    assert!(descriptor >= 0);
    assert_eq!(unsafe { libc::fcntl(descriptor, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
}

#[test]
#[ignore = "child helper invoked by the directory watchdog test"]
fn isolated_directory_descriptor_child() {
    let fixture = Fixture::new();
    let name = CString::new("fifo").unwrap();
    assert_eq!(
        unsafe { libc::mkfifoat(fixture.directory.as_raw_fd(), name.as_ptr(), 0o600) },
        0
    );
    assert!(open_directory_at(fixture.directory.as_raw_fd(), &name).is_err());
    let socket = fixture.root.path().join("socket");
    let _socket = std::os::unix::net::UnixListener::bind(socket).unwrap();
    assert!(open_directory_at(fixture.directory.as_raw_fd(), c"socket").is_err());
    for _ in 0..128 {
        let stream = fixture.stream();
        let descriptor = unsafe { libc::dirfd(stream.stream) };
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
        drop(stream);
        assert_closed(descriptor);

        let failed_descriptor = Cell::new(-1);
        let result = DirectoryEntries::open_with(fixture.directory.as_raw_fd(), |descriptor| {
            failed_descriptor.set(descriptor);
            set_errno(libc::ENOMEM);
            std::ptr::null_mut()
        });
        let error = match result {
            Ok(_) => panic!("injected fdopendir failure was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.raw_os_error(), Some(libc::ENOMEM));
        assert_closed(failed_descriptor.get());
        assert!(fixture.directory.metadata().unwrap().is_dir());
    }
}
