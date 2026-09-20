use crate::error::GitAiError;
use crate::model::domain::{
    AnalysisResult, AppliedCommand, FamilyState, GlobalState, NormalizedCommand, WorktreeState,
};
use crate::model::git_oid::is_zero_oid;
use crate::operations::daemon::analyzers::{AnalysisView, AnalyzerRegistry};
use std::path::PathBuf;

/// Convenience wrapper around [`reduce_family_command_with_ref_snapshot`] for
/// unit tests.
///
/// **Skips canonicalization** — worktrees are keyed by the raw path from
/// `cmd.worktree` (i.e. `canonical_worktree = None`).  Production callers
/// must use [`reduce_family_command_with_ref_snapshot`] with a
/// pre-canonicalized path so that symlinked worktree paths (e.g. `/tmp` →
/// `/private/tmp` on macOS) resolve to a single canonical key.
pub fn reduce_family_command(
    state: &mut FamilyState,
    cmd: NormalizedCommand,
    analyzers: &AnalyzerRegistry,
) -> Result<(AppliedCommand, AnalysisResult), GitAiError> {
    reduce_family_command_with_ref_snapshot(
        state,
        cmd,
        analyzers,
        &std::collections::HashMap::new(),
        None,
    )
}

pub fn reduce_family_command_with_ref_snapshot(
    state: &mut FamilyState,
    cmd: NormalizedCommand,
    analyzers: &AnalyzerRegistry,
    command_start_refs: &std::collections::HashMap<String, String>,
    canonical_worktree: Option<PathBuf>,
) -> Result<(AppliedCommand, AnalysisResult), GitAiError> {
    // Analyze against pre-command state so history/ref analyzers can infer old->new correctly.
    let refs_for_analysis;
    let analysis_refs = if command_start_refs.is_empty() {
        &state.refs
    } else {
        refs_for_analysis = state
            .refs
            .iter()
            .map(|(reference, oid)| (reference.clone(), oid.clone()))
            .chain(
                command_start_refs
                    .iter()
                    .map(|(reference, oid)| (reference.clone(), oid.clone())),
            )
            .collect();
        &refs_for_analysis
    };
    let analysis = analyzers.analyze(
        &cmd,
        AnalysisView {
            refs: analysis_refs,
            worktree: canonical_worktree
                .as_ref()
                .or(cmd.worktree.as_ref())
                .and_then(|path| state.worktrees.get(path)),
        },
    )?;
    apply_ref_changes(state, &cmd);
    if analysis.events.iter().any(|event| {
        matches!(
            event,
            crate::model::domain::SemanticEvent::OrphanBranchCreated { .. }
        )
    }) {
        state.refs.remove("HEAD");
    }
    apply_worktree_state(state, &cmd, canonical_worktree, &analysis);

    state.applied_seq = state.applied_seq.saturating_add(1);
    let applied = AppliedCommand {
        seq: state.applied_seq,
        command: cmd,
        analysis: analysis.clone(),
    };
    Ok((applied, analysis))
}

pub fn reduce_global_command(
    state: &mut GlobalState,
    cmd: NormalizedCommand,
    analyzers: &AnalyzerRegistry,
) -> Result<(AppliedCommand, AnalysisResult), GitAiError> {
    let empty_refs = std::collections::HashMap::new();
    let analysis = analyzers.analyze(&cmd, AnalysisView::from_refs(&empty_refs))?;
    state.applied_seq = state.applied_seq.saturating_add(1);
    let applied = AppliedCommand {
        seq: state.applied_seq,
        command: cmd,
        analysis: analysis.clone(),
    };
    Ok((applied, analysis))
}

pub fn reduce_checkpoint(state: &mut FamilyState) {
    state.applied_seq = state.applied_seq.saturating_add(1);
}

fn apply_ref_changes(state: &mut FamilyState, cmd: &NormalizedCommand) {
    for change in &cmd.ref_changes {
        if change.new.trim().is_empty() || is_zero_oid(&change.new) {
            state.refs.remove(&change.reference);
        } else {
            state
                .refs
                .insert(change.reference.clone(), change.new.clone());
        }
    }
}

