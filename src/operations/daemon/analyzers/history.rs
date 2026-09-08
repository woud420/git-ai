use crate::error::GitAiError;
use crate::model::domain::{
    AnalysisResult, CommandClass, Confidence, NormalizedCommand, ResetKind, SemanticEvent,
};
use crate::operations::daemon::analyzers::{AnalysisView, CommandAnalyzer, command_args};
use crate::operations::git::cli_parser::explicit_rebase_branch_arg;
use crate::operations::git::oid::{is_non_zero_oid, is_zero_oid};

#[derive(Default)]
pub struct HistoryAnalyzer;

impl CommandAnalyzer for HistoryAnalyzer {
    fn analyze(
        &self,
        cmd: &NormalizedCommand,
        state: AnalysisView<'_>,
    ) -> Result<AnalysisResult, GitAiError> {
        let name = cmd.primary_command.as_deref().unwrap_or_default();
        let args = command_args(cmd);

        let mut events = Vec::new();
        match name {
            "commit" | "revert" => {
                let amend = args.iter().any(|arg| arg == "--amend");
                if amend {
                    if let Some((old_head, new_head)) = amend_head_change(cmd) {
                        events.push(SemanticEvent::CommitAmended { old_head, new_head });
                    }
                } else if let Some((old_head, new_head)) = head_change(cmd, state.refs) {
                    events.push(SemanticEvent::CommitCreated {
                        base: sanitize_base(Some(old_head), &new_head),
                        new_head,
                    });
                }
            }
            "reset" => {
                if let Some((old_head, new_head)) = head_change(cmd, state.refs) {
                    events.push(SemanticEvent::Reset {
                        kind: infer_reset_kind(&args),
                        old_head,
                        new_head,
                    });
                }
            }
            "rebase" => {
                if args.iter().any(|arg| arg == "--abort") {
                    events.push(SemanticEvent::RebaseAbort {
                        head: current_head_from_ref_data(cmd, state.refs).unwrap_or_default(),
                    });
                } else if let Some((old_head, new_head)) = rebase_change(cmd, state.refs) {
                    events.push(SemanticEvent::RebaseComplete {
                        old_head,
                        new_head,
                        interactive: args.iter().any(|arg| arg == "-i" || arg == "--interactive"),
                    });
                }
            }
            "cherry-pick" => {
                if args.iter().any(|arg| arg == "--abort") {
                    events.push(SemanticEvent::CherryPickAbort {
                        head: current_head_from_ref_data(cmd, state.refs).unwrap_or_default(),
                    });
                } else if args.iter().any(|arg| arg == "--no-commit" || arg == "-n") {
                    events.push(SemanticEvent::CherryPickNoCommit {
                        source_commits: cmd.cherry_pick_source_oids.clone(),
                        head: current_head_from_ref_data(cmd, state.refs).unwrap_or_default(),
                    });
                } else if let Some((old_head, new_head)) = head_change(cmd, state.refs) {
                    events.push(SemanticEvent::CherryPickComplete {
                        original_head: old_head,
                        new_head,
                        source_commits: cmd.cherry_pick_source_oids.clone(),
                        new_commits: cherry_pick_new_commits(cmd),
                    });
                }
            }
            "merge" => {
                if args.iter().any(|arg| arg == "--squash") {
                    if let Some(source_head) = squash_source_head(&args, state.refs)
                        && let Some(onto) = current_head_from_ref_data(cmd, state.refs)
                    {
                        events.push(SemanticEvent::MergeSquash { source_head, onto });
                    }
                } else if let Some((old_head, new_head)) = head_change(cmd, state.refs) {
                    events.push(SemanticEvent::RefUpdated {
                        reference: "HEAD".to_string(),
                        old: old_head,
                        new: new_head,
                    });
                }
            }
            "update-ref" => {
                for change in cmd.ref_changes.iter().filter(|change| {
                    (change.reference == "HEAD" || change.reference.starts_with("refs/heads/"))
                        && change.old.trim() != change.new.trim()
                }) {
                    events.push(SemanticEvent::RefUpdated {
                        reference: change.reference.clone(),
                        old: change.old.clone(),
                        new: change.new.clone(),
                    });
                }
            }
            _ => unreachable!("registry should not route '{}' to HistoryAnalyzer", name),
        }

        if events.is_empty() {
            events.push(SemanticEvent::OpaqueCommand);
        }

        Ok(AnalysisResult {
            class: CommandClass::HistoryRewrite,
            events,
            confidence: if cmd.exit_code == 0 {
                Confidence::High
            } else {
                Confidence::Low
            },
        })
    }
}

