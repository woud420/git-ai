use super::*;
use crate::process_timeout::{TimedCommandOutput, run_prepared_command_with_timeout};
use std::time::Duration;

pub(crate) fn exec_git_with_timeout(
    args: &[String],
    timeout: Duration,
) -> Result<TimedCommandOutput<Vec<u8>>, GitAiError> {
    let effective_args = args_with_internal_git_profile(
        &args_with_disabled_hooks_if_needed(args),
        InternalGitProfile::General,
    );
    spawn_probe_log(&effective_args);
    let mut command = Command::new(config::Config::get().git_cmd());
    command.args(&effective_args);
    apply_internal_git_machine_env(&mut command);
    let output = run_prepared_command_with_timeout(command, timeout, Duration::from_millis(10))?;
    if output.timed_out {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("git command timed out after {} ms", timeout.as_millis()),
        )
        .into());
    }
    if let Some(error) = &output.wait_error {
        return Err(std::io::Error::other(error.clone()).into());
    }
    if !output.diagnostics.is_empty() {
        return Err(std::io::Error::other(output.diagnostics.join("; ")).into());
    }
    if output.status != Some(0) {
        return Err(git_cli_error(output.status, &output.stderr, effective_args));
    }
    Ok(output)
}
