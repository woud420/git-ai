use crate::clients::git_cli::exec_git_with_timeout;
use crate::error::GitAiError;
use crate::operations::git::refs::AI_AUTHORSHIP_PUSH_REFSPEC;
use crate::process_timeout::TimedCommandOutput;
use std::time::Duration;

pub(super) fn exec_notes_transport(
    args: &[String],
) -> Result<TimedCommandOutput<Vec<u8>>, GitAiError> {
    exec_git_with_timeout(args, notes_transport_timeout())
}

fn notes_transport_timeout() -> Duration {
    #[cfg(any(test, feature = "test-support"))]
    if let Ok(value) = std::env::var("GIT_AI_TEST_NOTES_SYNC_TIMEOUT_MS")
        && let Ok(millis) = value.parse::<u64>()
        && millis > 0
    {
        return Duration::from_millis(millis);
    }
    Duration::from_secs(30)
}

#[cfg(windows)]
pub(super) fn disabled_hooks_config() -> &'static str {
    "core.hooksPath=NUL"
}

#[cfg(not(windows))]
pub(super) fn disabled_hooks_config() -> &'static str {
    "core.hooksPath=/dev/null"
}

fn with_disabled_hooks(mut args: Vec<String>) -> Vec<String> {
    args.push("-c".to_string());
    args.push(disabled_hooks_config().to_string());
    args
}

pub(super) fn build_authorship_fetch_args(
    global_args: Vec<String>,
    remote_name: &str,
    fetch_refspec: &str,
) -> Vec<String> {
    let mut args = with_disabled_hooks(global_args);
    args.push("fetch".to_string());
    args.push("--no-tags".to_string());
    args.push("--recurse-submodules=no".to_string());
    args.push("--no-write-fetch-head".to_string());
    args.push("--no-write-commit-graph".to_string());
    args.push("--no-auto-maintenance".to_string());
    args.push(remote_name.to_string());
    args.push(fetch_refspec.to_string());
    args
}

pub(super) fn build_authorship_push_args(
    global_args: Vec<String>,
    remote_name: &str,
) -> Vec<String> {
    let mut args = with_disabled_hooks(global_args);
    args.push("push".to_string());
    args.push("--quiet".to_string());
    args.push("--no-recurse-submodules".to_string());
    args.push("--no-verify".to_string());
    args.push("--no-signed".to_string());
    args.push(remote_name.to_string());
    args.push(AI_AUTHORSHIP_PUSH_REFSPEC.to_string());
    args
}
