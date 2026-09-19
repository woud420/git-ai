use super::*;
use crate::model::domain::{
    CommandScope, Confidence, FamilyKey, FamilyState, GlobalState, RefChange, WatermarkState,
    WorktreeState,
};
use crate::operations::daemon::analyzers::AnalyzerRegistry;
use std::collections::HashMap;

fn family_state() -> FamilyState {
    FamilyState {
        family_key: FamilyKey::new("family:/tmp/repo"),
        refs: HashMap::new(),
        worktrees: HashMap::new(),
        last_error: None,
        applied_seq: 0,
        watermarks: WatermarkState::default(),
    }
}

fn normalized() -> NormalizedCommand {
    NormalizedCommand {
        scope: CommandScope::Family(FamilyKey::new("family:/tmp/repo")),
        family_key: Some(FamilyKey::new("family:/tmp/repo")),
        worktree: Some(PathBuf::from("/tmp/repo")),
        root_sid: "sid".to_string(),
        trace_derived: false,
        raw_argv: vec!["git".to_string(), "update-ref".to_string()],
        primary_command: Some("update-ref".to_string()),
        invoked_command: Some("update-ref".to_string()),
        invoked_args: Vec::new(),
        observed_child_commands: Vec::new(),
        exit_code: 0,
        started_at_ns: 1,
        finished_at_ns: 2,
        reflog_start_offsets: std::collections::HashMap::new(),
        stash_target_oid: None,
        cherry_pick_source_oids: Vec::new(),
        revert_source_oids: Vec::new(),
        ref_changes: vec![RefChange {
            reference: "refs/heads/main".to_string(),
            old: "".to_string(),
            new: "abc".to_string(),
        }],
        confidence: Confidence::Low,
    }
}

#[test]
fn reducer_applies_ref_changes_and_produces_applied_command() {
    let mut state = family_state();
    let registry = AnalyzerRegistry::new();
    let (applied, analysis) = reduce_family_command(&mut state, normalized(), &registry).unwrap();
    assert_eq!(applied.seq, 1);
    assert!(matches!(
        analysis.class,
        crate::model::domain::CommandClass::HistoryRewrite
    ));
    assert_eq!(
        state.refs.get("refs/heads/main").map(String::as_str),
        Some("abc")
    );
}

#[test]
fn reducer_does_not_update_refs_without_ref_transition_for_head_moving_commands() {
    let mut state = family_state();
    let registry = AnalyzerRegistry::new();
    let mut cmd = normalized();
    cmd.ref_changes.clear();
    cmd.raw_argv = vec!["git".to_string(), "commit".to_string()];
    cmd.primary_command = Some("commit".to_string());
    cmd.invoked_command = Some("commit".to_string());

    let (_applied, _analysis) = reduce_family_command(&mut state, cmd, &registry).unwrap();

    assert_eq!(state.refs.get("refs/heads/main").map(String::as_str), None);
}

#[test]
fn reducer_preserves_refs_for_stash_without_ref_transition() {
    let mut state = family_state();
    state
        .refs
        .insert("refs/heads/main".to_string(), "abc".to_string());
    let registry = AnalyzerRegistry::new();
    let mut cmd = normalized();
    cmd.ref_changes.clear();
    cmd.raw_argv = vec!["git".to_string(), "stash".to_string(), "push".to_string()];
    cmd.primary_command = Some("stash".to_string());
    cmd.invoked_command = Some("stash".to_string());
    cmd.invoked_args = vec!["push".to_string()];

    let (_applied, _analysis) = reduce_family_command(&mut state, cmd, &registry).unwrap();

    assert_eq!(
        state.refs.get("refs/heads/main").map(String::as_str),
        Some("abc")
    );
}

