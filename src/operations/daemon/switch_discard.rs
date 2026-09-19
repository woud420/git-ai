use super::working_log_discard::remove_matching_attributions as remove_working_log_attributions_matching;
use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand};
use crate::operations::git::refs::parse_batch_check_blob_oid;
use crate::operations::git::repository::Repository;
use std::collections::BTreeSet;

const MAX_PATHS: usize = 4096;
const MAX_QUERY_BYTES: usize = 1024 * 1024;

pub(super) enum Target {
    Branch(String),
    Detached,
}

pub(super) fn target(cmd: &NormalizedCommand) -> Option<Target> {
    use super::side_effect_helpers::parsed_invocation_for_normalized_command;
    use crate::operations::git::oid::is_non_zero_oid;

    if cmd.exit_code != 0 || cmd.raw_argv.is_empty() {
        return None;
    }
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if parsed.command.as_deref() != Some("switch") {
        return None;
    }
    // A replacement tree can make a canonical tracked path remain untracked.
    // Require the command itself to disable replacements; later refs cannot
    // establish which object view Git used when it discarded the edits.
    let mut canonical_objects = false;
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "--no-replace-objects" => canonical_objects = true,
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return None,
        }
    }
    if !canonical_objects {
        return None;
    }
    let mut force = false;
    let mut detached = false;
    let mut destination = None;
    for arg in &parsed.command_args {
        match arg.as_str() {
            "--discard-changes" | "--force" | "-f" => force = true,
            "--detach" | "-d" => detached = true,
            "--quiet" | "-q" | "--no-guess" => {}
            value if !value.starts_with('-') && destination.is_none() => {
                destination = Some(value);
            }
            _ => return None,
        }
    }
    let destination = destination.filter(|_| force)?;
    if detached {
        return is_non_zero_oid(destination).then_some(Target::Detached);
    }
    // Limit symbolic targets to literal branch names; revision expressions and
    // previous-branch selectors do not prove the new branch identity.
    if destination.is_empty()
        || destination.len() > 4096
        || !destination
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
        || destination.contains("..")
        || destination == "HEAD"
        || destination.starts_with("refs/") && !destination.starts_with("refs/heads/")
    {
        return None;
    }
    Some(Target::Branch(if destination.starts_with("refs/heads/") {
        destination.to_owned()
    } else {
        format!("refs/heads/{destination}")
    }))
}

pub(super) fn apply(
    repo: &Repository,
    head: &str,
    cmd: &NormalizedCommand,
) -> Result<(), GitAiError> {
    if !repo.is_collection_allowed(&crate::config::Config::fresh()) {
        return Ok(());
    }
    // Version 2 cannot encode skip-worktree entries. Later index reads cannot
    // establish whether the switch actually discarded a sparse working edit.
    if !cmd.index_v2
        || !matches!(&cmd.index_write, IndexWriteEvidence::Exact(path) if path == &repo.path().join("index.lock"))
        || !repo.storage.has_working_log(head)
    {
        return Ok(());
    }
    let working_log = repo.storage.working_log_for_base_commit(head)?;
    let initial = working_log.read_initial_attributions();
    let checkpoints = working_log.read_all_checkpoints()?;
    let paths: BTreeSet<_> = initial
        .files
        .keys()
        .chain(
            checkpoints
                .iter()
                .flat_map(|checkpoint| checkpoint.entries.iter().map(|entry| &entry.file)),
        )
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() > MAX_PATHS
        || paths.iter().any(|path| path.contains(['\n', '\r', '\0']))
        || paths
            .iter()
            .map(|path| head.len() + path.len() + 2)
            .sum::<usize>()
            > MAX_QUERY_BYTES
    {
        tracing::warn!(
            head,
            "same-OID switch attribution skipped: path query budget or encoding"
        );
        return Ok(());
    }

    // The index/worktree may already contain later edits. Resolve only recorded
    // paths in the command's immutable tree, without reading blob contents.
    let mut args = repo.global_args_for_exec();
    args.extend([
        "--no-replace-objects".to_string(),
        "cat-file".to_string(),
        "--batch-check=%(objectname) %(objecttype)".to_string(),
    ]);
    let input = paths
        .iter()
        .map(|path| format!("{head}:{path}\n"))
        .collect::<String>();
    let output = exec_git_stdin(&args, input.as_bytes())?;
    let stdout = String::from_utf8(output.stdout)?;
    let records: Vec<_> = stdout.lines().collect();
    if records.len() != paths.len() {
        tracing::warn!(
            head,
            "same-OID switch attribution skipped: incomplete path evidence"
        );
        return Ok(());
    }
    let tracked: BTreeSet<_> = paths
        .into_iter()
        .zip(records)
        .filter(|(_, record)| parse_batch_check_blob_oid(record).is_some())
        .map(|(path, _)| path.clone())
        .collect();
    if !tracked.is_empty() {
        // The family sequencer applies this before later checkpoint writes;
        // retaining other paths preserves untracked files that Git left intact.
        remove_working_log_attributions_matching(repo, head, |path| tracked.contains(path))?;
    }
    Ok(())
}
