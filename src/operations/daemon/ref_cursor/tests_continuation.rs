use super::tests_fixtures::*;
use super::*;

fn enrich_continuation(
    operation: &str,
    rows: &[(&str, &str, &str)],
    cursor_boundary: bool,
    exit_code: i32,
    later_window: bool,
) -> Vec<RefChange> {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    append_reflog(&git_dir, "HEAD", &[(D, A, "commit: before continuation")]);
    let boundary = fs::metadata(git_dir.join("logs/HEAD")).unwrap().len();
    let mut all_rows = vec![(D, A, "commit: before continuation")];
    all_rows.extend_from_slice(rows);
    append_reflog(&git_dir, "HEAD", &all_rows);
    let family = FamilyKey::new(git_dir.to_string_lossy());
    let mut cursor = RefCursor::new(family.clone());
    if cursor_boundary {
        cursor
            .initialize_reflog_cursor(&head_key(&git_dir), boundary)
            .unwrap();
    }
    let mut state = family_state(&family);
    state.refs.insert("HEAD".into(), A.into());
    let mut cmd = command_with_worktree(&family, Some(worktree), &[operation, "--continue"]);
    cmd.exit_code = exit_code;
    if later_window {
        cmd.started_at_ns = 10_000_000_000;
        cmd.finished_at_ns = 11_000_000_000;
    }
    cursor.enrich_command(&mut cmd, &state).unwrap();
    cmd.ref_changes
}

#[test]
fn continuation_admits_one_cursored_merge_or_revert_commit() {
    for (operation, message) in [
        ("merge", "commit (merge): resolved"),
        ("revert", "commit: Revert source"),
    ] {
        assert_eq!(
            enrich_continuation(operation, &[(A, B, message)], true, 0, false),
            vec![ref_change("HEAD", A, B)]
        );
    }
}

#[test]
fn continuation_rejects_multiple_commit_rows_without_consuming_a_first_commit() {
    for (operation, message) in [
        ("merge", "commit (merge): resolved"),
        ("revert", "commit: Revert source"),
    ] {
        assert!(
            enrich_continuation(
                operation,
                &[(A, B, message), (B, C, message)],
                true,
                0,
                false
            )
            .is_empty()
        );
    }
}

#[test]
fn continuation_requires_cursor_success_and_operation_time_evidence() {
    let rows = [(A, B, "commit: Revert source")];
    for (boundary, exit_code, later) in [(false, 0, false), (true, 1, false), (true, 0, true)] {
        assert!(enrich_continuation("revert", &rows, boundary, exit_code, later).is_empty());
    }
}

#[test]
fn merge_continue_does_not_claim_a_plain_commit_or_a_different_old_head() {
    assert!(
        enrich_continuation("merge", &[(A, B, "commit: unrelated")], true, 0, false).is_empty()
    );
    assert!(
        enrich_continuation(
            "merge",
            &[(C, B, "commit (merge): unrelated")],
            true,
            0,
            false
        )
        .is_empty()
    );
}
