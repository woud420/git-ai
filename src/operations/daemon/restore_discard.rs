use super::working_log_discard::remove_matching_attributions as remove_working_log_attributions_matching;
use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::model::domain::IndexWriteEvidence;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::refs::parse_batch_check_blob_oid;

pub(super) fn apply(
    worktree: &str,
    head: &str,
    path: &str,
    index_write: &IndexWriteEvidence,
) -> Result<(), GitAiError> {
    let repo = find_repository_in_path(worktree)?;
    if !repo.is_collection_allowed(&crate::config::Config::fresh()) {
        return Ok(());
    }
    // Both restore destinations can still refer to an alternate index. Only
    // Git's recorded write to this worktree's index proves default staged
    // evidence was discarded too; inspecting the later index cannot prove it.
    if !matches!(index_write, IndexWriteEvidence::Exact(path) if path == &repo.path().join("index.lock"))
    {
        return Ok(());
    }
    if !repo.storage.has_working_log(head) {
        return Ok(());
    }
    // A directory restore can skip descendants. Require an exact immutable blob
    // before treating the successful command as evidence that this path changed.
    let mut args = repo.global_args_for_exec();
    args.extend([
        "--no-replace-objects".to_string(),
        "cat-file".to_string(),
        "--batch-check=%(objectname) %(objecttype)".to_string(),
    ]);
    let output = exec_git_stdin(&args, format!("{head}:{path}\n").as_bytes())?;
    let stdout = String::from_utf8(output.stdout)?;
    let mut records = stdout.lines();
    if records
        .next()
        .and_then(parse_batch_check_blob_oid)
        .is_some()
        && records.next().is_none()
    {
        // Match only this file: descendant evidence can belong to a different
        // path shape, including one exposed through replacement objects.
        remove_working_log_attributions_matching(&repo, head, |file| file == path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::NormalizedCommand;
    use crate::model::domain::{CommandScope, Confidence};
    use crate::operations::daemon::analyzers::restore_discard::event;
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
