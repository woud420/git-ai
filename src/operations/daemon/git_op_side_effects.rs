pub use super::working_log_discard::remove_working_log_attributions_for_pathspecs;
use crate::error::GitAiError;
use crate::operations::daemon::actor_types::{ActorDaemonCoordinator, RecentReplayPrerequisite};
use crate::operations::daemon::side_effect_helpers::{
    parsed_invocation_for_normalized_command, parsed_invocation_for_side_effect,
};
use crate::operations::git::cli_parser::summarize_rebase_args;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::oid::is_non_zero_oid;
pub use crate::operations::git::oid::{is_full_oid as is_valid_oid, is_zero_oid};
use crate::operations::git::repository::Repository;
use crate::operations::git::sync_authorship::{fetch_authorship_notes, fetch_remote_from_args};

pub fn apply_push_side_effect(
    worktree: &str,
    command: Option<&str>,
    args: &[String],
) -> Result<(), GitAiError> {
    use crate::config::NotesBackendKind;
    use crate::operations::git::cli_parser::is_dry_run;
    use crate::operations::git::sync_authorship::{push_authorship_notes, push_remote_from_args};

    if crate::config::Config::get().notes_backend_kind() == NotesBackendKind::Http {
        tracing::debug!("apply_push_side_effect: skipping authorship push (Http backend)");
        return Ok(());
    }

    let repo = find_repository_in_path(worktree)?;
    let parsed = parsed_invocation_for_side_effect(command, args);

    if is_dry_run(&parsed.command_args)
        || parsed
            .command_args
            .iter()
            .any(|a| a == "-d" || a == "--delete")
        || parsed.command_args.iter().any(|a| a == "--mirror")
    {
        return Ok(());
    }

    let remote = push_remote_from_args(&repo, &parsed)?;

    crate::operations::commands::upgrade::maybe_schedule_background_update_check();
    tracing::debug!("started pushing authorship notes to remote: {}", remote);

    push_authorship_notes(&repo, &remote)
}

pub fn transcript_sweep_triggers_for_events(
    events: &[crate::model::domain::SemanticEvent],
) -> Vec<crate::operations::daemon::stream_worker::SweepTrigger> {
    let mut triggers = Vec::new();

    if events.iter().any(|event| {
        matches!(
            event,
            crate::model::domain::SemanticEvent::CommitCreated { .. }
                | crate::model::domain::SemanticEvent::CommitAmended { .. }
        )
    }) {
        triggers.push(crate::operations::daemon::stream_worker::SweepTrigger::PostCommit);
    }

    if events.iter().any(|event| {
        matches!(
            event,
            crate::model::domain::SemanticEvent::PushCompleted { .. }
        )
    }) {
        triggers.push(crate::operations::daemon::stream_worker::SweepTrigger::PostPush);
    }

    triggers
}

pub fn apply_fetch_notes_sync_side_effect(
    worktree: &str,
    cmd: &crate::model::domain::NormalizedCommand,
) {
    if cmd.raw_argv.is_empty() {
        return;
    }
    let parsed = parsed_invocation_for_normalized_command(cmd);
    let [remote] = parsed.command_args.as_slice() else {
        return;
    };
    // More complex forms can select multiple remotes or carry option values.
    // Do not guess their destination from mutable config after the command.
    if parsed.command.as_deref() != Some("fetch") || remote.starts_with('-') {
        return;
    }

    // Normalization already resolved -C. Other globals can redirect transport
    // or repository selection and are not preserved by the notes sync helper.
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return,
        }
    }

    if let Err(error) =
        apply_pull_notes_sync_side_effect(worktree, parsed.command.as_deref(), &parsed.command_args)
    {
        tracing::warn!(%remote, %error, "best-effort fetch notes sync failed");
    }
}

pub fn apply_pull_notes_sync_side_effect(
    worktree: &str,
    command: Option<&str>,
    args: &[String],
) -> Result<(), GitAiError> {
    use crate::config::NotesBackendKind;

    let repo = find_repository_in_path(worktree)?;
    let config = crate::config::Config::fresh();
    if command == Some("fetch") && !repo.is_collection_allowed(&config) {
        return Ok(());
    }
    let parsed = parsed_invocation_for_side_effect(command, args);
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
        return crate::operations::git::notes_api::warm_cache_for_remote(&repo, &remote);
    }

    fetch_authorship_notes(&repo, &remote)?;
    Ok(())
}