#[test]
fn reducer_preserves_ref_deletion_and_raw_observation_semantics() {
    for (new, deleted) in [
        (String::new(), true),
        (" \t\n".to_string(), true),
        ("0".repeat(40), true),
        ("0".repeat(64), true),
        ("0".repeat(39), false),
        ("0".repeat(41), false),
        ("0".repeat(63), false),
        ("0".repeat(65), false),
        (format!(" {}", "0".repeat(40)), false),
        (format!("{}\n", "0".repeat(64)), false),
        ("abc".to_string(), false),
        ("A".repeat(40), false),
        ("f".repeat(64), false),
    ] {
        let mut state = family_state();
        state.refs.insert(
            "refs/heads/feature".to_string(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        );
        let registry = AnalyzerRegistry::new();
        let mut cmd = normalized();
        cmd.ref_changes = vec![RefChange {
            reference: "refs/heads/feature".to_string(),
            old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            new: new.clone(),
        }];
        let observed = cmd.ref_changes.clone();
        let (applied, analysis) = reduce_family_command(&mut state, cmd, &registry).unwrap();
        assert_eq!(
            state.refs.get("refs/heads/feature"),
            if deleted { None } else { Some(&new) },
            "new ref value: {new:?}",
        );
        assert_eq!(applied.command.ref_changes, observed);
        assert_eq!(applied.analysis, analysis);
        assert_eq!(applied.seq, 1);
    }
}

#[test]
fn reducer_records_worktree_branch_from_unique_head_branch_transition() {
    let mut state = family_state();
    let registry = AnalyzerRegistry::new();
    let mut cmd = normalized();
    cmd.ref_changes = vec![
        RefChange {
            reference: "HEAD".to_string(),
            old: "aaa".to_string(),
            new: "bbb".to_string(),
        },
        RefChange {
            reference: "refs/heads/main".to_string(),
            old: "aaa".to_string(),
            new: "bbb".to_string(),
        },
    ];

    let (_applied, _analysis) = reduce_family_command(&mut state, cmd, &registry).unwrap();
    let worktree = state.worktrees.get(&PathBuf::from("/tmp/repo")).unwrap();

    assert_eq!(worktree.head.as_deref(), Some("bbb"));
    assert_eq!(worktree.branch.as_deref(), Some("refs/heads/main"));
    assert!(!worktree.detached);
}

#[test]
fn reducer_preserves_worktree_branch_when_command_does_not_move_head() {
    let mut state = family_state();
    state.worktrees.insert(
        PathBuf::from("/tmp/repo"),
        WorktreeState {
            head: Some("aaa".to_string()),
            branch: Some("refs/heads/main".to_string()),
            detached: false,
            last_updated_ns: 1,
        },
    );
    let registry = AnalyzerRegistry::new();
    let mut cmd = normalized();
    cmd.ref_changes = vec![RefChange {
        reference: "refs/heads/other".to_string(),
        old: "ccc".to_string(),
        new: "ddd".to_string(),
    }];

    let (_applied, _analysis) = reduce_family_command(&mut state, cmd, &registry).unwrap();
    let worktree = state.worktrees.get(&PathBuf::from("/tmp/repo")).unwrap();

    assert_eq!(worktree.head.as_deref(), Some("aaa"));
    assert_eq!(worktree.branch.as_deref(), Some("refs/heads/main"));
    assert!(!worktree.detached);
}

#[test]
fn reducer_updates_branch_for_checkout_new_branch_without_head_oid_move() {
    let mut state = family_state();
    state.worktrees.insert(
        PathBuf::from("/tmp/repo"),
        WorktreeState {
            head: Some("aaa".to_string()),
            branch: Some("refs/heads/main".to_string()),
            detached: false,
            last_updated_ns: 1,
        },
    );
    let registry = AnalyzerRegistry::new();
    let mut cmd = normalized();
    cmd.raw_argv = vec![
        "git".to_string(),
        "checkout".to_string(),
        "-b".to_string(),
        "feature".to_string(),
    ];
    cmd.primary_command = Some("checkout".to_string());
    cmd.invoked_command = Some("checkout".to_string());
    cmd.invoked_args = vec!["-b".to_string(), "feature".to_string()];
    cmd.ref_changes.clear();

    let (_applied, _analysis) = reduce_family_command(&mut state, cmd, &registry).unwrap();
    let worktree = state.worktrees.get(&PathBuf::from("/tmp/repo")).unwrap();

    assert_eq!(worktree.head.as_deref(), Some("aaa"));
    assert_eq!(worktree.branch.as_deref(), Some("refs/heads/feature"));
    assert!(!worktree.detached);
}

#[test]
fn reducer_marks_head_only_transition_as_detached_or_unknown_branch() {
    let mut state = family_state();
    let registry = AnalyzerRegistry::new();
    let mut cmd = normalized();
    cmd.ref_changes = vec![RefChange {
        reference: "HEAD".to_string(),
        old: "aaa".to_string(),
        new: "bbb".to_string(),
    }];

    let (_applied, _analysis) = reduce_family_command(&mut state, cmd, &registry).unwrap();
    let worktree = state.worktrees.get(&PathBuf::from("/tmp/repo")).unwrap();

    assert_eq!(worktree.head.as_deref(), Some("bbb"));
    assert_eq!(worktree.branch, None);
    assert!(worktree.detached);
}

#[test]
fn global_reducer_never_drops_commands() {
    let mut state = GlobalState { applied_seq: 0 };
    let registry = AnalyzerRegistry::new();
    let (applied, _analysis) = reduce_global_command(&mut state, normalized(), &registry).unwrap();
    assert_eq!(applied.seq, 1);
    assert_eq!(state.applied_seq, 1);
}

/// Pins that `canonical_worktree` drives worktree state keying:
/// a command whose `cmd.worktree` is a raw path (e.g. a symlink like
/// `/tmp/repo`) and a later command using the resolved canonical path
/// must both update the SAME `WorktreeState` entry — the one keyed by
/// the canonical path.
///
/// This is the behavioral guarantee that `family_actor` relies on when it
/// calls `reduce_family_command_with_ref_snapshot` with the
/// canonicalized path: symlinked and resolved paths collapse to one slot.
#[test]
fn canonical_worktree_overrides_raw_path_keying() {
    let raw_path = PathBuf::from("/tmp/repo");
    let canonical_path = PathBuf::from("/private/tmp/repo");
    let registry = AnalyzerRegistry::new();

    // First call: raw worktree path, canonical override supplied.
    let mut state = family_state();
    let mut cmd = normalized();
    cmd.ref_changes = vec![RefChange {
        reference: "HEAD".to_string(),
        old: "aaa".to_string(),
        new: "bbb".to_string(),
    }];
    cmd.worktree = Some(raw_path.clone());
    reduce_family_command_with_ref_snapshot(
        &mut state,
        cmd,
        &registry,
        &std::collections::HashMap::new(),
        Some(canonical_path.clone()),
    )
    .unwrap();

    // State must be keyed by the CANONICAL path, not the raw path.
    assert!(
        !state.worktrees.contains_key(&raw_path),
        "worktree must not be keyed by raw path"
    );
    assert!(
        state.worktrees.contains_key(&canonical_path),
        "worktree must be keyed by canonical path"
    );

    // Second call: this time the caller already has the canonical path
    // (as family_actor would after a second canonicalize call on the
    // same real directory).
    let mut cmd2 = normalized();
    cmd2.ref_changes = vec![RefChange {
        reference: "HEAD".to_string(),
        old: "bbb".to_string(),
        new: "ccc".to_string(),
    }];
    cmd2.worktree = Some(canonical_path.clone());
    reduce_family_command_with_ref_snapshot(
        &mut state,
        cmd2,
        &registry,
        &std::collections::HashMap::new(),
        Some(canonical_path.clone()),
    )
    .unwrap();

    // Still exactly one entry, keyed by the canonical path.
    assert_eq!(
        state.worktrees.len(),
        1,
        "both commands must update the same WorktreeState entry"
    );
    let worktree = state.worktrees.get(&canonical_path).unwrap();
    assert_eq!(worktree.head.as_deref(), Some("ccc"));
}

#[test]
fn reducer_uses_this_worktree_head_and_never_the_family_head() {
    use crate::model::domain::{FamilyKey, FamilyState, SemanticEvent, WatermarkState};
    use std::collections::HashMap;
    use std::path::PathBuf;

    let mut cmd = normalized();
    cmd.raw_argv = ["git", "rm", "--", ":(top,literal)file.txt"]
        .map(str::to_string)
        .to_vec();
    cmd.primary_command = Some("rm".to_string());
    cmd.invoked_command = Some("rm".to_string());
    cmd.invoked_args = cmd.raw_argv[2..].to_vec();
    cmd.ref_changes.clear();
    cmd.trace_derived = true;
    cmd.started_at_ns = 20;
    cmd.finished_at_ns = 30;
    let worktree = WorktreeState {
        head: Some("a".repeat(40)),
        branch: None,
        detached: true,
        last_updated_ns: 10,
    };
    cmd.worktree = Some(PathBuf::from("/alias/worktree"));
    let canonical = PathBuf::from("/canonical/worktree");
    let mut state = FamilyState {
        family_key: FamilyKey::new("family"),
        refs: HashMap::from([("HEAD".into(), "b".repeat(40))]),
        worktrees: HashMap::from([(canonical.clone(), worktree)]),
        last_error: None,
        applied_seq: 0,
        watermarks: WatermarkState::default(),
    };
    let (_, analysis) = reduce_family_command_with_ref_snapshot(
        &mut state,
        cmd.clone(),
        &AnalyzerRegistry::new(),
        &HashMap::new(),
        Some(canonical.clone()),
    )
    .unwrap();
    assert_eq!(
        analysis.events,
        vec![SemanticEvent::WorkingTreeFilesRemoved {
            head: "a".repeat(40),
            files: vec!["file.txt".into()],
        }]
    );

    state.worktrees.clear();
    let (_, analysis) = reduce_family_command_with_ref_snapshot(
        &mut state,
        cmd,
        &AnalyzerRegistry::new(),
        &HashMap::new(),
        Some(canonical),
    )
    .unwrap();
    assert_eq!(analysis.events, vec![SemanticEvent::OpaqueCommand]);
}
