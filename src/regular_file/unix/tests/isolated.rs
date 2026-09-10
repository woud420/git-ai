use super::*;
use std::process::{Command, Stdio};

#[test]
fn fifo_and_descriptor_lifecycle_have_a_bounded_child_watchdog() {
    let test_name = concat!(module_path!(), "::isolated_fifo_and_descriptor_child");
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
            assert!(status.success(), "isolated reader checks failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("FIFO/read descriptor test exceeded its watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct FifoSwap {
    path: PathBuf,
}

impl FileRead for FifoSwap {
    fn before_open(&mut self) {
        fs::remove_file(&self.path).unwrap();
        let path = CString::new(self.path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    }
}

struct FdFailure<'a> {
    descriptor: &'a Cell<RawFd>,
    expire: bool,
}

impl FileRead for FdFailure<'_> {
    fn opened(&mut self, file: &File) {
        self.descriptor.set(file.as_raw_fd());
    }

    fn read(&mut self, _: &mut File, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::Other))
    }

    fn after_read(&mut self, budget: &mut MetadataReadBudget) {
        if self.expire {
            budget.deadline = Instant::now();
        }
    }
}

#[test]
#[ignore = "child helper invoked by the bounded watchdog test"]
fn isolated_fifo_and_descriptor_child() {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    super::retained::isolated_descriptor_checks();
    let fixture = Fixture::new();
    let name = CString::new("fifo").unwrap();
    assert_eq!(
        unsafe { libc::mkfifoat(fixture.directory.as_raw_fd(), name.as_ptr(), 0o600) },
        0
    );
    assert!(matches!(
        read_with(
            &fixture.directory,
            OsStr::new("fifo"),
            32,
            &mut budget(),
            &mut DirectRead
        ),
        Err(E::NotRegular)
    ));
    let fifo = open_record_at(fixture.directory.as_raw_fd(), &name).unwrap();
    assert!(!fifo.metadata().unwrap().is_file());
    drop(fifo);
    assert!(matches!(
        fixture.read(
            &mut budget(),
            &mut FifoSwap {
                path: fixture.path()
            }
        ),
        Err(E::NotRegular)
    ));
    fs::remove_file(fixture.path()).unwrap();
    fs::write(fixture.path(), b"original").unwrap();
    let socket_path = fixture.root.path().join("socket");
    let _socket = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    assert!(matches!(
        read_with(
            &fixture.directory,
            OsStr::new("socket"),
            32,
            &mut budget(),
            &mut DirectRead
        ),
        Err(E::NotRegular)
    ));
    for attempt in 0..128 {
        let descriptor = Cell::new(-1);
        let mut reader = FdFailure {
            descriptor: &descriptor,
            expire: attempt % 2 == 0,
        };
        assert!(fixture.read(&mut budget(), &mut reader).is_err());
        assert!(descriptor.get() >= 0);
        assert_eq!(unsafe { libc::fcntl(descriptor.get(), libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    }
}
