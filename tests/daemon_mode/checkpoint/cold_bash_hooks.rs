#[cfg(unix)]
use super::super::*;

#[cfg(unix)]
use super::run_authorized_bash_hooks_from_cold_daemon;

#[test]
#[serial]
#[cfg(unix)]
fn authorized_legacy_bash_hooks_make_cold_daemon_ready_before_persistence() {
    run_authorized_bash_hooks_from_cold_daemon(false);
}

#[test]
#[serial]
#[cfg(unix)]
fn authorized_bash_v2_hooks_make_cold_daemon_ready_before_persistence() {
    run_authorized_bash_hooks_from_cold_daemon(true);
}

#[test]
#[serial]
#[cfg(unix)]
fn denied_and_empty_checkpoint_hooks_do_not_start_cold_daemon() {
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let socket_paths = ColdDaemonSocketPaths::new(&repo);
    let empty_hook_input = json!({
        "cwd": repo.canonical_path(),
        "file_paths": [],
    })
    .to_string();
    let mut empty_command = repo.git_ai_command_without_pre_sync_for_test(
        &[
            "checkpoint",
            "mock_ai",
            "--hook-input",
            &empty_hook_input,
            "--",
        ],
        &[],
    );
    let empty_output = empty_command
        .env("GIT_AI_TEST_ALLOW_DAEMON_AUTOSPAWN", "1")
        .env("GIT_AI_DAEMON_CONTROL_SOCKET", &socket_paths.control)
        .env("GIT_AI_DAEMON_TRACE_SOCKET", &socket_paths.trace)
        .output()
        .expect("failed to invoke empty checkpoint hook");
    assert!(empty_output.status.success());
    assert!(!socket_paths.control.exists());

    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(Vec::new());
    });
    let denied_hook_input = json!({
        "session_id": "denied-cold-bash-session",
        "cwd": repo.canonical_path(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "denied-cold-bash-tool",
        "tool_input": { "command": "true" }
    })
    .to_string();
    let mut denied_command = repo.git_ai_command_without_pre_sync_for_test(
        &["checkpoint", "codex", "--hook-input", &denied_hook_input],
        &[],
    );
    let denied_output = denied_command
        .env("GIT_AI_TEST_ALLOW_DAEMON_AUTOSPAWN", "1")
        .env("GIT_AI_DAEMON_CONTROL_SOCKET", &socket_paths.control)
        .env("GIT_AI_DAEMON_TRACE_SOCKET", &socket_paths.trace)
        .output()
        .expect("failed to invoke denied checkpoint hook");
    assert!(denied_output.status.success());
    assert!(String::from_utf8_lossy(&denied_output.stderr).contains("no repositories are allowed"));
    assert!(
        !socket_paths.control.exists(),
        "authorization denials must happen before Bash daemon readiness"
    );
}