fn sanitize_base(base: Option<String>, new_head: &str) -> Option<String> {
    base.filter(|candidate| candidate != new_head && !is_zero_oid(candidate))
}

fn squash_source_head(
    args: &[String],
    refs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let source = merge_source_args(args).into_iter().next()?;
    resolve_revision_from_ref_state(source, refs)
}

fn merge_source_args(args: &[String]) -> Vec<&str> {
    let mut sources = Vec::new();
    let mut iter = args.iter().map(String::as_str).peekable();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            sources.extend(iter.filter(|value| !value.is_empty()));
            break;
        }
        if arg == "-m"
            || arg == "--message"
            || arg == "-s"
            || arg == "--strategy"
            || arg == "-X"
            || arg == "--strategy-option"
        {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--message=")
            || arg.starts_with("--strategy=")
            || arg.starts_with("--strategy-option=")
            || arg.starts_with("--gpg-sign=")
            || arg.starts_with("-m")
            || arg.starts_with("-s")
            || arg.starts_with("-X")
            || arg.starts_with("-S")
        {
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        sources.push(arg);
    }
    sources
}

fn resolve_revision_from_ref_state(
    revision: &str,
    refs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    if is_non_zero_oid(revision) {
        return Some(revision.to_string());
    }
    if revision == "HEAD" {
        return refs.get("HEAD").filter(|oid| is_non_zero_oid(oid)).cloned();
    }
    if revision.starts_with("refs/") {
        return refs
            .get(revision)
            .filter(|oid| is_non_zero_oid(oid))
            .cloned();
    }

    for reference in [
        format!("refs/heads/{}", revision),
        format!("refs/remotes/{}", revision),
        format!("refs/tags/{}", revision),
    ] {
        if let Some(oid) = refs.get(&reference)
            && is_non_zero_oid(oid)
        {
            return Some(oid.clone());
        }
    }

    None
}

fn valid_ref_transition(change: &crate::model::domain::RefChange) -> Option<(String, String)> {
    let old = change.old.trim();
    let new = change.new.trim();
    if old == new || !is_non_zero_oid(old) || !is_non_zero_oid(new) {
        return None;
    }
    Some((old.to_string(), new.to_string()))
}

fn first_ref_transition_for(cmd: &NormalizedCommand, reference: &str) -> Option<(String, String)> {
    cmd.ref_changes
        .iter()
        .filter(|change| change.reference == reference)
        .find_map(valid_ref_transition)
}

fn current_head_from_ref_data(
    cmd: &NormalizedCommand,
    refs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    cmd.ref_changes
        .iter()
        .rev()
        .find(|change| change.reference == "HEAD")
        .map(|change| change.new.clone())
        .or_else(|| refs.get("HEAD").cloned())
        .filter(|head| is_non_zero_oid(head))
}

fn amend_head_change(cmd: &NormalizedCommand) -> Option<(String, String)> {
    // Amend is defined by the HEAD transition made by `git commit --amend`.
    // Prefer that exact transition over branch hints: branch context is not
    // part of stock trace2 and can be stale if it was read after the command.
    if let Some(change) = first_ref_transition_for(cmd, "HEAD") {
        return Some(change);
    }

    single_branch_ref_change(cmd)
}

fn head_change(
    cmd: &NormalizedCommand,
    _refs: &std::collections::HashMap<String, String>,
) -> Option<(String, String)> {
    let head_span = cmd
        .ref_changes
        .iter()
        .filter(|change| {
            change.reference == "HEAD"
                && !change.new.trim().is_empty()
                && change.old.trim() != change.new.trim()
        })
        .collect::<Vec<_>>();
    if let Some((old_head, new_head)) = change_span(&head_span) {
        return Some((old_head, new_head));
    }

    single_branch_ref_change(cmd)
}

