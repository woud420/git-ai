use crate::error::GitAiError;
use crate::model::domain::{NormalizedCommand, SemanticEvent};
use crate::operations::daemon::side_effect_helpers::parsed_invocation_for_normalized_command;
use crate::operations::git::cli_parser::is_dry_run;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::oid::is_non_zero_oid;
use crate::operations::git::sync_authorship::{fetch_authorship_notes, fetch_remote_from_args};
use std::collections::HashSet;

pub(super) fn sync_before_rewrite(
    cmd: &NormalizedCommand,
    events: &[SemanticEvent],
) -> Result<(), GitAiError> {
    let Some(worktree) = cmd.worktree.as_ref().filter(|_| cmd.exit_code == 0) else {
        return Ok(());
    };
    let worktree = worktree.to_string_lossy();
    if events
        .iter()
        .any(|event| matches!(event, SemanticEvent::PullCompleted { .. }))
    {
        apply_transport_notes_sync_side_effect(&worktree, cmd)
    } else {
        if events
            .iter()
            .any(|event| matches!(event, SemanticEvent::FetchCompleted { .. }))
        {
            apply_fetch_notes_sync_side_effect(&worktree, cmd);
        }
        Ok(())
    }
}

fn incoming_revision_oids(cmd: &NormalizedCommand, remote: &str) -> Vec<String> {
    let prefix = format!("refs/remotes/{remote}/");
    let mut seen = HashSet::new();
    let changed = || {
        cmd.ref_changes
            .iter()
            .filter(|change| change.old != change.new && is_non_zero_oid(&change.new))
    };
    let mut revisions: Vec<String> = changed()
        .filter(|change| change.reference.starts_with(&prefix))
        .filter(|change| seen.insert(change.new.as_str()))
        .map(|change| change.new.clone())
        .collect();
    // URL/path pulls may have no tracking ref; their recorded HEAD transitions
    // still identify immutable incoming history. Never resolve the current HEAD.
    if revisions.is_empty() && cmd.primary_command.as_deref() == Some("pull") {
        revisions.extend(
            changed()
                .filter(|change| change.reference == "HEAD")
                .filter(|change| seen.insert(change.new.as_str()))
                .map(|change| change.new.clone()),
        );
    }
    revisions
}

fn apply_fetch_notes_sync_side_effect(worktree: &str, cmd: &NormalizedCommand) {
    if let Some(source) = explicit_fetch_pack_source(cmd) {
        if let Err(error) =
            crate::operations::git::sync_authorship::fetch_authorship_notes_from_repository(
                worktree, &source,
            )
        {
            tracing::warn!(remote = %source, %error, "best-effort fetch-pack notes sync failed");
        }
        return;
    }
    let Some(remote) = explicit_fetch_remote(cmd) else {
        return;
    };
    if let Err(error) = apply_transport_notes_sync_side_effect(worktree, cmd) {
        tracing::warn!(%remote, %error, "best-effort fetch notes sync failed");
    }
}


// A standalone `fetch-pack --all <absolute path>` names its one source
// explicitly; other forms can select refs or remotes we must not guess.
fn explicit_fetch_pack_source(cmd: &NormalizedCommand) -> Option<String> {
    if cmd.raw_argv.is_empty() {
        return None;
    }
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if parsed.command.as_deref() != Some("fetch-pack") {
        return None;
    }
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return None,
        }
    }
    match parsed.command_args.as_slice() {
        [all, source] if all == "--all" && std::path::Path::new(source).is_absolute() => {
            Some(source.clone())
        }
        _ => None,
    }
}

pub(super) fn explicit_fetch_remote(cmd: &NormalizedCommand) -> Option<String> {
    if cmd.raw_argv.is_empty() {
        return None;
    }
    let parsed = parsed_invocation_for_normalized_command(cmd);
    let [remote] = parsed.command_args.as_slice() else {
        return None;
    };
    // More complex forms can select multiple remotes or carry option values.
    // Do not guess their destination from mutable config after the command.
    if parsed.command.as_deref() != Some("fetch") || remote.starts_with('-') {
        return None;
    }
    // Normalization already resolved -C. Other globals can redirect transport
    // or repository selection and are not preserved by the notes sync helper.
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return None,
        }
    }
    Some(remote.clone())
}

fn apply_transport_notes_sync_side_effect(
    worktree: &str,
    cmd: &NormalizedCommand,
) -> Result<(), GitAiError> {
    use crate::config::NotesBackendKind;

    let command = cmd.primary_command.as_deref();
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if is_dry_run(&parsed.command_args) {
        return Ok(());
    }
    let repo = find_repository_in_path(worktree)?;
    let config = crate::config::Config::fresh();
    if command == Some("fetch") && !repo.is_collection_allowed(&config) {
        return Ok(());
    }
    let remote = fetch_remote_from_args(&repo, &parsed)?;
    let notes_backend = config.notes_backend_kind();

    tracing::info!(
        command = command.unwrap_or("pull"),
        remote = %remote,
        backend = %notes_backend,
        worktree = %worktree,
        "handling pull notes sync"
    );

    if notes_backend == NotesBackendKind::Http {
        return crate::operations::git::notes_api::warm_cache_for_revisions(
            &repo,
            &incoming_revision_oids(cmd, &remote),
        );
    }

    fetch_authorship_notes(&repo, &remote)?;
    Ok(())
}