fn apply_worktree_state(
    state: &mut FamilyState,
    cmd: &NormalizedCommand,
    canonical_worktree: Option<PathBuf>,
    analysis: &AnalysisResult,
) {
    let Some(worktree) = cmd.worktree.as_ref() else {
        return;
    };
    let key = canonical_worktree.unwrap_or_else(|| worktree.clone());
    let previous = state.worktrees.get(&key);
    let head_change = cmd
        .ref_changes
        .iter()
        .rfind(|change| change.reference == "HEAD");

    let orphan_branch = analysis.events.iter().find_map(|event| match event {
        crate::model::domain::SemanticEvent::OrphanBranchCreated { branch, .. } => {
            Some(branch.clone())
        }
        _ => None,
    });
    let (head, branch, detached) = if let Some(branch) = orphan_branch {
        (None, Some(branch), false)
    } else if let Some(head_change) = head_change {
        // DEFERRED: `detached` is inferred as "no unique
        // branch ref moved with HEAD". When a checkout/switch to an EXISTING
        // branch produces an ambiguous ref-change pairing (e.g. multiple
        // refs/heads/* share the same old->new as HEAD, so
        // unique_branch_for_head_change returns None), the worktree is
        // misclassified as detached. Harmless for attribution today (the head
        // OID is still correct); a precise fix would consult the actual
        // post-command symbolic-ref/branch name rather than inferring from
        // ref-change pairing.
        let branch = unique_branch_for_head_change(cmd, head_change);
        (
            Some(head_change.new.clone()),
            branch.clone(),
            branch.is_none(),
        )
    } else if let Some(branch) = checkout_or_switch_branch_target(cmd) {
        (
            previous.and_then(|worktree| worktree.head.clone()),
            Some(branch),
            false,
        )
    } else {
        (
            previous.and_then(|worktree| worktree.head.clone()),
            previous.and_then(|worktree| worktree.branch.clone()),
            previous.is_some_and(|worktree| worktree.detached),
        )
    };

    state.worktrees.insert(
        key,
        WorktreeState {
            head,
            branch,
            detached,
            last_updated_ns: cmd.finished_at_ns,
        },
    );
}

fn unique_branch_for_head_change(
    cmd: &NormalizedCommand,
    head_change: &crate::model::domain::RefChange,
) -> Option<String> {
    let mut matches = cmd
        .ref_changes
        .iter()
        .filter(|change| {
            change.reference.starts_with("refs/heads/")
                && change.old == head_change.old
                && change.new == head_change.new
        })
        .map(|change| change.reference.clone());
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn checkout_or_switch_branch_target(cmd: &NormalizedCommand) -> Option<String> {
    let command = cmd.primary_command.as_deref()?;
    let args = command_args(cmd);
    match command {
        "checkout" => checkout_created_branch_target(&args),
        "switch" => switch_branch_target(&args),
        _ => None,
    }
    .map(|branch| {
        if branch.starts_with("refs/") {
            branch
        } else {
            format!("refs/heads/{branch}")
        }
    })
}

fn command_args(cmd: &NormalizedCommand) -> Vec<String> {
    if !cmd.invoked_args.is_empty() {
        let mut args = vec![
            cmd.invoked_command
                .clone()
                .or_else(|| cmd.primary_command.clone())
                .unwrap_or_default(),
        ];
        args.extend(cmd.invoked_args.clone());
        return args;
    }
    cmd.raw_argv
        .iter()
        .skip_while(|arg| arg.as_str() != cmd.primary_command.as_deref().unwrap_or(""))
        .cloned()
        .collect()
}

fn checkout_created_branch_target(args: &[String]) -> Option<String> {
    let mut idx = usize::from(args.first().is_some_and(|arg| arg == "checkout"));
    while idx < args.len() {
        match args[idx].as_str() {
            "-b" | "-B" => return args.get(idx + 1).cloned(),
            value if value.starts_with("-b") && value.len() > 2 => {
                return Some(value[2..].to_string());
            }
            value if value.starts_with("-B") && value.len() > 2 => {
                return Some(value[2..].to_string());
            }
            "--" => return None,
            _ => idx += 1,
        }
    }
    None
}

fn switch_branch_target(args: &[String]) -> Option<String> {
    let mut idx = usize::from(args.first().is_some_and(|arg| arg == "switch"));
    while idx < args.len() {
        match args[idx].as_str() {
            "-c" | "-C" | "--create" | "--force-create" => return args.get(idx + 1).cloned(),
            value if value.starts_with("--create=") => {
                return Some(value["--create=".len()..].to_string());
            }
            value if value.starts_with("--force-create=") => {
                return Some(value["--force-create=".len()..].to_string());
            }
            value if value.starts_with("-c") && value.len() > 2 => {
                return Some(value[2..].to_string());
            }
            value if value.starts_with("-C") && value.len() > 2 => {
                return Some(value[2..].to_string());
            }
            "--detach" | "-d" | "--" => return None,
            value if !value.starts_with('-') => return Some(value.to_string()),
            _ => idx += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests;
