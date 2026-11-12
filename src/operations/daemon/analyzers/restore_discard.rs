use super::literal_path::proven_literal_worktree_path;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand, SemanticEvent};
use crate::model::git_oid::is_non_zero_oid;
use crate::operations::git::cli_parser::parse_git_cli_args;
use std::collections::HashMap;

/// The discard event for a successful `command` that reset one literal path to
/// HEAD in both the index and the worktree. `source_and_path` picks the source
/// revision and the path out of the command's arguments.
pub(super) fn head_path_discard_event(
    cmd: &NormalizedCommand,
    refs: &HashMap<String, String>,
    command: &str,
    source_and_path: fn(&[String]) -> Option<(&str, &str)>,
) -> Option<SemanticEvent> {
    if cmd.exit_code != 0
        || cmd.raw_argv.is_empty()
        || !matches!(cmd.index_write, IndexWriteEvidence::Exact(_))
    {
        return None;
    }
    let worktree = cmd.worktree.as_deref()?;
    let head = refs.get("HEAD").filter(|head| is_non_zero_oid(head))?;
    let parsed = parse_git_cli_args(&super::normalized_args(&cmd.raw_argv));
    if parsed.command.as_deref() != Some(command) {
        return None;
    }
    let (source, path) = source_and_path(&parsed.command_args)?;
    if source != head {
        return None;
    }

    let path = proven_literal_worktree_path(&parsed.global_args, worktree, path)?;
    Some(SemanticEvent::WorkingLogPathDiscarded {
        base_commit: head.clone(),
        path,
    })
}

