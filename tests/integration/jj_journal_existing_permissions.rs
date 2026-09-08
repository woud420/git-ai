use super::*;
use git_ai::model::repository::sqlite::open_with_flags_and_memory_limits;
use std::os::unix::fs::PermissionsExt;

struct RestorePermissions {
    path: PathBuf,
    original: fs::Permissions,
}

impl Drop for RestorePermissions {
    fn drop(&mut self) {
        fs::set_permissions(&self.path, self.original.clone())
            .expect("restore fixture permissions");
    }
}

#[test]
fn jj_journal_existing_refuses_sqlite_read_write_fallback_to_read_only() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let setup = wal_connection(&fixture);
    shape(&setup);
    let before = readonly_snapshot(&setup);
    drop(setup);
    let writable = JjObservationJournal::open_existing_at_path(&fixture.path).unwrap();
    fixture.assert_only_first(&writable);
    drop(writable);

    // Close every writable SQLite handle before chmod to prevent descriptor reuse.
    let restore = RestorePermissions {
        path: fixture.path.clone(),
        original: fs::metadata(&fixture.path).unwrap().permissions(),
    };
    let mut permissions = restore.original.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&fixture.path, permissions).unwrap();
    assert_eq!(
        fs::metadata(&fixture.path).unwrap().permissions().mode() & 0o222,
        0
    );
    let control = open_with_flags_and_memory_limits(
        &fixture.path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .unwrap();
    let actually_readonly = control.is_readonly(rusqlite::DatabaseName::Main).unwrap();
    assert_eq!(readonly_snapshot(&control), before);
    let mode: String = control
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    drop(control);
    if !actually_readonly && unsafe { libc::geteuid() } == 0 {
        eprintln!(
            "JJ_EXISTING_READONLY_FIXTURE_UNAVAILABLE: effective uid 0 bypassed the file write-permission denial"
        );
        return;
    }
    assert!(
        actually_readonly,
        "SQLite READ_WRITE control must fall back to a read-only Main database"
    );

    let result = JjObservationJournal::open_existing_at_path(&fixture.path);
    let refused = result.is_err();
    drop(result);
    let after = open_with_flags_and_memory_limits(
        &fixture.path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(readonly_snapshot(&after), before);
    drop(after);
    assert!(
        refused,
        "existing writable opener accepted SQLite's read-only fallback"
    );
    drop(restore);
}
