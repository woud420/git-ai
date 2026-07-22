use crate::error::GitAiError;
use crate::model::domain::RewriteEvent;
use crate::operations::git::cli_parser::explicit_rebase_branch_arg;
use std::collections::HashMap;

fn revert_destination_changes(
    cmd: &crate::model::domain::NormalizedCommand,
) -> Vec<&crate::model::domain::RefChange> {
    cmd.ref_changes
        .iter()
        .filter(|change| {
            change.reference == "HEAD"
                && super::is_valid_oid(&change.old)
                && !super::is_zero_oid(&change.old)
                && super::is_valid_oid(&change.new)
                && !super::is_zero_oid(&change.new)
                && change.old != change.new
        })
        .collect()
}

pub(crate) fn apply_revert_complete_rewrite(
    repo: &crate::operations::git::repository::Repository,
    cmd: &crate::model::domain::NormalizedCommand,
    source_oids: &[String],
) -> Result<(), GitAiError> {
    let specs: Vec<crate::operations::authorship::rewrite_revert::RevertSpec> =
        revert_destination_changes(cmd)
            .into_iter()
            .enumerate()
            .map(
                |(index, change)| crate::operations::authorship::rewrite_revert::RevertSpec {
                    revert_commit: change.new.clone(),
                    parent: Some(change.old.clone()),
                    reverted_commit: source_oids.get(index).cloned(),
                },
            )
            .collect();
    let metric_commits =
        crate::operations::authorship::rewrite_revert::handle_revert_commits_with_metrics(
            repo, &specs,
        )?;
    crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(repo, metric_commits);
    Ok(())
}

pub(crate) fn apply_cherry_pick_complete_rewrite(
    repo: &crate::operations::git::repository::Repository,
    original_head: &str,
    sources: &[String],
    new_commits: &[String],
) -> Result<(), GitAiError> {
    let pairs = crate::operations::authorship::rewrite_cherry_pick::match_cherry_pick_pairs(
        repo,
        sources,
        new_commits,
    )?;
    let mut rewrite_metric_commits = Vec::new();
    if !pairs.is_empty() {
        let (src, dst): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        let outcome = crate::operations::authorship::rewrite::handle_rewrite_event_with_metrics(
            repo,
            RewriteEvent::CherryPickComplete {
                sources: src,
                new_commits: dst,
            },
        )?;
        rewrite_metric_commits.extend(outcome.metric_commits);
    }

    let existing_notes = crate::operations::git::notes_api::read_notes_batch(repo, new_commits)?;
    let author = repo.effective_author_identity().formatted_or_unknown();

    // The cherry-picked commits form a chain: each commit's parent is the
    // previous one (the first's parent is original_head). Build the
    // (commit, parent) pairs, then batch the parent->commit diffs for the
    // commits that actually need reconstruction into ONE diff-tree so the loop
    // performs no per-commit git spawns.
    let mut commit_parent_pairs: Vec<(String, String)> = Vec::new();
    let mut parent = original_head.to_string();
    for commit_sha in new_commits {
        commit_parent_pairs.push((commit_sha.clone(), parent.clone()));
        parent = commit_sha.clone();
    }
    let qualifying: Vec<&(String, String)> = commit_parent_pairs
        .iter()
        .filter(|(_, parent_sha)| repo.storage.has_working_log(parent_sha))
        .collect();
    let diff_pairs: Vec<(String, String)> = qualifying
        .iter()
        .map(|(commit_sha, parent_sha)| (parent_sha.clone(), commit_sha.clone()))
        .collect();
    let diff_results = if diff_pairs.is_empty() {
        Vec::new()
    } else {
        crate::operations::authorship::rewrite::compute_diff_trees_batch(repo, &diff_pairs)?
    };
    let diff_by_commit: HashMap<&str, &crate::operations::authorship::rewrite::DiffTreeResult> =
        qualifying
            .iter()
            .zip(diff_results.iter())
            .map(|((commit_sha, _), result)| (commit_sha.as_str(), result))
            .collect();

    super::flush_pending_note_writes(
        repo,
        &commit_parent_pairs,
        &existing_notes,
        author,
        &diff_by_commit,
    )?;

    let rewrite_metric_commits = if rewrite_metric_commits.is_empty() {
        rewrite_metric_commits
    } else {
        let parent_by_commit: HashMap<&str, &str> = commit_parent_pairs
            .iter()
            .map(|(commit_sha, parent_sha)| (commit_sha.as_str(), parent_sha.as_str()))
            .collect();
        rewrite_metric_commits
            .into_iter()
            .map(|mut commit| {
                if let Some(parent_sha) = parent_by_commit.get(commit.new_sha.as_str()) {
                    commit = commit.with_parent_sha((*parent_sha).to_string());
                }
                if let Some(diff) = diff_by_commit.get(commit.new_sha.as_str()) {
                    commit = commit.with_parent_diff((*diff).clone());
                }
                commit
            })
            .collect()
    };
    crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
        repo,
        rewrite_metric_commits,
    );

    Ok(())
}

