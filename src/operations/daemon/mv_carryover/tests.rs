use super::*;
use crate::model::domain::{CommandScope, Confidence};
use crate::model::domain::{NormalizedCommand, SemanticEvent};
use crate::operations::daemon::analyzers::mv_carryover::event;
use std::collections::HashMap;

fn fixture() -> (NormalizedCommand, HashMap<String, String>) {
    let worktree = std::env::temp_dir().join("mv-root");
    let head = "a".repeat(40);
    let cmd = NormalizedCommand {
        scope: CommandScope::Global,
        family_key: None,
        worktree: Some(worktree.clone()),
        root_sid: "move".into(),
        trace_derived: true,
        raw_argv: vec![
            "git".into(),
            "-C".into(),
            worktree.to_str().unwrap().into(),
            "mv".into(),
            "--".into(),
            "nested/file.txt".into(),
            ".".into(),
        ],
        primary_command: Some("mv".into()),
        invoked_command: Some("mv".into()),
        invoked_args: Vec::new(),
        observed_child_commands: Vec::new(),
        transport_targets: None,
        index_v2: false,
        index_write: IndexWriteEvidence::Exact(worktree.join(".git/index.lock")),
        exit_code: 0,
        started_at_ns: 1,
        finished_at_ns: 2,
        reflog_start_offsets: HashMap::new(),
        stash_target_oid: None,
        cherry_pick_source_oids: Vec::new(),
        revert_source_oids: Vec::new(),
        ref_changes: Vec::new(),
        confidence: Confidence::High,
    };
    (cmd, HashMap::from([("HEAD".into(), head)]))
}

#[test]
fn accepts_only_a_proven_root_move_mapping() {
    let (cmd, refs) = fixture();
    assert!(
        matches!(event(&cmd, &refs), Some(SemanticEvent::WorkingLogPathMoved { base_commit, source, destination })
        if base_commit == refs["HEAD"] && source == "nested/file.txt" && destination == "file.txt")
    );
    for args in [
        vec!["--dry-run", "--", "nested/file.txt", "."],
        vec!["--force", "--", "nested/file.txt", "."],
        vec!["-k", "--", "nested/file.txt", "."],
        vec!["--", "nested/file.txt", "destination/"],
        vec!["nested/file.txt", "."],
        vec!["--", "nested/file.txt", "nested/other.txt", "."],
        vec!["--", "../file.txt", "."],
        vec!["--", "nested/./file.txt", "."],
        vec!["--", "file.txt", "."],
        vec!["--", "nested/bracket[1].txt", "."],
    ] {
        let mut current = cmd.clone();
        current.raw_argv.truncate(4);
        current
            .raw_argv
            .extend(args.iter().map(|arg| arg.to_string()));
        assert!(event(&current, &refs).is_none(), "{args:?}");
    }
    let mut literal = cmd.clone();
    literal.raw_argv[5] = "nested/bracket[1].txt".into();
    literal.raw_argv.insert(1, "--literal-pathspecs".into());
    assert!(event(&literal, &refs).is_some());
}

#[test]
fn rejects_unknown_roots_heads_and_receipts() {
    let (cmd, refs) = fixture();
    let mut current = cmd.clone();
    current.raw_argv.drain(1..3);
    assert!(event(&current, &refs).is_none());
    current = cmd.clone();
    current.raw_argv[2] = "relative-root".into();
    assert!(event(&current, &refs).is_none());
    current = cmd.clone();
    current
        .raw_argv
        .splice(3..3, ["-c".into(), "core.bare=false".into()]);
    assert!(event(&current, &refs).is_none());
    current = cmd.clone();
    current.exit_code = 1;
    assert!(event(&current, &refs).is_none());
    for receipt in [IndexWriteEvidence::Missing, IndexWriteEvidence::Conflicting] {
        current = cmd.clone();
        current.index_write = receipt;
        assert!(event(&current, &refs).is_none());
    }
    assert!(event(&cmd, &HashMap::new()).is_none());
    assert!(event(&cmd, &HashMap::from([("HEAD".into(), "0".repeat(40))])).is_none());
    assert!(event(&cmd, &HashMap::from([("HEAD".into(), "main".into())])).is_none());
}

#[test]
fn old_serialized_commands_cannot_acquire_move_evidence() {
    let (cmd, refs) = fixture();
    let mut value = serde_json::to_value(&cmd).unwrap();
    value.as_object_mut().unwrap().remove("index_write");
    let old: NormalizedCommand = serde_json::from_value(value).unwrap();
    assert_eq!(old.index_write, IndexWriteEvidence::Missing);
    assert!(event(&old, &refs).is_none());
    let restored: NormalizedCommand =
        serde_json::from_value(serde_json::to_value(&cmd).unwrap()).unwrap();
    assert_eq!(event(&restored, &refs), event(&cmd, &refs));
}