fn single_branch_ref_change(cmd: &NormalizedCommand) -> Option<(String, String)> {
    let mut branch_refs = cmd
        .ref_changes
        .iter()
        .filter(|change| {
            change.reference.starts_with("refs/heads/")
                && !change.new.trim().is_empty()
                && change.old.trim() != change.new.trim()
        })
        .collect::<Vec<_>>();
    if branch_refs.is_empty() {
        return None;
    }
    branch_refs.sort_by(|a, b| a.reference.cmp(&b.reference));
    branch_refs.dedup_by(|a, b| a.reference == b.reference && a.old == b.old && a.new == b.new);
    let first_ref = branch_refs.first()?.reference.as_str();
    if branch_refs
        .iter()
        .any(|change| change.reference.as_str() != first_ref)
    {
        return None;
    }
    change_span(&branch_refs)
}

fn cherry_pick_new_commits(cmd: &NormalizedCommand) -> Vec<String> {
    cmd.ref_changes
        .iter()
        .filter(|change| change.reference == "HEAD")
        .filter_map(valid_ref_transition)
        .map(|(_, new)| new)
        .collect()
}

fn rebase_change(
    cmd: &NormalizedCommand,
    refs: &std::collections::HashMap<String, String>,
) -> Option<(String, String)> {
    if let Some((old_head, new_head)) = explicit_rebase_branch_change(cmd) {
        return Some((old_head, new_head));
    }

    if let Some((old_head, new_head)) = inferred_rebase_branch_change(cmd) {
        return Some((old_head, new_head));
    }

    let (old_head, new_head) = head_change(cmd, refs)?;
    (old_head != new_head).then_some((old_head, new_head))
}

fn inferred_rebase_branch_change(cmd: &NormalizedCommand) -> Option<(String, String)> {
    let mut candidates = cmd
        .ref_changes
        .iter()
        .filter(|change| {
            change.reference.starts_with("refs/heads/")
                && !change.old.trim().is_empty()
                && !change.new.trim().is_empty()
                && change.old.trim() != change.new.trim()
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }

    if candidates.len() == 1 {
        let change = candidates.pop()?;
        return Some((change.old.trim().to_string(), change.new.trim().to_string()));
    }

    None
}

fn explicit_rebase_branch_change(cmd: &NormalizedCommand) -> Option<(String, String)> {
    let args = command_args(cmd);
    let branch = explicit_rebase_branch_arg(&args)?;
    let branch_ref = if branch.starts_with("refs/") {
        branch.to_string()
    } else {
        format!("refs/heads/{}", branch)
    };
    cmd.ref_changes
        .iter()
        .find(|change| {
            change.reference == branch_ref
                && !change.old.trim().is_empty()
                && !change.new.trim().is_empty()
                && change.old.trim() != change.new.trim()
        })
        .map(|change| (change.old.trim().to_string(), change.new.trim().to_string()))
}

fn change_span(changes: &[&crate::model::domain::RefChange]) -> Option<(String, String)> {
    let first = changes.first()?;
    let last = changes.last()?;
    let old_head = first.old.trim();
    let new_head = last.new.trim();
    if old_head.is_empty() || new_head.is_empty() || old_head == new_head {
        return None;
    }
    Some((old_head.to_string(), new_head.to_string()))
}

fn infer_reset_kind(args: &[String]) -> ResetKind {
    if args.iter().any(|arg| arg == "--soft") {
        return ResetKind::Soft;
    }
    if args.iter().any(|arg| arg == "--mixed") {
        return ResetKind::Mixed;
    }
    if args.iter().any(|arg| arg == "--hard") {
        return ResetKind::Hard;
    }
    if args.iter().any(|arg| arg == "--merge") {
        return ResetKind::Merge;
    }
    if args.iter().any(|arg| arg == "--keep") {
        return ResetKind::Keep;
    }
    ResetKind::Mixed
}

#[path = "history_tests.rs"]
#[cfg(test)]
mod tests;
