use super::*;
use crate::operations::daemon::analyzers::tests::command;

fn fixture(args: &[&str]) -> (NormalizedCommand, WorktreeState) {
    let mut cmd = command("rm", args);
    cmd.trace_derived = true;
    cmd.started_at_ns = 20;
    cmd.finished_at_ns = 30;
    let state = WorktreeState {
        head: Some("a".repeat(40)),
        branch: None,
        detached: true,
        last_updated_ns: 10,
    };
    (cmd, state)
}

#[test]
fn accepts_only_proven_file_removals() {
    let (cmd, state) = fixture(&[
        "git",
        "-C",
        "sub",
        "rm",
        "-f",
        "--",
        ":(top,literal)file[*].txt",
        ":(literal,top)nested/file.txt",
    ]);
    assert_eq!(
        analyze_removal(&cmd, Some(&state)),
        Some(SemanticEvent::WorkingTreeFilesRemoved {
            head: "a".repeat(40),
            files: vec!["file[*].txt".into(), "nested/file.txt".into()],
        })
    );
}

#[test]
fn rejects_ambiguous_or_non_destructive_invocations() {
    for args in [
        vec!["rm", "-f", "--", "file.txt"],
        vec!["rm", "-f", "--", ":(top)file*"],
        vec!["rm", "-f", "--", ":(top,literal)../file.txt"],
        vec!["rm", "-f", "--", ":(top,literal)dir/"],
        vec!["rm", "-f", "--", ":(top,literal)file.txt", "other.txt"],
        vec!["rm", "-r", "--", ":(top,literal)dir"],
        vec!["rm", "--cached", "--", ":(top,literal)file.txt"],
        vec!["rm", "--dry-run", "--", ":(top,literal)file.txt"],
        vec!["rm", "--ignore-unmatch", "--", ":(top,literal)file.txt"],
        vec!["rm", "--pathspec-from-file=paths"],
        vec!["rm", "--"],
        vec!["--literal-pathspecs", "rm", "--", ":(top,literal)file.txt"],
        vec![
            "-c",
            "core.worktree=elsewhere",
            "rm",
            "--",
            ":(top,literal)file.txt",
        ],
    ] {
        let mut argv = vec!["git"];
        argv.extend(args);
        let (cmd, state) = fixture(&argv);
        assert_eq!(analyze_removal(&cmd, Some(&state)), None, "{argv:?}");
    }
}

#[test]
fn rejects_missing_unverified_or_future_worktree_heads() {
    let (mut cmd, mut state) = fixture(&["git", "rm", "--", ":(top,literal)file.txt"]);
    assert_eq!(analyze_removal(&cmd, None), None);
    state.head = None;
    assert_eq!(analyze_removal(&cmd, Some(&state)), None);
    state.head = Some("HEAD".into());
    assert_eq!(analyze_removal(&cmd, Some(&state)), None);
    state.head = Some("0".repeat(40));
    assert_eq!(analyze_removal(&cmd, Some(&state)), None);
    state.head = Some("a".repeat(40));
    state.last_updated_ns = 21;
    assert_eq!(analyze_removal(&cmd, Some(&state)), None);
    state.last_updated_ns = 10;
    cmd.exit_code = 1;
    assert_eq!(analyze_removal(&cmd, Some(&state)), None);
    cmd.exit_code = 0;
    cmd.trace_derived = false;
    assert_eq!(analyze_removal(&cmd, Some(&state)), None);
}

#[test]
fn reducer_uses_this_worktree_head_and_never_the_family_head() {
    use crate::model::domain::{FamilyKey, FamilyState, WatermarkState};
    use crate::operations::daemon::{analyzers::AnalyzerRegistry, reducer};
    use std::collections::HashMap;
    use std::path::PathBuf;

    let (mut cmd, worktree) = fixture(&["git", "rm", "--", ":(top,literal)file.txt"]);
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
    let (_, analysis) = reducer::reduce_family_command_with_ref_snapshot(
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
    let (_, analysis) = reducer::reduce_family_command_with_ref_snapshot(
        &mut state,
        cmd,
        &AnalyzerRegistry::new(),
        &HashMap::new(),
        Some(canonical),
    )
    .unwrap();
    assert_eq!(analysis.events, vec![SemanticEvent::OpaqueCommand]);
}
