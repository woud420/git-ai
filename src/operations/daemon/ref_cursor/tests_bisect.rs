use super::tests_fixtures::*;
use super::*;
use crate::model::domain::BisectCheckout;

fn fixture(
    entries: &[(&str, &str, &str)],
) -> (tempfile::TempDir, RefCursor, NormalizedCommand, FamilyState) {
    let temp = tempfile::tempdir().unwrap();
    let git_dir = temp.path().join(".git");
    append_reflog(&git_dir, "HEAD", entries);
    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let mut state = family_state(&family);
    state.refs.insert("HEAD".to_string(), A.to_string());
    let mut cmd = command_with_worktree(
        &family,
        Some(temp.path().to_path_buf()),
        &["bisect", "good"],
    );
    cmd.trace_derived = true;
    cmd.finished_at_ns = 1_000;
    cmd.observed_child_commands = vec!["checkout".to_string()];
    cmd.bisect_checkout = Some(BisectCheckout {
        target: B.to_string(),
        started_at_ns: 100,
        finished_at_ns: 900,
    });
    (temp, RefCursor::new(family), cmd, state)
}

#[test]
fn bisect_cursor_uses_exact_child_target_and_replay_is_idempotent() {
    let message = format!("checkout: moving from main to {B}");
    let (_temp, mut cursor, mut cmd, state) = fixture(&[(A, B, &message)]);
    cursor.enrich_command(&mut cmd, &state).unwrap();
    assert_eq!(cmd.ref_changes, vec![ref_change("HEAD", A, B)]);
    cursor.enrich_command(&mut cmd, &state).unwrap();
    assert!(cmd.ref_changes.is_empty());
}

#[test]
fn bisect_cursor_rejects_ambiguous_same_second_target_matches() {
    let first = format!("checkout: moving from main to {B}");
    let second = format!("checkout: moving from {C} to {B}");
    let (_temp, mut cursor, mut cmd, state) = fixture(&[(A, B, &first), (C, B, &second)]);
    cursor.enrich_command(&mut cmd, &state).unwrap();
    assert!(cmd.ref_changes.is_empty());
}

#[test]
fn bisect_cursor_counts_noop_rows_before_deciding_ownership() {
    let message = format!("checkout: moving from main to {B}");
    let noop = format!("checkout: moving from {B} to {B}");
    let (_temp, mut cursor, mut cmd, state) = fixture(&[(A, B, &message), (B, B, &noop)]);
    cursor.enrich_command(&mut cmd, &state).unwrap();
    assert!(cmd.ref_changes.is_empty());
}

#[test]
fn bisect_cursor_rejects_incomplete_or_unowned_receipts() {
    for boundary in [
        "missing",
        "failed",
        "synthetic",
        "multiple_children",
        "time",
        "target",
        "oid",
    ] {
        let message = format!("checkout: moving from main to {B}");
        let (_temp, mut cursor, mut cmd, state) = fixture(&[(A, B, &message)]);
        match boundary {
            "missing" => cmd.bisect_checkout = None,
            "failed" => cmd.exit_code = 1,
            "synthetic" => cmd.trace_derived = false,
            "multiple_children" => cmd.observed_child_commands.push("checkout".to_string()),
            "time" => cmd.bisect_checkout.as_mut().unwrap().started_at_ns = 2_000,
            "target" => cmd.bisect_checkout.as_mut().unwrap().target = "other".to_string(),
            "oid" => cmd.bisect_checkout.as_mut().unwrap().target = C.to_string(),
            _ => unreachable!(),
        }
        cursor.enrich_command(&mut cmd, &state).unwrap();
        assert!(cmd.ref_changes.is_empty(), "{boundary}");
    }
}

#[test]
fn bisect_cursor_ignores_current_branch_target() {
    let (temp, mut cursor, mut cmd, state) =
        fixture(&[(A, B, "checkout: moving from main to feature")]);
    cmd.bisect_checkout.as_mut().unwrap().target = "feature".to_string();
    let reference = temp.path().join(".git/refs/heads/feature");
    fs::create_dir_all(reference.parent().unwrap()).unwrap();
    fs::write(reference, format!("{C}\n")).unwrap();
    cursor.enrich_command(&mut cmd, &state).unwrap();
    assert_eq!(cmd.ref_changes, vec![ref_change("HEAD", A, B)]);
}

#[test]
fn bisect_cursor_missing_or_oversize_log_does_not_move_evidence() {
    for oversized in [false, true] {
        let message = format!("checkout: moving from main to {B}");
        let (temp, mut cursor, mut cmd, state) = fixture(&[(A, B, &message)]);
        let path = temp.path().join(".git/logs/HEAD");
        if oversized {
            let mut log = fs::read(&path).unwrap();
            log.resize(1024 * 1024 + 1, b' ');
            fs::write(&path, log).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        cursor.enrich_command(&mut cmd, &state).unwrap();
        assert!(cmd.ref_changes.is_empty());
    }
}

#[test]
fn bisect_cursor_accepts_a_late_offset_without_using_later_head() {
    let message = format!("checkout: moving from main to {B}");
    let (temp, mut cursor, mut cmd, state) = fixture(&[(A, B, &message), (B, C, "commit: later")]);
    let git_dir = temp.path().join(".git");
    cmd.reflog_start_offsets.insert(
        head_key(&git_dir),
        fs::metadata(git_dir.join("logs/HEAD")).unwrap().len(),
    );
    fs::write(git_dir.join("HEAD"), format!("{C}\n")).unwrap();
    cursor.enrich_command(&mut cmd, &state).unwrap();
    assert_eq!(cmd.ref_changes, vec![ref_change("HEAD", A, B)]);
}

#[test]
fn bisect_cursor_unreadable_log_does_not_fail_the_family() {
    let message = format!("checkout: moving from main to {B}");
    let (temp, mut cursor, mut cmd, state) = fixture(&[(A, B, &message)]);
    let path = temp.path().join(".git/logs/HEAD");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    cursor.enrich_command(&mut cmd, &state).unwrap();
    assert!(cmd.ref_changes.is_empty());
}

#[test]
fn bisect_receipt_wire_field_is_optional() {
    let (_temp, _cursor, mut cmd, _state) = fixture(&[]);
    cmd.bisect_checkout = None;
    let value = serde_json::to_value(&cmd).unwrap();
    assert!(value.get("bisect_checkout").is_none());
    let decoded: NormalizedCommand = serde_json::from_value(value).unwrap();
    assert!(decoded.bisect_checkout.is_none());
    assert!(!decoded.trace_derived);
}
