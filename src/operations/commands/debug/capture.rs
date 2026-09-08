use crate::process_spawn::format_posix_shell_command as format_command_for_error;
use crate::process_timeout::{TimedCommandOutput, run_command_with_timeout_and_env};
use std::time::Duration;

pub(super) fn run_command_capture(program: &str, args: &[&str]) -> Result<String, String> {
    run_command_capture_with_timeout(program, args, DEBUG_COMMAND_TIMEOUT)
}

pub(super) fn run_git_command_capture(program: &str, args: &[&str]) -> Result<String, String> {
    run_git_command_capture_with_timeout(program, args, DEBUG_COMMAND_TIMEOUT)
}

pub(super) fn run_command_capture_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    run_command_capture_with_timeout_and_env(program, args, timeout, &[], &[])
}

pub(super) fn run_git_command_capture_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    run_command_capture_with_timeout_and_env(
        program,
        args,
        timeout,
        crate::clients::git_cli::INTERNAL_GIT_ENV_REMOVE,
        crate::clients::git_cli::INTERNAL_GIT_ENV_SET,
    )
}

pub(super) fn run_command_capture_with_timeout_and_env(
    program: &str,
    args: &[&str],
    timeout: Duration,
    env_remove: &[&str],
    env_set: &[(&str, &str)],
) -> Result<String, String> {
    let command = format_command_for_error(program, args);
    let output = run_command_with_timeout_and_env(
        program,
        args,
        None,
        timeout,
        DEBUG_COMMAND_POLL_INTERVAL,
        env_remove,
        env_set,
    )
    .map_err(|e| {
        format!(
            "failed to execute '{}': {}",
            program,
            strip_execute_prefix(&e)
        )
    })?;

    capture_result(&command, timeout, output)
}

pub(super) fn capture_result(
    command: &str,
    timeout: Duration,
    output: TimedCommandOutput,
) -> Result<String, String> {
    if output.timed_out {
        return Err(format_timeout_capture_error(command, timeout, output));
    }
    if output.wait_error.is_some() {
        return Err(format_wait_capture_error(command, output));
    }

    command_output_to_result(output)
}

pub(super) fn command_output_to_result(output: TimedCommandOutput) -> Result<String, String> {
    if output.status != Some(0) {
        let mut stderr = output.stderr.trim().to_string();
        append_debug_diagnostics(&mut stderr, &output.diagnostics);
        let code = output
            .status
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        if stderr.is_empty() {
            return Err(format!("exit code {}", code));
        }
        return Err(format!("exit code {}: {}", code, stderr));
    }

    Ok(output.stdout)
}

pub(super) fn format_timeout_capture_error(
    command: &str,
    timeout: Duration,
    output: TimedCommandOutput,
) -> String {
    let mut message = format!(
        "timed out after {:.1}s running '{}'",
        timeout.as_secs_f64(),
        command
    );
    append_debug_diagnostics(&mut message, &output.diagnostics);
    if let Some(wait_error) = output.wait_error {
        message.push_str(&format!("; failed while waiting: {}", wait_error));
    }
    if !output.stdout.trim().is_empty() {
        message.push_str(&format!(
            "; stdout before timeout: {}",
            output.stdout.trim()
        ));
    }
    if !output.stderr.trim().is_empty() {
        message.push_str(&format!(
            "; stderr before timeout: {}",
            output.stderr.trim()
        ));
    }
    message
}

pub(super) fn format_wait_capture_error(command: &str, output: TimedCommandOutput) -> String {
    let wait_error = output.wait_error.as_deref().unwrap_or("unknown wait error");
    let mut message = format!("failed while waiting for '{}': {}", command, wait_error);
    append_debug_diagnostics(&mut message, &output.diagnostics);
    if !output.stdout.trim().is_empty() {
        message.push_str(&format!(
            "; stdout before wait failure: {}",
            output.stdout.trim()
        ));
    }
    if !output.stderr.trim().is_empty() {
        message.push_str(&format!(
            "; stderr before wait failure: {}",
            output.stderr.trim()
        ));
    }
    message
}

pub(super) fn append_debug_diagnostics(message: &mut String, diagnostics: &[String]) {
    for diagnostic in diagnostics {
        if !message.is_empty() {
            message.push_str("; ");
        }
        message.push_str(diagnostic);
    }
}

pub(super) fn strip_execute_prefix(error: &str) -> &str {
    error.strip_prefix("failed to execute: ").unwrap_or(error)
}

const DEBUG_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const DEBUG_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(100);
