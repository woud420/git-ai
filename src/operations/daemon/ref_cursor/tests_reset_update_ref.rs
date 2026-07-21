use super::*;
use crate::model::domain::{
    CommandScope, Confidence, FamilyKey, FamilyState, NormalizedCommand, WatermarkState,
    WorktreeState,
};
use std::collections::HashMap;
use std::fs;

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const C: &str = "3333333333333333333333333333333333333333";
const D: &str = "4444444444444444444444444444444444444444";

fn family_state(family: &FamilyKey) -> FamilyState {
    FamilyState {
        family_key: family.clone(),
        refs: HashMap::new(),
        worktrees: HashMap::new(),
        last_error: None,
        applied_seq: 0,
        watermarks: WatermarkState::default(),
    }
}

fn command(family: &FamilyKey, args: &[&str]) -> NormalizedCommand {
    command_with_worktree(family, None, args)
}

fn command_with_worktree(
    family: &FamilyKey,
    worktree: Option<PathBuf>,
    args: &[&str],
) -> NormalizedCommand {
    NormalizedCommand {
        scope: CommandScope::Family(family.clone()),
        family_key: Some(family.clone()),
        worktree,
        root_sid: "sid".to_string(),
        raw_argv: std::iter::once("git".to_string())
            .chain(args.iter().map(|arg| arg.to_string()))
            .collect(),
        primary_command: args.first().map(|arg| arg.to_string()),
        invoked_command: args.first().map(|arg| arg.to_string()),
        invoked_args: args.iter().map(|arg| arg.to_string()).collect(),
        observed_child_commands: Vec::new(),
        exit_code: 0,
        started_at_ns: 1,
        finished_at_ns: 2,
        reflog_start_offsets: HashMap::new(),
        stash_target_oid: None,
        cherry_pick_source_oids: Vec::new(),
        revert_source_oids: Vec::new(),
        ref_changes: Vec::new(),
        confidence: Confidence::Low,
    }
}

fn append_reflog(common_dir: &Path, reference: &str, entries: &[(&str, &str, &str)]) {
    let path = common_dir.join("logs").join(reference);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = String::new();
    for (old, new, message) in entries {
        text.push_str(&format!(
            "{old} {new} Test User <test@example.com> 0 +0000\t{message}\n"
        ));
    }
    fs::write(path, text).unwrap();
}

#[test]
fn first_observed_head_boundary_skips_prior_reset_history() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let head_log = git_dir.join("logs/HEAD");
    fs::create_dir_all(head_log.parent().unwrap()).unwrap();

    let old_line = format!("{A} {B} Test User <test@example.com> 0 +0000\treset: moving to old\n");
    let start_offset = old_line.len() as u64;
    let current_line =
        format!("{C} {D} Test User <test@example.com> 0 +0000\treset: moving to {D}\n");
    fs::write(&head_log, format!("{old_line}{current_line}")).unwrap();

    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let mut state = family_state(&family);
    state.refs.insert("HEAD".to_string(), A.to_string());
    let mut cursor = RefCursor::new(family.clone());
    let mut cmd = command_with_worktree(&family, Some(worktree), &["reset", "--hard", D]);
    cmd.reflog_start_offsets
        .insert(head_key(&git_dir), start_offset);

    cursor.enrich_command(&mut cmd, &state).unwrap();

    assert_eq!(
        cmd.ref_changes,
        vec![RefChange {
            reference: "HEAD".to_string(),
            old: C.to_string(),
            new: D.to_string(),
        }]
    );
}

