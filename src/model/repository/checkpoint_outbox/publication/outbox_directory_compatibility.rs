use super::*;
use std::collections::BTreeSet;

#[test]
fn directory_entries_keep_outbox_dot_filter_and_unfiltered_other_names() {
    let directory = tempfile::tempdir().unwrap();
    for name in ["example.ready", ".partial.tmp", "unexpected", "lock"] {
        std::fs::write(directory.path().join(name), []).unwrap();
    }
    let parent = std::fs::File::open(directory.path()).unwrap();
    let mut entries = DirectoryEntries::open(parent.as_raw_fd()).unwrap();
    let mut names = BTreeSet::new();
    for _ in 0..5 {
        match entries.next_name().unwrap() {
            Some(name) => assert!(names.insert(name.into_string().unwrap())),
            None => break,
        }
    }
    assert_eq!(
        names,
        ["example.ready", ".partial.tmp", "unexpected", "lock"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert!(entries.next_name().unwrap().is_none());
}

#[test]
fn directory_entries_keep_outbox_scan_root_error_mapping() {
    let direct = match DirectoryEntries::open(-1) {
        Ok(_) => panic!("invalid directory descriptor was accepted"),
        Err(error) => error,
    };
    let expected = E::from_io("scan root", std::io::Error::from_raw_os_error(libc::EBADF));
    assert_eq!(direct.to_string(), expected.to_string());
}
