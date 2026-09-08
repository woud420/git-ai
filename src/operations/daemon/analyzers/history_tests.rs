use super::*;
use crate::model::domain::{CommandScope, RefChange};

fn command(primary: &str, argv: &[&str]) -> NormalizedCommand {
    NormalizedCommand {
        scope: CommandScope::Global,
        family_key: None,
        worktree: None,
        root_sid: "r".to_string(),
        trace_derived: false,
        raw_argv: argv.iter().map(|s| s.to_string()).collect(),
        primary_command: Some(primary.to_string()),
        invoked_command: Some(primary.to_string()),
        invoked_args: argv.iter().skip(2).map(|s| s.to_string()).collect(),
        observed_child_commands: Vec::new(),
        exit_code: 0,
        started_at_ns: 1,
        finished_at_ns: 2,
        reflog_start_offsets: std::collections::HashMap::new(),
        stash_target_oid: None,
        cherry_pick_source_oids: Vec::new(),
        revert_source_oids: Vec::new(),
        ref_changes: vec![RefChange {
            reference: "HEAD".to_string(),
            old: "a".to_string(),
            new: "b".to_string(),
        }],
        confidence: Confidence::Low,
    }
}

fn assert_only_opaque(result: &AnalysisResult) {
    assert!(
        result
            .events
            .iter()
            .all(|event| matches!(event, SemanticEvent::OpaqueCommand)),
        "expected only opaque events, got {:?}",
        result.events
    );
}

#[test]
fn update_ref_reports_cursor_ref_changes() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command(
        "update-ref",
        &[
            "git",
            "update-ref",
            "refs/heads/main",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
    );
    cmd.ref_changes = vec![RefChange {
        reference: "refs/heads/main".to_string(),
        old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        new: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
    }];

    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();

    assert!(result.events.iter().any(|event| matches!(
        event,
        SemanticEvent::RefUpdated { reference, old, new }
            if reference == "refs/heads/main"
                && old == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                && new == "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    )));
}

#[test]
fn update_ref_without_cursor_ref_change_is_opaque() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command(
        "update-ref",
        &[
            "git",
            "update-ref",
            "refs/heads/main",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
    );
    cmd.ref_changes.clear();
    let refs = std::collections::HashMap::from([(
        "refs/heads/main".to_string(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
    )]);

    let result = analyzer
        .analyze(&cmd, AnalysisView { refs: &refs })
        .unwrap();

    assert_only_opaque(&result);
}

#[test]
fn squash_merge_resolves_branch_from_ref_state() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("merge", &["git", "merge", "--squash", "feature"]);
    cmd.ref_changes.clear();
    let refs = std::collections::HashMap::from([
        (
            "HEAD".to_string(),
            "1111111111111111111111111111111111111111".to_string(),
        ),
        (
            "refs/heads/feature".to_string(),
            "2222222222222222222222222222222222222222".to_string(),
        ),
    ]);

    let result = analyzer
        .analyze(&cmd, AnalysisView { refs: &refs })
        .unwrap();

    assert!(result.events.iter().any(|event| matches!(
        event,
        SemanticEvent::MergeSquash { source_head, onto }
            if source_head == "2222222222222222222222222222222222222222"
                && onto == "1111111111111111111111111111111111111111"
    )));
}

#[test]
fn squash_merge_with_unresolved_source_is_opaque() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("merge", &["git", "merge", "--squash", "feature"]);
    cmd.ref_changes.clear();
    let refs = std::collections::HashMap::from([(
        "HEAD".to_string(),
        "1111111111111111111111111111111111111111".to_string(),
    )]);

    let result = analyzer
        .analyze(&cmd, AnalysisView { refs: &refs })
        .unwrap();

    assert_only_opaque(&result);
}

#[test]
fn commit_without_amend_emits_commit_created() {
    let analyzer = HistoryAnalyzer;
    let result = analyzer
        .analyze(
            &command("commit", &["git", "commit", "-m", "x"]),
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();
    assert!(
        result
            .events
            .iter()
            .any(|event| matches!(event, SemanticEvent::CommitCreated { .. }))
    );
}

#[test]
fn amend_prefers_head_transition_over_zero_old_branch_change() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "commit", "--amend", "-m", "x"]);
    cmd.ref_changes = vec![
        RefChange {
            reference: "refs/heads/main".to_string(),
            old: "0000000000000000000000000000000000000000".to_string(),
            new: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        },
        RefChange {
            reference: "refs/heads/main".to_string(),
            old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            new: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
        RefChange {
            reference: "HEAD".to_string(),
            old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            new: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
    ];

    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();

    assert!(result.events.iter().any(|event| matches!(
        event,
        SemanticEvent::CommitAmended { old_head, new_head }
            if old_head == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                && new_head == "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    )));
}

#[test]
fn amend_prefers_head_transition_over_contaminated_branch_hint() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "commit", "--amend", "-m", "x"]);
    cmd.ref_changes = vec![
        RefChange {
            reference: "HEAD".to_string(),
            old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            new: "dddddddddddddddddddddddddddddddddddddddd".to_string(),
        },
        RefChange {
            reference: "refs/heads/child".to_string(),
            old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            new: "dddddddddddddddddddddddddddddddddddddddd".to_string(),
        },
        RefChange {
            reference: "refs/heads/parent".to_string(),
            old: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            new: "dddddddddddddddddddddddddddddddddddddddd".to_string(),
        },
    ];

    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();

    assert!(result.events.iter().any(|event| matches!(
        event,
        SemanticEvent::CommitAmended { old_head, new_head }
            if old_head == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                && new_head == "dddddddddddddddddddddddddddddddddddddddd"
    )));
}

#[test]
fn amend_uses_first_head_transition_when_later_head_moves_are_captured() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "commit", "--amend", "-m", "x"]);
    cmd.ref_changes = vec![
        RefChange {
            reference: "HEAD".to_string(),
            old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            new: "dddddddddddddddddddddddddddddddddddddddd".to_string(),
        },
        RefChange {
            reference: "HEAD".to_string(),
            old: "dddddddddddddddddddddddddddddddddddddddd".to_string(),
            new: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string(),
        },
    ];

    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();

    assert!(result.events.iter().any(|event| matches!(
        event,
        SemanticEvent::CommitAmended { old_head, new_head }
            if old_head == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                && new_head == "dddddddddddddddddddddddddddddddddddddddd"
    )));
}