pub(crate) fn event(
    cmd: &NormalizedCommand,
    refs: &HashMap<String, String>,
) -> Option<SemanticEvent> {
    head_path_discard_event(cmd, refs, "restore", |args| match args {
        // A worktree-only restore can leave the same edit staged. Discarding
        // its evidence would misattribute a later commit of that index entry.
        [
            source_flag,
            source,
            staged_flag,
            worktree_flag,
            separator,
            path,
        ] if source_flag == "--source"
            && staged_flag == "--staged"
            && worktree_flag == "--worktree"
            && separator == "--" =>
        {
            Some((source, path))
        }
        [source_flag, staged_flag, worktree_flag, separator, path]
            if staged_flag == "--staged" && worktree_flag == "--worktree" && separator == "--" =>
        {
            Some((source_flag.strip_prefix("--source=")?, path))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::Confidence;
    use crate::operations::daemon::checkout_discard::is_explicit_path_checkout;

    type Event = fn(&NormalizedCommand, &HashMap<String, String>) -> Option<SemanticEvent>;
    type Case = (Event, NormalizedCommand, HashMap<String, String>);

    /// `git -C <root> <command> <args..>` resetting `file.txt` to HEAD, with
    /// exact index-write evidence.
    fn valid(command: &str, args: &[&str]) -> (NormalizedCommand, HashMap<String, String>) {
        let head = "a".repeat(40);
        let root = std::env::temp_dir().join(format!("{command}-root"));
        let mut raw_argv = vec!["git", "-C", root.to_str().unwrap(), command];
        raw_argv.extend(
            args.iter()
                .map(|arg| if *arg == "HEAD" { &head } else { *arg }),
        );
        let cmd = NormalizedCommand {
            worktree: Some(root.clone()),
            root_sid: command.into(),
            trace_derived: true,
            index_write: IndexWriteEvidence::Exact(root.join(".git/index.lock")),
            confidence: Confidence::High,
            invoked_args: Vec::new(),
            ..super::super::tests::command(command, &raw_argv)
        };
        (cmd, HashMap::from([("HEAD".into(), head)]))
    }

    fn valid_restore() -> (NormalizedCommand, HashMap<String, String>) {
        valid(
            "restore",
            &[
                "--source",
                "HEAD",
                "--staged",
                "--worktree",
                "--",
                "file.txt",
            ],
        )
    }

    fn valid_checkout() -> (NormalizedCommand, HashMap<String, String>) {
        valid("checkout", &["HEAD", "--", "file.txt"])
    }

    fn cases() -> [Case; 2] {
        let (restore, restore_refs) = valid_restore();
        let (checkout, checkout_refs) = valid_checkout();
        [
            (event, restore, restore_refs),
            (
                super::super::checkout_discard::event,
                checkout,
                checkout_refs,
            ),
        ]
    }

    #[test]
    fn discard_accepts_proven_absolute_and_root_relative_paths() {
        for (event, original, refs) in cases() {
            let mut cmd = original.clone();
            assert!(event(&cmd, &refs).is_some());
            let absolute = cmd.worktree.as_ref().unwrap().join("file.txt");
            *cmd.raw_argv.last_mut().unwrap() = absolute.to_str().unwrap().to_string();
            cmd.raw_argv.drain(1..3);
            assert!(event(&cmd, &refs).is_some());
            let mut cmd = original;
            cmd.raw_argv.insert(1, "--literal-pathspecs".into());
            *cmd.raw_argv.last_mut().unwrap() = "bracket[1].txt".into();
            assert!(event(&cmd, &refs).is_some());
        }
    }

    #[test]
    fn discard_replayed_commands_require_recorded_index_evidence() {
        for (event, cmd, refs) in cases() {
            let mut serialized = serde_json::to_value(&cmd).unwrap();
            serialized.as_object_mut().unwrap().remove("index_write");
            let old: NormalizedCommand = serde_json::from_value(serialized).unwrap();
            assert_eq!(old.index_write, IndexWriteEvidence::Missing);
            assert!(event(&old, &refs).is_none());
            for receipt in [cmd.index_write.clone(), IndexWriteEvidence::Conflicting] {
                let mut current = cmd.clone();
                current.index_write = receipt.clone();
                let replayed: NormalizedCommand =
                    serde_json::from_slice(&serde_json::to_vec(&current).unwrap()).unwrap();
                assert_eq!(replayed.index_write, receipt);
                assert_eq!(event(&replayed, &refs), event(&current, &refs));
            }
        }
    }

    #[test]
    fn discard_handles_backslashes_using_native_path_semantics() {
        for (event, mut cmd, refs) in cases() {
            *cmd.raw_argv.last_mut().unwrap() = "dir\\file".into();
            #[cfg(windows)]
            assert!(matches!(
                event(&cmd, &refs),
                Some(SemanticEvent::WorkingLogPathDiscarded { path, .. }) if path == "dir/file"
            ));
            #[cfg(not(windows))]
            assert!(event(&cmd, &refs).is_none());
        }
    }

    #[test]
    fn discard_refuses_ambiguous_or_unrepresentable_paths() {
        for (event, original, refs) in cases() {
            for path in [
                "../file",
                ":(top,literal)file",
                "dir/../file",
                "dir//file",
                "file\nnext",
                "file\r",
                "file\0",
                "file\u{fffd}",
                "",
                "*.txt",
                "bracket[1].txt",
            ] {
                let mut cmd = original.clone();
                *cmd.raw_argv.last_mut().unwrap() = path.into();
                assert!(event(&cmd, &refs).is_none(), "{path:?}");
            }
            let mut cmd = original.clone();
            *cmd.raw_argv.last_mut().unwrap() = "x".repeat(4097);
            assert!(event(&cmd, &refs).is_none());
            cmd = original;
            cmd.raw_argv.drain(1..3);
            assert!(event(&cmd, &refs).is_none());
        }
    }

    #[test]
    fn restore_discard_skips_incomplete_modes() {
        let (original, refs) = valid_restore();
        for omitted in ["--staged", "--worktree", "--source", "--"] {
            let mut cmd = original.clone();
            cmd.raw_argv.retain(|arg| arg != omitted);
            assert!(event(&cmd, &refs).is_none(), "{omitted}");
        }
    }

    #[test]
    fn checkout_discard_skips_incomplete_modes() {
        let (original, refs) = valid_checkout();
        let head = refs.get("HEAD").unwrap().as_str();
        for args in [
            vec![head, "file.txt"],
            vec![head, "other.txt", "file.txt"],
            vec!["--", "file.txt"],
            vec!["--ours", "--", "file.txt"],
            vec!["--theirs", "--", "file.txt"],
            vec!["--patch", head, "--", "file.txt"],
            vec!["--force", head, "--", "file.txt"],
            vec![head, "--pathspec-from-file=paths"],
        ] {
            let mut cmd = original.clone();
            cmd.raw_argv.truncate(4);
            cmd.raw_argv.extend(args.iter().map(|arg| arg.to_string()));
            assert!(!is_explicit_path_checkout(&cmd), "{args:?}");
            assert!(
                super::super::checkout_discard::event(&cmd, &refs).is_none(),
                "{args:?}"
            );
        }
    }

    #[test]
    fn discard_skips_other_sources_globals_and_missing_index_evidence() {
        for (event, original, refs) in cases() {
            for globals in [
                vec!["-c", "core.quotePath=false"],
                vec!["--git-dir=other"],
                vec!["--work-tree=other"],
            ] {
                let mut cmd = original.clone();
                cmd.raw_argv
                    .splice(1..1, globals.into_iter().map(String::from));
                assert!(event(&cmd, &refs).is_none());
            }
            for receipt in [IndexWriteEvidence::Missing, IndexWriteEvidence::Conflicting] {
                let mut cmd = original.clone();
                cmd.index_write = receipt;
                assert!(event(&cmd, &refs).is_none());
            }
            for value in ["b".repeat(40), "0".repeat(40), "invalid".into()] {
                assert!(event(&original, &HashMap::from([("HEAD".into(), value)])).is_none());
            }
            assert!(event(&original, &HashMap::new()).is_none());
            let mut cmd = original.clone();
            cmd.exit_code = 1;
            assert!(event(&cmd, &refs).is_none());
            cmd = original.clone();
            cmd.worktree = None;
            assert!(event(&cmd, &refs).is_none());
            cmd = original;
            cmd.raw_argv.clear();
            assert!(event(&cmd, &refs).is_none());
        }
    }
}