#[test]
fn reset_late_reflog_offset_uses_command_message_not_stale_state() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let head_log = git_dir.join("logs/HEAD");
    fs::create_dir_all(head_log.parent().unwrap()).unwrap();

    let stale_line =
        format!("{A} {B} Test User <test@example.com> 0 +0000\treset: moving to stale\n");
    let current_line =
        format!("{C} {D} Test User <test@example.com> 0 +0000\treset: moving to {D}\n");
    let late_offset = (stale_line.len() + current_line.len()) as u64;
    fs::write(&head_log, format!("{stale_line}{current_line}")).unwrap();

    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let mut state = family_state(&family);
    state.refs.insert("HEAD".to_string(), A.to_string());
    let mut cursor = RefCursor::new(family.clone());
    let mut cmd = command_with_worktree(&family, Some(worktree), &["reset", "--soft", D]);
    cmd.reflog_start_offsets
        .insert(head_key(&git_dir), late_offset);

    cursor.enrich_command(&mut cmd, &state).unwrap();

    assert_eq!(
        cmd.ref_changes,
        vec![RefChange {
            reference: "HEAD".to_string(),
            old: C.to_string(),
            new: D.to_string(),
        }]
    );
}

#[test]
fn first_observed_common_boundary_skips_prior_update_ref_history() {
    let temp = tempfile::tempdir().unwrap();
    let reference = "refs/heads/main";
    let log_path = temp.path().join("logs").join(reference);
    fs::create_dir_all(log_path.parent().unwrap()).unwrap();

    let old_line = format!("{A} {B} Test User <test@example.com> 0 +0000\told stdin update\n");
    let start_offset = old_line.len() as u64;
    let current_line =
        format!("{C} {D} Test User <test@example.com> 0 +0000\tcurrent stdin update\n");
    fs::write(&log_path, format!("{old_line}{current_line}")).unwrap();

    let family = FamilyKey::new(temp.path().to_string_lossy().to_string());
    let state = family_state(&family);
    let mut cursor = RefCursor::new(family.clone());
    let mut cmd = command(&family, &["update-ref", "--stdin"]);
    cmd.reflog_start_offsets
        .insert(common_key(reference), start_offset);

    cursor.enrich_command(&mut cmd, &state).unwrap();

    assert_eq!(
        cmd.ref_changes,
        vec![RefChange {
            reference: reference.to_string(),
            old: C.to_string(),
            new: D.to_string(),
        }]
    );
}

#[test]
fn direct_branch_update_ref_uses_argv_transition_when_reflog_cursor_starts_too_late() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let reference = "refs/heads/feature";
    fs::create_dir_all(git_dir.join("logs").join("refs/heads")).unwrap();
    fs::create_dir_all(git_dir.join("logs")).unwrap();

    let branch_line = format!("{C} {D} Test User <test@example.com> 0 +0000\t\n");
    let head_line = format!("{C} {D} Test User <test@example.com> 0 +0000\t\n");
    let branch_log = git_dir.join("logs").join(reference);
    let head_log = git_dir.join("logs").join("HEAD");
    fs::write(&branch_log, &branch_line).unwrap();
    fs::write(&head_log, &head_line).unwrap();

    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let state = family_state(&family);
    let mut cursor = RefCursor::new(family.clone());
    let mut cmd = command_with_worktree(&family, Some(worktree), &["update-ref", reference, D, C]);
    cmd.reflog_start_offsets
        .insert(common_key(reference), branch_line.len() as u64);
    cmd.reflog_start_offsets
        .insert(head_key(&git_dir), head_line.len() as u64);

    cursor.enrich_command(&mut cmd, &state).unwrap();

    assert_eq!(
        cmd.ref_changes,
        vec![
            RefChange {
                reference: reference.to_string(),
                old: C.to_string(),
                new: D.to_string(),
            },
            RefChange {
                reference: "HEAD".to_string(),
                old: C.to_string(),
                new: D.to_string(),
            },
        ]
    );
}

