use super::*;

struct RestorePermissions {
    path: PathBuf,
    original: std::fs::Permissions,
}

impl Drop for RestorePermissions {
    fn drop(&mut self) {
        std::fs::set_permissions(&self.path, self.original.clone()).unwrap();
    }
}

#[test]
fn jj_observer_intent_refuses_sqlite_read_write_fallback_to_read_only() {
    let fixture = Fixture::new();
    replace(&fixture.path, &None, &record(1)).unwrap();
    let before = {
        let connection = fixture.connection();
        snapshot(&connection)
    };
    let original = std::fs::metadata(&fixture.path).unwrap().permissions();
    let _restore = RestorePermissions {
        path: fixture.path.clone(),
        original: original.clone(),
    };
    let mut protected = original;
    protected.set_readonly(true);
    std::fs::set_permissions(&fixture.path, protected).unwrap();
    let control =
        sqlite::open_with_flags_and_memory_limits(&fixture.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .unwrap();
    if !control.is_readonly(rusqlite::DatabaseName::Main).unwrap() {
        assert_eq!(
            unsafe { libc::geteuid() },
            0,
            "non-root fixture must exercise SQLite read-only fallback"
        );
        eprintln!("JJ_OBSERVER_INTENT_READONLY_FIXTURE_UNAVAILABLE: root retains write access");
        return;
    }
    assert_eq!(snapshot(&control), before);
    drop(control);
    assert!(replace(&fixture.path, &Some(record(1)), &record(2)).is_err());
    let readonly =
        sqlite::open_with_flags_and_memory_limits(&fixture.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(snapshot(&readonly), before);
}
