use super::tests_fixtures::*;
use super::*;

#[test]
fn fetch_reflog_read_failure_remains_best_effort() {
    let dir = tempfile::tempdir().unwrap();
    let family = FamilyKey(dir.path().to_string_lossy().into_owned());
    fs::write(dir.path().join("logs"), "unreadable reflog directory").unwrap();
    let mut cmd = command(&family, &["fetch", "origin"]);
    let mut cursor = RefCursor::new(family.clone());
    cursor
        .enrich_command(&mut cmd, &family_state(&family))
        .unwrap();
    assert!(cmd.ref_changes.is_empty());
}

#[test]
fn fetch_can_identify_a_new_tracking_ref_without_an_existing_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let family = FamilyKey(dir.path().to_string_lossy().into_owned());
    let reference = "refs/remotes/origin/topic";
    append_reflog(
        dir.path(),
        reference,
        &[(&zero_oid(), B, "fetch origin: storing head")],
    );
    let mut cmd = command(&family, &["fetch", "origin"]);
    let mut cursor = RefCursor::new(family.clone());
    cursor
        .enrich_command(&mut cmd, &family_state(&family))
        .unwrap();
    assert_eq!(cmd.ref_changes, vec![ref_change(reference, &zero_oid(), B)]);
}

#[test]
fn fetch_without_a_cursor_does_not_claim_existing_ref_history() {
    let dir = tempfile::tempdir().unwrap();
    let family = FamilyKey(dir.path().to_string_lossy().into_owned());
    append_reflog(
        dir.path(),
        "refs/remotes/origin/main",
        &[(A, B, "fetch origin: fast-forward")],
    );
    let mut cmd = command(&family, &["fetch", "origin"]);
    let mut cursor = RefCursor::new(family.clone());
    cursor
        .enrich_command(&mut cmd, &family_state(&family))
        .unwrap();
    assert!(cmd.ref_changes.is_empty());
}

#[test]
fn fetch_records_the_cursored_incoming_oid_before_later_ref_updates() {
    let dir = tempfile::tempdir().unwrap();
    let family = FamilyKey(dir.path().to_string_lossy().into_owned());
    let reference = "refs/remotes/origin/main";
    append_reflog(
        dir.path(),
        reference,
        &[(A, B, "fetch origin: fast-forward")],
    );
    let mut cmd = command(&family, &["fetch", "origin"]);
    cmd.reflog_start_offsets.insert(common_key(reference), 0);
    let mut cursor = RefCursor::new(family.clone());
    cursor
        .enrich_command(&mut cmd, &family_state(&family))
        .unwrap();
    assert_eq!(cmd.ref_changes, vec![ref_change(reference, A, B)]);
    append_reflog(
        dir.path(),
        reference,
        &[(A, B, "fetch origin: fast-forward"), (B, C, "later update")],
    );
    assert_eq!(cmd.ref_changes, vec![ref_change(reference, A, B)]);
}

#[test]
fn fetch_ref_matching_fails_closed_for_same_window_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    let family = FamilyKey(dir.path().to_string_lossy().into_owned());
    let reference = "refs/remotes/origin/main";
    append_reflog(
        dir.path(),
        reference,
        &[
            (A, B, "fetch origin: fast-forward"),
            (B, C, "fetch origin: fast-forward"),
        ],
    );
    let mut cmd = command(&family, &["fetch", "origin"]);
    cmd.reflog_start_offsets.insert(common_key(reference), 0);
    let mut cursor = RefCursor::new(family.clone());
    cursor
        .enrich_command(&mut cmd, &family_state(&family))
        .unwrap();
    assert!(cmd.ref_changes.is_empty());
}

#[test]
fn fetch_ignores_old_other_remote_and_non_fetch_reflog_entries() {
    let dir = tempfile::tempdir().unwrap();
    let family = FamilyKey(dir.path().to_string_lossy().into_owned());
    append_reflog(
        dir.path(),
        "refs/remotes/origin/main",
        &[(A, B, "fetch origin: fast-forward")],
    );
    append_reflog(
        dir.path(),
        "refs/remotes/other/main",
        &[(A, C, "fetch origin: fast-forward")],
    );
    append_reflog(
        dir.path(),
        "refs/remotes/origin/unrelated",
        &[(A, C, "update-ref")],
    );
    let mut cmd = command(&family, &["fetch", "origin"]);
    cmd.started_at_ns = 10_000_000_000;
    cmd.finished_at_ns = 11_000_000_000;
    let mut cursor = RefCursor::new(family.clone());
    cursor
        .enrich_command(&mut cmd, &family_state(&family))
        .unwrap();
    assert!(cmd.ref_changes.is_empty());
}