#[test]
fn direct_branch_update_ref_does_not_treat_stale_head_reflog_match_as_current_head_move() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let reference = "refs/heads/feature";
    fs::create_dir_all(git_dir.join("logs").join("refs/heads")).unwrap();
    fs::create_dir_all(git_dir.join("logs")).unwrap();

    let stale_branch_line = format!("{C} {D} Test User <test@example.com> 0 +0000\t\n");
    let stale_head_line = format!("{C} {D} Test User <test@example.com> 0 +0000\t\n");
    let branch_log = git_dir.join("logs").join(reference);
    let head_log = git_dir.join("logs").join("HEAD");
    fs::write(&branch_log, &stale_branch_line).unwrap();
    fs::write(&head_log, &stale_head_line).unwrap();

    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let state = family_state(&family);
    let mut cursor = RefCursor::new(family.clone());
    let mut cmd = command_with_worktree(&family, Some(worktree), &["update-ref", reference, D, C]);
    cmd.started_at_ns = 2_000_000_000;
    cmd.finished_at_ns = 2_000_000_000;
    cmd.reflog_start_offsets
        .insert(common_key(reference), stale_branch_line.len() as u64);
    cmd.reflog_start_offsets
        .insert(head_key(&git_dir), stale_head_line.len() as u64);

    cursor.enrich_command(&mut cmd, &state).unwrap();

    assert_eq!(
        cmd.ref_changes,
        vec![RefChange {
            reference: reference.to_string(),
            old: C.to_string(),
            new: D.to_string(),
        }]
    );
}

#[test]
fn direct_update_ref_consumes_matching_reflog_entry_before_later_unstructured_update_ref() {
    let temp = tempfile::tempdir().unwrap();
    append_reflog(temp.path(), "refs/heads/main", &[(A, B, "")]);
    let family = FamilyKey::new(temp.path().to_string_lossy().to_string());
    let state = family_state(&family);
    let mut cursor = RefCursor::new(family.clone());

    let mut direct = command(&family, &["update-ref", "refs/heads/main", B, A]);
    cursor.enrich_command(&mut direct, &state).unwrap();
    assert_eq!(
        direct.ref_changes,
        vec![RefChange {
            reference: "refs/heads/main".to_string(),
            old: A.to_string(),
            new: B.to_string(),
        }]
    );

    let mut later = command(&family, &["update-ref", "--stdin"]);
    cursor.enrich_command(&mut later, &state).unwrap();

    assert!(
        later.ref_changes.is_empty(),
        "later unstructured update-ref must not replay reflog entry already represented by argv: {:?}",
        later.ref_changes
    );
}

#[test]
fn direct_branch_update_ref_consumes_head_mirror_before_later_unstructured_update_ref() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let reference = "refs/heads/feature";
    append_reflog(&git_dir, reference, &[(A, B, "")]);
    append_reflog(&git_dir, "HEAD", &[(A, B, "")]);
    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let state = family_state(&family);
    let mut cursor = RefCursor::new(family.clone());

    let mut direct = command_with_worktree(
        &family,
        Some(worktree.clone()),
        &["update-ref", reference, B, A],
    );
    cursor.enrich_command(&mut direct, &state).unwrap();
    assert_eq!(
        direct.ref_changes,
        vec![
            RefChange {
                reference: reference.to_string(),
                old: A.to_string(),
                new: B.to_string(),
            },
            RefChange {
                reference: "HEAD".to_string(),
                old: A.to_string(),
                new: B.to_string(),
            },
        ]
    );

    let mut later = command_with_worktree(&family, Some(worktree), &["update-ref", "--stdin"]);
    cursor.enrich_command(&mut later, &state).unwrap();

    assert!(
        later.ref_changes.is_empty(),
        "later unstructured update-ref must not replay HEAD or branch reflog entries already represented by argv: {:?}",
        later.ref_changes
    );
}

#[test]
fn direct_head_update_ref_uses_argv_and_late_cursor_branch_mirror_once() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let reference = "refs/heads/feature";
    append_reflog(&git_dir, reference, &[(A, B, "")]);
    append_reflog(&git_dir, "HEAD", &[(A, B, "")]);
    let branch_len = fs::metadata(git_dir.join("logs").join(reference))
        .unwrap()
        .len();
    let head_len = fs::metadata(git_dir.join("logs").join("HEAD"))
        .unwrap()
        .len();
    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let state = family_state(&family);
    let mut cursor = RefCursor::new(family.clone());

    let mut direct = command_with_worktree(
        &family,
        Some(worktree.clone()),
        &["update-ref", "HEAD", B, A],
    );
    direct
        .reflog_start_offsets
        .insert(common_key(reference), branch_len);
    direct
        .reflog_start_offsets
        .insert(head_key(&git_dir), head_len);
    cursor.enrich_command(&mut direct, &state).unwrap();
    assert_eq!(
        direct.ref_changes,
        vec![
            RefChange {
                reference: "HEAD".to_string(),
                old: A.to_string(),
                new: B.to_string(),
            },
            RefChange {
                reference: reference.to_string(),
                old: A.to_string(),
                new: B.to_string(),
            },
        ]
    );

    let mut later = command_with_worktree(&family, Some(worktree), &["update-ref", "--stdin"]);
    cursor.enrich_command(&mut later, &state).unwrap();

    assert!(
        later.ref_changes.is_empty(),
        "later unstructured update-ref must not replay branch mirror already represented by direct HEAD update-ref: {:?}",
        later.ref_changes
    );
}

