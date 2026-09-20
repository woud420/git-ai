use super::literal_path::proven_literal_worktree_path;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand, SemanticEvent};
use crate::model::git_oid::is_non_zero_oid;
use crate::operations::git::cli_parser::parse_git_cli_args;
use std::collections::HashMap;

pub(crate) fn event(
    cmd: &NormalizedCommand,
    refs: &HashMap<String, String>,
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
    if parsed.command.as_deref() != Some("restore") {
        return None;
    }
    let (source, path) = match parsed.command_args.as_slice() {
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
            (source.as_str(), path.as_str())
        }
        [source_flag, staged_flag, worktree_flag, separator, path]
            if staged_flag == "--staged" && worktree_flag == "--worktree" && separator == "--" =>
        {
            (source_flag.strip_prefix("--source=")?, path.as_str())
        }
        _ => return None,
    };
    if source != head {
        return None;
    }

    let path = proven_literal_worktree_path(&parsed.global_args, worktree, path)?;
    Some(SemanticEvent::WorkingLogPathDiscarded {
        base_commit: head.clone(),
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::NormalizedCommand;
    use crate::model::domain::{CommandScope, Confidence};
    use std::collections::HashMap;

    fn command(argv: &[&str]) -> NormalizedCommand {
        NormalizedCommand {
            scope: CommandScope::Global,
            family_key: None,
            worktree: Some("/repo".into()),
            root_sid: "restore".into(),
            trace_derived: true,
            raw_argv: argv.iter().map(|s| s.to_string()).collect(),
            primary_command: Some("restore".into()),
            invoked_command: Some("restore".into()),
            invoked_args: Vec::new(),
            observed_child_commands: Vec::new(),
            transport_targets: None,
            index_v2: false,
            index_write: Default::default(),
            exit_code: 0,
            started_at_ns: 1,
            finished_at_ns: 2,
            reflog_start_offsets: HashMap::new(),
            stash_target_oid: None,
            cherry_pick_source_oids: Vec::new(),
            revert_source_oids: Vec::new(),
            ref_changes: Vec::new(),
            confidence: Confidence::High,
        }
    }

    fn valid() -> (NormalizedCommand, HashMap<String, String>) {
        let head = "a".repeat(40);
        let root = std::env::temp_dir().join("restore-root");
        let mut cmd = command(&[
            "git",
            "-C",
            root.to_str().unwrap(),
            "restore",
            "--source",
            &head,
            "--staged",
            "--worktree",
            "--",
            "file.txt",
        ]);
        cmd.worktree = Some(root.clone());
        cmd.index_write = IndexWriteEvidence::Exact(root.join(".git/index.lock"));
        (cmd, HashMap::from([("HEAD".into(), head)]))
    }

    #[test]
    fn restore_discard_accepts_proven_absolute_and_root_relative_paths() {
        let (mut cmd, refs) = valid();
        assert!(event(&cmd, &refs).is_some());
        let absolute = cmd.worktree.as_ref().unwrap().join("file.txt");
        *cmd.raw_argv.last_mut().unwrap() = absolute.to_str().unwrap().to_string();
        cmd.raw_argv.drain(1..3);
        assert!(event(&cmd, &refs).is_some());
        let (mut cmd, refs) = valid();
        cmd.raw_argv.insert(1, "--literal-pathspecs".into());
        *cmd.raw_argv.last_mut().unwrap() = "bracket[1].txt".into();
        assert!(event(&cmd, &refs).is_some());
    }

    #[test]
    fn restore_discard_replayed_commands_require_recorded_index_evidence() {
        let (cmd, refs) = valid();
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

    #[test]
    fn restore_discard_handles_backslashes_using_native_path_semantics() {
        let (mut cmd, refs) = valid();
        *cmd.raw_argv.last_mut().unwrap() = "dir\\file".into();
        #[cfg(windows)]
        assert!(matches!(
            event(&cmd, &refs),
            Some(crate::model::domain::SemanticEvent::WorkingLogPathDiscarded { path, .. })
                if path == "dir/file"
        ));
        #[cfg(not(windows))]
        assert!(event(&cmd, &refs).is_none());
    }

    #[test]
    fn restore_discard_refuses_ambiguous_or_unrepresentable_paths() {
        let (original, refs) = valid();
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

    #[test]
    fn restore_discard_skips_incomplete_modes_sources_globals_and_index_evidence() {
        let (original, refs) = valid();
        for omitted in ["--staged", "--worktree", "--source", "--"] {
            let mut cmd = original.clone();
            cmd.raw_argv.retain(|arg| arg != omitted);
            assert!(event(&cmd, &refs).is_none(), "{omitted}");
        }
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