#[test]
fn reset_emits_reset_kind() {
    let analyzer = HistoryAnalyzer;
    let result = analyzer
        .analyze(
            &command("reset", &["git", "reset", "--hard", "HEAD~1"]),
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();
    assert!(result.events.iter().any(|event| matches!(
        event,
        SemanticEvent::Reset {
            kind: ResetKind::Hard,
            ..
        }
    )));
}

#[test]
fn commit_without_ref_transition_is_opaque() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "commit", "-m", "x"]);
    cmd.ref_changes.clear();

    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();

    assert_only_opaque(&result);
}

#[test]
fn commit_without_ref_transition_ignores_family_refs() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "commit", "-m", "x"]);
    cmd.ref_changes.clear();
    let refs = std::collections::HashMap::from([(
        "refs/heads/main".to_string(),
        "wrong-family-head".to_string(),
    )]);

    let result = analyzer
        .analyze(&cmd, AnalysisView { refs: &refs })
        .unwrap();

    assert_only_opaque(&result);
}

#[test]
fn commit_without_ref_transition_ignores_family_head() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "commit", "-m", "x"]);
    cmd.ref_changes.clear();
    let refs =
        std::collections::HashMap::from([("refs/heads/main".to_string(), "old-head".to_string())]);

    let result = analyzer
        .analyze(&cmd, AnalysisView { refs: &refs })
        .unwrap();
    assert_only_opaque(&result);
}

#[test]
fn commit_without_ref_transition_does_not_read_head_reflog() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "commit", "-m", "x"]);
    cmd.ref_changes.clear();

    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();

    assert_only_opaque(&result);
}