pub(crate) fn apply_cherry_pick_no_commit_rewrite(
    repo: &crate::operations::git::repository::Repository,
    sources: &[String],
    parent_head: &str,
    new_head: &str,
) -> Result<(), GitAiError> {
    if sources.is_empty() || new_head.is_empty() {
        return Ok(());
    }
    let mappings = sources
        .iter()
        .map(|source| (source.clone(), new_head.to_string()))
        .collect::<Vec<_>>();
    crate::operations::git::sync_authorship::fetch_missing_notes_for_commits(repo, sources)?;
    let shifted_notes =
        crate::operations::authorship::rewrite::shift_authorship_notes_merging_existing_with_notes(
            repo, &mappings,
        )?;
    if crate::operations::authorship::rewrite::rewrite_metrics_enabled() {
        let mut metric_commit = crate::operations::authorship::rewrite::RewriteMetricCommit::new(
            new_head.to_string(),
            sources.to_vec(),
            crate::operations::authorship::rewrite::RewriteMetricOperation::CherryPickNoCommit,
        )
        .with_parent_sha(parent_head.to_string());
        if let Some((_, note)) = shifted_notes
            .into_iter()
            .find(|(commit_sha, _)| commit_sha == new_head)
        {
            metric_commit = metric_commit.with_authorship_note(note);
        }
        crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
            repo,
            vec![metric_commit],
        );
    }
    Ok(())
}

pub(crate) fn strict_rebase_original_head_from_command(
    cmd: &crate::model::domain::NormalizedCommand,
    semantic_old_head: &str,
) -> Option<String> {
    if let Some(branch_spec) = explicit_rebase_branch_arg(&cmd.invoked_args)
        && let Some(branch_ref) = explicit_rebase_branch_ref_name(&branch_spec)
        && let Some(old_head) = cmd
            .ref_changes
            .iter()
            .find(|change| {
                change.reference == branch_ref
                    && super::is_valid_oid(&change.old)
                    && !super::is_zero_oid(&change.old)
            })
            .map(|change| change.old.clone())
    {
        return Some(old_head);
    }

    if super::is_valid_oid(semantic_old_head) && !super::is_zero_oid(semantic_old_head) {
        return Some(semantic_old_head.to_string());
    }

    if let Some(old_head) = cmd
        .ref_changes
        .iter()
        .find(|change| {
            change.reference.starts_with("refs/heads/")
                && super::is_valid_oid(&change.old)
                && !super::is_zero_oid(&change.old)
        })
        .map(|change| change.old.clone())
    {
        return Some(old_head);
    }

    if let Some(old_head) = cmd
        .ref_changes
        .iter()
        .find(|change| {
            change.reference == "HEAD"
                && super::is_valid_oid(&change.old)
                && !super::is_zero_oid(&change.old)
        })
        .map(|change| change.old.clone())
    {
        return Some(old_head);
    }

    None
}

fn explicit_rebase_branch_ref_name(branch_spec: &str) -> Option<String> {
    if branch_spec.starts_with("refs/") {
        return Some(branch_spec.to_string());
    }
    if super::is_valid_oid(branch_spec) || branch_spec == "HEAD" || branch_spec.starts_with("@{") {
        return None;
    }
    Some(format!("refs/heads/{}", branch_spec))
}