#[test]
fn direct_head_update_ref_uses_known_worktree_branch_when_other_branch_matches_same_second() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let current = "refs/heads/main";
    let other = "refs/heads/other";
    append_reflog(&git_dir, current, &[(A, B, "")]);
    append_reflog(&git_dir, other, &[(A, B, "")]);
    append_reflog(&git_dir, "HEAD", &[(A, B, "")]);
    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let mut state = family_state(&family);
    state.worktrees.insert(
        worktree.canonicalize().unwrap(),
        WorktreeState {
            head: Some(A.to_string()),
            branch: Some(current.to_string()),
            detached: false,
            last_updated_ns: 0,
        },
    );
    let mut cursor = RefCursor::new(family.clone());

    let mut direct = command_with_worktree(
        &family,
        Some(worktree.clone()),
        &["update-ref", "HEAD", B, A],
    );
    cursor.enrich_command(&mut direct, &state).unwrap();
    assert_eq!(
        direct.ref_changes,
        vec![
            RefChange {
                reference: "HEAD".to_string(),
                old: A.to_string(),
                new: B.to_string(),
            },
            RefChange {
                reference: current.to_string(),
                old: A.to_string(),
                new: B.to_string(),
            },
        ]
    );

    let mut later = command_with_worktree(&family, Some(worktree), &["update-ref", other, B, A]);
    cursor.enrich_command(&mut later, &state).unwrap();

    assert_eq!(
        later.ref_changes,
        vec![RefChange {
            reference: other.to_string(),
            old: A.to_string(),
            new: B.to_string(),
        }]
    );
}

#[test]
fn direct_head_update_ref_without_known_branch_does_not_guess_ambiguous_branch_mirror() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    append_reflog(&git_dir, "refs/heads/main", &[(A, B, "")]);
    append_reflog(&git_dir, "refs/heads/other", &[(A, B, "")]);
    append_reflog(&git_dir, "HEAD", &[(A, B, "")]);
    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let state = family_state(&family);
    let mut cursor = RefCursor::new(family.clone());

    let mut direct = command_with_worktree(&family, Some(worktree), &["update-ref", "HEAD", B, A]);
    cursor.enrich_command(&mut direct, &state).unwrap();

    assert_eq!(
        direct.ref_changes,
        vec![RefChange {
            reference: "HEAD".to_string(),
            old: A.to_string(),
            new: B.to_string(),
        }]
    );
}

#[test]
fn direct_branch_update_ref_does_not_attach_head_when_state_names_different_branch() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    let current = "refs/heads/main";
    let updated = "refs/heads/feature";
    append_reflog(&git_dir, updated, &[(A, B, "")]);
    append_reflog(&git_dir, "HEAD", &[(A, B, "")]);
    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    let mut state = family_state(&family);
    state.worktrees.insert(
        worktree.canonicalize().unwrap(),
        WorktreeState {
            head: Some(A.to_string()),
            branch: Some(current.to_string()),
            detached: false,
            last_updated_ns: 0,
        },
    );
    let mut cursor = RefCursor::new(family.clone());

    let mut direct = command_with_worktree(&family, Some(worktree), &["update-ref", updated, B, A]);
    cursor.enrich_command(&mut direct, &state).unwrap();

    assert_eq!(
        direct.ref_changes,
        vec![RefChange {
            reference: updated.to_string(),
            old: A.to_string(),
            new: B.to_string(),
        }]
    );
}
