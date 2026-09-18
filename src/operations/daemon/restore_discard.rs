use super::working_log_discard::remove_matching_attributions as remove_working_log_attributions_matching;
use super::side_effect_helpers::parsed_invocation_for_normalized_command;
use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::model::domain::{NormalizedCommand, SemanticEvent};
use crate::operations::git::find_repository_in_path;
use crate::operations::git::oid::is_non_zero_oid;
use crate::operations::git::refs::parse_batch_check_blob_oid;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn event(
    cmd: &NormalizedCommand,
    refs: &HashMap<String, String>,
) -> Option<SemanticEvent> {
    if cmd.exit_code != 0 || cmd.raw_argv.is_empty() {
        return None;
    }
    let worktree = cmd.worktree.as_deref()?;
    let head = refs.get("HEAD").filter(|head| is_non_zero_oid(head))?;
    let parsed = parsed_invocation_for_normalized_command(cmd);
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

    let mut root_cwd_proven = false;
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        let directory = if arg == "-C" {
            globals.next()?.as_str()
        } else {
            arg.strip_prefix("-C").filter(|value| !value.is_empty())?
        };
        root_cwd_proven = Path::new(directory).is_absolute() && Path::new(directory) == worktree;
    }
    let path = if let Some(path) = path.strip_prefix(":(top,literal)") {
        path
    } else if root_cwd_proven && !path.contains(['*', '?', '[', ']', ':']) {
        path
    } else {
        return None;
    };
    // Keep batch framing and root-relative path interpretation unambiguous.
    if path.len() > 4096
        || path.contains(['\n', '\r', '\0', '\\', '\u{fffd}'])
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return None;
    }
    Some(SemanticEvent::WorkingLogPathDiscarded {
        base_commit: head.clone(),
        path: path.to_string(),
    })
}

pub(super) fn apply(worktree: &str, head: &str, path: &str) -> Result<(), GitAiError> {
    let repo = find_repository_in_path(worktree)?;
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
    use crate::model::domain::{CommandScope, Confidence};

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

    #[test]
    fn restore_discard_accepts_only_proven_root_relative_paths() {
        let head = "a".repeat(40);
        let refs = HashMap::from([("HEAD".into(), head.clone())]);
        for globals in [vec![], vec!["-C", "relative"], vec!["-C/repo/nested"]] {
            let mut argv = vec!["git"];
            argv.extend(globals);
            argv.extend([
                "restore",
                "--source",
                &head,
                "--staged",
                "--worktree",
                "--",
                ":(top,literal)dir/a[1].txt",
            ]);
            assert_eq!(
                event(&command(&argv), &refs),
                Some(SemanticEvent::WorkingLogPathDiscarded {
                    base_commit: head.clone(),
                    path: "dir/a[1].txt".into(),
                })
            );
        }
        let source = format!("--source={head}");
        let root = std::env::temp_dir().join("restore-root");
        let argv = [
            "git",
            "-C",
            root.to_str().unwrap(),
            "restore",
            &source,
            "--staged",
            "--worktree",
            "--",
            "dir/file.txt",
        ];
        let mut cmd = command(&argv);
        cmd.worktree = Some(root);
        assert!(event(&cmd, &refs).is_some());
        for path in [
            "file.txt",
            "../file",
            "/absolute",
            ":(top,literal)..",
            ":(top,literal)dir/../file",
            ":(top,literal)dir//file",
            ":(top,literal)file\nnext",
            ":(top,literal)file\r",
            ":(top,literal)file\0",
            ":(top,literal)dir\\file",
            ":(top,literal)file\u{fffd}",
            ":(top,literal)/file",
            ":(top,literal)",
        ] {
            let argv = [
                "git",
                "restore",
                "--source",
                &head,
                "--staged",
                "--worktree",
                "--",
                path,
            ];
            assert!(event(&command(&argv), &refs).is_none(), "{path:?}");
        }
        let path = format!(":(top,literal){}", "x".repeat(4097));
        assert!(
            event(
                &command(&[
                    "git",
                    "restore",
                    "--source",
                    &head,
                    "--staged",
                    "--worktree",
                    "--",
                    &path
                ]),
                &refs
            )
            .is_none()
        );
    }

    #[test]
    fn restore_discard_skips_incomplete_modes_sources_and_globals() {
        let head = "a".repeat(40);
        let refs = HashMap::from([("HEAD".into(), head.clone())]);
        for args in [
            vec!["--source", &head, "--worktree", "--", ":(top,literal)file"],
            vec!["--source", &head, "--staged", "--", ":(top,literal)file"],
            vec![
                "--source",
                "HEAD",
                "--staged",
                "--worktree",
                "--",
                ":(top,literal)file",
            ],
            vec!["--staged", "--worktree", "--", ":(top,literal)file"],
            vec![
                "--source",
                &head,
                "--staged",
                "--worktree",
                "--patch",
                "--",
                ":(top,literal)file",
            ],
            vec![
                "--source",
                &head,
                "--staged",
                "--worktree",
                "--",
                ":(top,literal)file",
                "other",
            ],
            vec![
                "--source",
                &head,
                "--staged",
                "--worktree",
                "--pathspec-from-file=paths",
            ],
        ] {
            let mut argv = vec!["git", "restore"];
            argv.extend(args);
            assert!(event(&command(&argv), &refs).is_none(), "{argv:?}");
        }
        for globals in [
            vec!["-c", "core.quotePath=false"],
            vec!["--git-dir=other"],
            vec!["--literal-pathspecs"],
            vec!["--work-tree=other"],
        ] {
            let mut argv = vec!["git"];
            argv.extend(globals);
            argv.extend([
                "restore",
                "--source",
                &head,
                "--staged",
                "--worktree",
                "--",
                ":(top,literal)file",
            ]);
            assert!(event(&command(&argv), &refs).is_none(), "{argv:?}");
        }
        let argv = [
            "git",
            "restore",
            "--source",
            &head,
            "--staged",
            "--worktree",
            "--",
            ":(top,literal)file",
        ];
        let mut cmd = command(&argv);
        assert!(event(&cmd, &HashMap::new()).is_none());
        for value in ["b".repeat(40), "0".repeat(40), "invalid".into()] {
            assert!(event(&cmd, &HashMap::from([("HEAD".into(), value)])).is_none());
        }
        cmd.exit_code = 1;
        assert!(event(&cmd, &refs).is_none());
        cmd.exit_code = 0;
        cmd.worktree = None;
        assert!(event(&cmd, &refs).is_none());
        cmd = command(&argv);
        cmd.raw_argv.clear();
        assert!(event(&cmd, &refs).is_none());
    }
}