pub fn apply_clone_notes_sync_side_effect(worktree: &str) -> Result<(), GitAiError> {
    use crate::config::NotesBackendKind;

    let repo = find_repository_in_path(worktree)?;
    let remote = "origin";
    let notes_backend = crate::config::Config::fresh().notes_backend_kind();

    tracing::info!(
        command = "clone",
        remote = %remote,
        backend = %notes_backend,
        worktree = %worktree,
        "handling clone notes sync"
    );

    if notes_backend == NotesBackendKind::Http {
        return crate::operations::git::notes_api::warm_cache_for_remote(&repo, remote);
    }

    fetch_authorship_notes(&repo, remote)?;
    Ok(())
}

pub fn apply_pull_fast_forward_working_log_side_effect(
    worktree: &str,
    old_head: &str,
    new_head: &str,
) -> Result<(), GitAiError> {
    let repo = find_repository_in_path(worktree)?;
    repo.storage.rename_working_log(old_head, new_head)?;
    Ok(())
}

pub fn apply_checkout_switch_working_log_side_effect(
    cmd: &crate::model::domain::NormalizedCommand,
) -> Result<(), GitAiError> {
    let Some(worktree) = cmd.worktree.as_ref() else {
        return Ok(());
    };
    let repo = find_repository_in_path(&worktree.to_string_lossy())?;
    let parsed = parsed_invocation_for_normalized_command(cmd);
    let (mut old_head, new_head) = ActorDaemonCoordinator::resolve_heads_for_command(cmd);
    if is_zero_oid(&old_head) {
        old_head = "initial".to_string();
    }

    if cmd.primary_command.as_deref() == Some("checkout") {
        let pathspecs = parsed.pathspecs();
        if !pathspecs.is_empty() {
            if !old_head.is_empty() {
                remove_working_log_attributions_for_pathspecs(&repo, &old_head, &pathspecs)?;
            }
            return Ok(());
        }
    }

    if old_head.is_empty() || new_head.is_empty() || old_head == new_head {
        return Ok(());
    }

    let is_merge = parsed.has_command_flag("--merge") || parsed.has_command_flag("-m");
    let is_force = match cmd.primary_command.as_deref() {
        Some("checkout") => parsed.has_command_flag("--force") || parsed.has_command_flag("-f"),
        Some("switch") => {
            parsed.has_command_flag("--discard-changes")
                || parsed.has_command_flag("--force")
                || parsed.has_command_flag("-f")
        }
        _ => false,
    };

    if is_force {
        repo.storage.delete_working_log_for_base_commit(&old_head)?;
        return Ok(());
    }

    if is_merge {
        let final_state =
            crate::operations::authorship::virtual_attribution::checkout_merge_final_state_snapshot(
                &repo, &old_head, &new_head,
            )?;
        if final_state.is_empty() {
            repo.storage.delete_working_log_for_base_commit(&old_head)?;
            return Ok(());
        }
        let author = repo.effective_author_identity().formatted_or_unknown();
        crate::operations::authorship::virtual_attribution::restore_working_log_carryover(
            &repo,
            &old_head,
            &new_head,
            final_state,
            Some(author),
        )?;
        repo.storage.delete_working_log_for_base_commit(&old_head)?;
        return Ok(());
    }

    repo.storage.rename_working_log(&old_head, &new_head)?;
    Ok(())
}

