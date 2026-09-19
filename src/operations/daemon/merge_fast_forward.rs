use super::side_effect_helpers::parsed_invocation_for_normalized_command;
use crate::model::domain::NormalizedCommand;
use crate::operations::git::oid::is_non_zero_oid;

pub(super) fn has_explicit_fast_forward_transition(cmd: &NormalizedCommand) -> bool {
    if cmd.exit_code != 0
        || cmd.primary_command.as_deref() != Some("merge")
        || cmd.raw_argv.is_empty()
    {
        return false;
    }
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if parsed.command.as_deref() != Some("merge") {
        return false;
    }
    let [first, second] = parsed.command_args.as_slice() else {
        return false;
    };
    if !((first == "--ff-only" && !second.is_empty() && !second.starts_with('-'))
        || (second == "--ff-only" && !first.is_empty() && !first.starts_with('-')))
    {
        return false;
    }

    // The worktree identity already accounts for -C. Other globals can change
    // the object view and are not replayed by the existing migration helper.
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return false,
        }
    }

    // An ancestry check alone also accepts newly created merge commits. Require
    // the explicit mode plus one command-owned HEAD transition; never guess from
    // a branch ref or from HEAD at asynchronous processing time.
    let mut heads = cmd
        .ref_changes
        .iter()
        .filter(|change| change.reference == "HEAD");
    let Some(head) = heads.next() else {
        return false;
    };
    heads.next().is_none()
        && head.old != head.new
        && is_non_zero_oid(&head.old)
        && is_non_zero_oid(&head.new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::{CommandScope, Confidence, RefChange};

    fn command(argv: &[&str]) -> NormalizedCommand {
        NormalizedCommand {
            scope: CommandScope::Global,
            family_key: None,
            worktree: None,
            root_sid: "merge-guard".into(),
            trace_derived: false,
            raw_argv: argv.iter().map(|s| s.to_string()).collect(),
            primary_command: Some("merge".into()),
            invoked_command: Some("merge".into()),
            invoked_args: Vec::new(),
            observed_child_commands: Vec::new(),
            transport_targets: None,
            exit_code: 0,
            started_at_ns: 1,
            finished_at_ns: 2,
            reflog_start_offsets: Default::default(),
            stash_target_oid: None,
            cherry_pick_source_oids: Vec::new(),
            revert_source_oids: Vec::new(),
            ref_changes: vec![RefChange {
                reference: "HEAD".into(),
                old: "a".repeat(40),
                new: "b".repeat(40),
            }],
            confidence: Confidence::High,
        }
    }

    #[test]
    fn accepts_explicit_target_and_directory_variants() {
        for argv in [
            vec!["git", "merge", "--ff-only", "feature"],
            vec!["git", "merge", "feature", "--ff-only"],
            vec!["git", "-C", "subdir", "merge", "--ff-only", "feature"],
            vec!["git", "-Csubdir", "merge", "--ff-only", "feature"],
        ] {
            assert!(
                has_explicit_fast_forward_transition(&command(&argv)),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn leaves_other_modes_and_global_overrides_unchanged() {
        for argv in [
            vec!["git", "merge", "feature"],
            vec!["git", "merge", "--ff-only"],
            vec!["git", "merge", "--ff-only", ""],
            vec!["git", "merge", "--ff-only", "--help"],
            vec!["git", "merge", "--ff-only", "one", "two"],
            vec!["git", "merge", "--no-ff", "feature"],
            vec!["git", "merge", "--ff-only", "--no-ff", "feature"],
            vec!["git", "merge", "--ff-only", "--no-commit", "feature"],
            vec!["git", "merge", "--ff-only", "--squash", "feature"],
            vec!["git", "merge", "--ff-only", "--autostash", "feature"],
            vec!["git", "merge", "--continue"],
            vec!["git", "merge", "--abort"],
            vec![
                "git",
                "-c",
                "merge.ff=only",
                "merge",
                "--ff-only",
                "feature",
            ],
            vec!["git", "--git-dir=other", "merge", "--ff-only", "feature"],
            vec![
                "git",
                "--no-replace-objects",
                "merge",
                "--ff-only",
                "feature",
            ],
            vec!["git", "pull", "--ff-only", "feature"],
        ] {
            assert!(
                !has_explicit_fast_forward_transition(&command(&argv)),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn refuses_missing_ambiguous_invalid_and_failed_evidence() {
        let original = command(&["git", "merge", "--ff-only", "feature"]);
        let mut cmd = original.clone();
        cmd.ref_changes.clear();
        assert!(!has_explicit_fast_forward_transition(&cmd));
        cmd = original.clone();
        cmd.ref_changes[0].reference = "refs/heads/main".into();
        assert!(!has_explicit_fast_forward_transition(&cmd));
        cmd = original.clone();
        cmd.ref_changes.push(cmd.ref_changes[0].clone());
        assert!(!has_explicit_fast_forward_transition(&cmd));
        for old in [
            String::new(),
            "0".repeat(40),
            "invalid".into(),
            "b".repeat(40),
        ] {
            cmd = original.clone();
            cmd.ref_changes[0].old = old;
            assert!(!has_explicit_fast_forward_transition(&cmd));
        }
        cmd = original.clone();
        cmd.exit_code = 1;
        assert!(!has_explicit_fast_forward_transition(&cmd));
        cmd = original;
        cmd.raw_argv.clear();
        assert!(!has_explicit_fast_forward_transition(&cmd));
    }
}