#[test]
fn commit_prefers_head_transition_over_other_branch_ref_changes() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("commit", &["git", "-C", "/repo-b", "commit", "-m", "x"]);
    cmd.ref_changes = vec![
        RefChange {
            reference: "refs/heads/branch-a".to_string(),
            old: "0000000000000000000000000000000000000000".to_string(),
            new: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        },
        RefChange {
            reference: "refs/heads/branch-b".to_string(),
            old: "0000000000000000000000000000000000000000".to_string(),
            new: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
        RefChange {
            reference: "HEAD".to_string(),
            old: "0000000000000000000000000000000000000000".to_string(),
            new: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        },
    ];
    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();
    assert!(
        result.events.iter().any(|event| matches!(
            event,
            SemanticEvent::CommitCreated {
                new_head,
                ..
            } if new_head == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )),
        "expected commit-created event to use the captured HEAD transition, got {:?}",
        result.events
    );
}

#[test]
fn head_change_prefers_head_transition_over_branch_ref_change() {
    let mut cmd = command("commit", &["git", "commit", "-m", "x"]);
    cmd.ref_changes = vec![
        RefChange {
            reference: "HEAD".to_string(),
            old: "old-head".to_string(),
            new: "wrong-head".to_string(),
        },
        RefChange {
            reference: "refs/heads/main".to_string(),
            old: "old-main".to_string(),
            new: "new-main".to_string(),
        },
    ];
    let change = head_change(&cmd, &Default::default());
    assert_eq!(
        change,
        Some(("old-head".to_string(), "wrong-head".to_string())),
        "expected captured HEAD transition to win over branch ref changes"
    );
}

#[test]
fn rebase_continue_prefers_branch_ref_change_over_head_span() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("rebase", &["git", "rebase", "--continue"]);
    cmd.ref_changes = vec![
        RefChange {
            reference: "refs/heads/feature".to_string(),
            old: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            new: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
        RefChange {
            reference: "HEAD".to_string(),
            old: "cccccccccccccccccccccccccccccccccccccccc".to_string(),
            new: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
    ];
    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();
    assert!(
        result.events.iter().any(|event| matches!(
            event,
            SemanticEvent::RebaseComplete {
                old_head,
                new_head,
                ..
            } if old_head == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                && new_head == "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        )),
        "expected rebase-complete to use branch ref span, got {:?}",
        result.events
    );
}

#[test]
fn cherry_pick_uses_full_head_ref_change_span() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command(
        "cherry-pick",
        &["git", "cherry-pick", "source-1", "source-2", "source-3"],
    );
    cmd.ref_changes = vec![
        RefChange {
            reference: "HEAD".to_string(),
            old: "a".to_string(),
            new: "b".to_string(),
        },
        RefChange {
            reference: "HEAD".to_string(),
            old: "b".to_string(),
            new: "c".to_string(),
        },
        RefChange {
            reference: "HEAD".to_string(),
            old: "c".to_string(),
            new: "d".to_string(),
        },
    ];

    let result = analyzer
        .analyze(
            &cmd,
            AnalysisView {
                refs: &Default::default(),
            },
        )
        .unwrap();

    assert!(
        result.events.iter().any(|event| matches!(
            event,
            SemanticEvent::CherryPickComplete {
                original_head,
                new_head,
                ..
            } if original_head == "a" && new_head == "d"
        )),
        "expected cherry-pick span event, got {:?}",
        result.events
    );
}

#[test]
fn cherry_pick_without_ref_transition_is_opaque() {
    let analyzer = HistoryAnalyzer;
    let mut cmd = command("cherry-pick", &["git", "cherry-pick", "--continue"]);
    cmd.ref_changes.clear();
    let refs = std::collections::HashMap::from([
        ("HEAD".to_string(), "old-head".to_string()),
        ("refs/heads/main".to_string(), "old-head".to_string()),
    ]);
    let result = analyzer
        .analyze(&cmd, AnalysisView { refs: &refs })
        .unwrap();
    assert_only_opaque(&result);
}