pub fn recent_checkout_switch_prerequisite_from_command(
    cmd: &crate::model::domain::NormalizedCommand,
) -> Option<RecentReplayPrerequisite> {
    let parsed = parsed_invocation_for_normalized_command(cmd);
    let (old_head, new_head) = ActorDaemonCoordinator::resolve_heads_for_command(cmd);

    if old_head.is_empty() || new_head.is_empty() || old_head == new_head {
        return None;
    }

    if cmd.primary_command.as_deref() == Some("checkout") && !parsed.pathspecs().is_empty() {
        return None;
    }

    let is_force = match cmd.primary_command.as_deref() {
        Some("checkout") => parsed.has_command_flag("--force") || parsed.has_command_flag("-f"),
        Some("switch") => {
            parsed.has_command_flag("--discard-changes")
                || parsed.has_command_flag("--force")
                || parsed.has_command_flag("-f")
        }
        _ => false,
    };
    if is_force {
        return None;
    }

    let is_merge = parsed.has_command_flag("--merge") || parsed.has_command_flag("-m");
    if is_merge {
        return None;
    }

    Some(RecentReplayPrerequisite::CheckoutSwitchRename {
        target_head: new_head,
        old_head,
    })
}
pub fn family_key_for_repository(repo: &Repository) -> String {
    crate::operations::git::canonicalize::canonicalize_or_self(repo.common_dir())
        .to_string_lossy()
        .to_string()
}
pub fn is_non_auxiliary_ref(reference: &str) -> bool {
    !(reference.starts_with("refs/notes/")
        || reference.starts_with("refs/tags/")
        || reference.starts_with("refs/replace/"))
}

/// Check whether `ancestor` is an ancestor of `descendant` using
/// `git merge-base --is-ancestor`.
pub fn is_ancestor_commit(repository: &Repository, ancestor: &str, descendant: &str) -> bool {
    repository
        .is_ancestor(ancestor, descendant)
        .unwrap_or(false)
}

pub fn repo_is_ancestor(
    repository: &crate::operations::git::repository::Repository,
    ancestor: &str,
    descendant: &str,
) -> bool {
    is_ancestor_commit(repository, ancestor, descendant)
}

pub fn rebase_is_control_mode(cmd: &crate::model::domain::NormalizedCommand) -> bool {
    summarize_rebase_args(&cmd.invoked_args).is_control_mode
}

pub fn rebase_onto_from_command(
    cmd: &crate::model::domain::NormalizedCommand,
    repository: &Repository,
    original_head: &str,
    new_tip: &str,
) -> Option<String> {
    let head_changes = cmd
        .ref_changes
        .iter()
        .filter(|change| {
            change.reference == "HEAD"
                && is_non_zero_oid(&change.old)
                && is_non_zero_oid(&change.new)
                && change.old != change.new
        })
        .collect::<Vec<_>>();

    head_changes
        .iter()
        .find(|change| {
            change.old == original_head
                && change.new != original_head
                && change.new != new_tip
                && is_ancestor_commit(repository, &change.new, new_tip)
        })
        .map(|change| change.new.clone())
        .or_else(|| {
            head_changes
                .iter()
                .find(|change| {
                    change.old != original_head
                        && change.old != new_tip
                        && is_ancestor_commit(repository, &change.old, new_tip)
                })
                .map(|change| change.old.clone())
        })
}

pub fn valid_non_zero_ref_change(change: &crate::model::domain::RefChange) -> bool {
    is_non_zero_oid(&change.old) && is_non_zero_oid(&change.new) && change.old != change.new
}

pub fn rewrite_metric_branch_for_ref(reference: &str) -> Option<String> {
    crate::operations::authorship::rewrite::branch_name_from_ref(reference)
}

pub fn rewrite_metric_branch_for_transition(
    cmd: &crate::model::domain::NormalizedCommand,
    old_tip: &str,
    new_tip: &str,
    reference_hint: Option<&str>,
) -> Option<String> {
    reference_hint
        .and_then(rewrite_metric_branch_for_ref)
        .or_else(|| {
            cmd.ref_changes
                .iter()
                .rev()
                .find(|change| {
                    change.reference.starts_with("refs/heads/")
                        && change.old == old_tip
                        && change.new == new_tip
                })
                .and_then(|change| rewrite_metric_branch_for_ref(&change.reference))
        })
}

pub(crate) fn rewrite_metric_commits_with_branch(
    metric_commits: Vec<crate::operations::authorship::rewrite::RewriteMetricCommit>,
    branch: Option<String>,
) -> Vec<crate::operations::authorship::rewrite::RewriteMetricCommit> {
    match branch {
        Some(branch) => metric_commits
            .into_iter()
            .map(|commit| commit.with_branch(branch.clone()))
            .collect(),
        None => metric_commits,
    }
}
