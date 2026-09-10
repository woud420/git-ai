use super::*;
use std::path::Path;
use std::process::{Command, Stdio};

fn chmod(arguments: &[&str], path: &Path) {
    let mut child = Command::new("/bin/chmod")
        .args(arguments)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let limit = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "test-owned ACL setup failed");
            return;
        }
        if Instant::now() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("test-owned ACL setup exceeded its watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn any_namespace_or_seal_acl_rejects_without_clearing_it() {
    for target in ["namespace", "leaf"] {
        for entry in ["everyone allow readattr", "everyone deny writeattr"] {
            let fixture = Fixture::new();
            create_seal(&fixture);
            let namespace = occupied_namespace(&fixture);
            let path = if target == "namespace" {
                namespace
            } else {
                namespace.join("registration")
            };
            chmod(&["+a", entry], &path);
            let before = fs::metadata(&path).unwrap();
            for _ in 0..2 {
                let mut budget = RegistrationCaptureBudget::new(deadline());
                require_error(budget.open_initial(&fixture.context()));
            }
            let after = fs::metadata(&path).unwrap();
            assert_eq!(
                (before.ctime(), before.ctime_nsec()),
                (after.ctime(), after.ctime_nsec())
            );
            // Cleanup belongs only to this test's fixture and is never a
            // registration recovery or publisher behavior.
            chmod(&["-N"], &path);
        }
    }
}

#[test]
fn source_ancestor_acl_is_outside_the_seal_ownership_policy() {
    let fixture = Fixture::new();
    chmod(&["+a", "everyone allow readattr"], &fixture.repo);
    create_seal(&fixture);
    assert_eq!(
        fs::read(occupied_namespace(&fixture).join("registration")).unwrap(),
        expected_seal(SOURCE)
    );
    chmod(&["-N"], &fixture.repo);
}
