use super::*;
use crate::operations::daemon::analyzers::tests::command;

#[test]
fn orphan_checkout_requires_the_content_preserving_form() {
    let state = WorktreeState {
        head: Some("a".repeat(40)),
        branch: Some("refs/heads/main".into()),
        detached: false,
        last_updated_ns: 0,
    };
    let mut cmd = command("checkout", &["git", "checkout", "--orphan", "new-root"]);
    cmd.trace_derived = true;
    assert_eq!(
        analyze_orphan_checkout(&cmd, Some(&state)),
        Some(SemanticEvent::OrphanBranchCreated {
            old_head: "a".repeat(40),
            branch: "refs/heads/new-root".into(),
            discard_tracked: false,
        })
    );
    for argv in [
        vec!["git", "checkout", "--orphan", "new-root", "HEAD~1"],
        vec!["git", "checkout", "--orphan", "new-root", "HEAD"],
        vec!["git", "checkout", "-f", "--orphan", "new-root"],
        vec!["git", "checkout", "--orphan", "new-root", "--", "file"],
        vec![
            "git",
            "-c",
            "core.worktree=elsewhere",
            "checkout",
            "--orphan",
            "new-root",
        ],
    ] {
        cmd.raw_argv = argv.iter().map(|arg| arg.to_string()).collect();
        assert_eq!(
            analyze_orphan_checkout(&cmd, Some(&state)),
            None,
            "{argv:?}"
        );
    }
}

#[test]
fn orphan_checkout_with_observed_ref_mutation_is_not_assumed_unborn() {
    let state = WorktreeState {
        head: Some("a".repeat(40)),
        branch: Some("refs/heads/main".into()),
        detached: false,
        last_updated_ns: 0,
    };
    let mut cmd = command("checkout", &["git", "checkout", "--orphan", "new-root"]);
    cmd.trace_derived = true;
    cmd.ref_changes.push(crate::model::domain::RefChange {
        reference: "HEAD".into(),
        old: "0".repeat(40),
        new: "b".repeat(40),
    });
    assert_eq!(analyze_orphan_checkout(&cmd, Some(&state)), None);
    cmd.ref_changes.clear();
    cmd.observed_child_commands.push("commit".into());
    assert_eq!(analyze_orphan_checkout(&cmd, Some(&state)), None);
}
