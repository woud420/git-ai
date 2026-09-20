use crate::error::GitAiError;
use crate::model::domain::{
    AnalysisResult, CommandClass, Confidence, NormalizedCommand, PullStrategy, SemanticEvent,
};
use crate::operations::daemon::analyzers::{
    AnalysisView, CommandAnalyzer, command_args, normalized_args,
};
use std::path::PathBuf;

#[derive(Default)]
pub struct TransportAnalyzer;

impl CommandAnalyzer for TransportAnalyzer {
    fn analyze(
        &self,
        cmd: &NormalizedCommand,
        _state: AnalysisView<'_>,
    ) -> Result<AnalysisResult, GitAiError> {
        let name = cmd.primary_command.as_deref().unwrap_or_default();
        let args = command_args(cmd);

        let mut events = Vec::new();
        match name {
            "fetch" | "fetch-pack" => events.push(SemanticEvent::FetchCompleted {
                remote: first_positional(&args),
            }),
            "pull" => events.push(SemanticEvent::PullCompleted {
                remote: first_positional(&args),
                strategy: infer_pull_strategy(cmd, &args),
            }),
            "push" => events.push(SemanticEvent::PushCompleted {
                remote: first_positional(&args),
            }),
            "send-pack" => {
                if let Some(repository) = send_pack_destination(cmd) {
                    events.push(SemanticEvent::SendPackCompleted { repository });
                }
            }
            "clone" => events.push(SemanticEvent::CloneCompleted {
                target: infer_clone_target(&args)
                    .or_else(|| cmd.worktree.clone())
                    .unwrap_or_else(|| PathBuf::from(".")),
            }),
            "ls-remote" => events.push(SemanticEvent::LsRemoteCompleted),
            _ => unreachable!("registry should not route '{}' to TransportAnalyzer", name),
        }

        Ok(AnalysisResult {
            class: CommandClass::Transport,
            events,
            confidence: if cmd.exit_code == 0 {
                Confidence::High
            } else {
                Confidence::Low
            },
        })
    }
}

fn send_pack_destination(cmd: &NormalizedCommand) -> Option<String> {
    if cmd.exit_code != 0 || cmd.raw_argv.is_empty() {
        return None;
    }
    let parsed =
        crate::operations::git::cli_parser::parse_git_cli_args(&normalized_args(&cmd.raw_argv));
    let [repository, refspec] = parsed.command_args.as_slice() else {
        return None;
    };
    let (source, destination) = refspec.split_once(':')?;
    if parsed.command.as_deref() != Some("send-pack")
        || !std::path::Path::new(repository).is_absolute()
        || !crate::model::git_oid::is_non_zero_oid(source)
        || !destination.starts_with("refs/heads/")
        || destination.len() == "refs/heads/".len()
        || destination.contains('*')
    {
        return None;
    }
    // Normalization resolves -C. Other globals can change repository selection
    // or transport semantics that this separate notes operation cannot replay.
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return None,
        }
    }
    Some(repository.clone())
}

fn first_positional(args: &[String]) -> Option<String> {
    args.iter().find(|arg| !arg.starts_with('-')).cloned()
}

fn infer_pull_strategy(cmd: &NormalizedCommand, args: &[String]) -> PullStrategy {
    if let Some(strategy) = infer_pull_strategy_from_args(args) {
        return strategy;
    }
    let raw_args = normalized_args(&cmd.raw_argv);
    if let Some(strategy) = infer_pull_strategy_from_args(&raw_args) {
        return strategy;
    }
    if cmd
        .observed_child_commands
        .iter()
        .any(|child| child == "rebase")
    {
        return PullStrategy::Rebase;
    }
    PullStrategy::Merge
}

fn infer_pull_strategy_from_args(args: &[String]) -> Option<PullStrategy> {
    if args
        .iter()
        .any(|arg| arg == "--no-rebase" || arg == "--rebase=false")
    {
        return Some(PullStrategy::Merge);
    }
    if args.iter().any(|arg| arg == "--ff-only") {
        return Some(PullStrategy::FastForwardOnly);
    }
    if args
        .iter()
        .any(|arg| arg == "--rebase=merges" || arg == "--rebase-merges")
    {
        return Some(PullStrategy::RebaseMerges);
    }
    if args
        .iter()
        .any(|arg| arg == "--rebase" || arg == "--rebase=true")
    {
        return Some(PullStrategy::Rebase);
    }
    None
}

fn infer_clone_target(args: &[String]) -> Option<PathBuf> {
    if args.is_empty() {
        return None;
    }
    let mut filtered = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "-C" || arg == "--origin" || arg == "--template" {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        filtered.push(arg.clone());
    }
    filtered.last().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::daemon::analyzers::tests::command;

    #[test]
    fn pull_with_rebase_maps_strategy() {
        let analyzer = TransportAnalyzer;
        let result = analyzer
            .analyze(
                &command("pull", &["git", "pull", "--rebase"]),
                AnalysisView::from_refs(&Default::default()),
            )
            .unwrap();
        assert!(result.events.iter().any(|event| matches!(
            event,
            SemanticEvent::PullCompleted {
                strategy: PullStrategy::Rebase,
                ..
            }
        )));
    }

    #[test]
    fn send_pack_sync_requires_a_successful_literal_profile() {
        let root = std::env::temp_dir();
        let endpoint = root.to_str().unwrap();
        for length in [40, 64] {
            let spec = format!("{}:refs/heads/main", "a".repeat(length));
            let mut cmd = command("send-pack", &["git", "send-pack", endpoint, &spec]);
            assert_eq!(send_pack_destination(&cmd), Some(endpoint.into()));
            cmd.exit_code = 1;
            assert!(send_pack_destination(&cmd).is_none());
        }
        let spec = format!("{}:refs/heads/main", "a".repeat(40));
        for args in [
            vec!["git", "send-pack", "--stdin", endpoint],
            vec!["git", "send-pack", "--atomic", endpoint, &spec],
            vec!["git", "send-pack", "--mirror", endpoint],
            vec!["git", "send-pack", "--all", endpoint],
            vec!["git", "send-pack", endpoint, ":refs/heads/main"],
            vec!["git", "send-pack", endpoint, "HEAD:refs/heads/main"],
            vec![
                "git",
                "send-pack",
                endpoint,
                "0000000000000000000000000000000000000000:refs/heads/main",
            ],
            vec!["git", "send-pack", "origin", &spec],
            vec!["git", "send-pack", "relative/path", &spec],
            vec![
                "git",
                "-c",
                "receive.unpackLimit=0",
                "send-pack",
                endpoint,
                &spec,
            ],
        ] {
            assert!(
                send_pack_destination(&command("send-pack", &args)).is_none(),
                "{args:?}"
            );
        }
    }

    #[test]
    fn send_pack_sync_does_not_reclassify_a_parent_push() {
        let mut cmd = command("push", &["git", "push", "origin"]);
        cmd.observed_child_commands.push("send-pack".into());
        let result = TransportAnalyzer
            .analyze(&cmd, AnalysisView::from_refs(&Default::default()))
            .unwrap();
        assert!(matches!(
            result.events.as_slice(),
            [SemanticEvent::PushCompleted { .. }]
        ));
    }
}
