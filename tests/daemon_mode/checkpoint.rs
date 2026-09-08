#[cfg(unix)]
use super::*;

#[cfg(unix)]
fn run_authorized_bash_hooks_from_cold_daemon(bash_checkpoints_v2: bool) {
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.patch_git_ai_config(|patch| {
        patch.feature_flags = Some(json!({"bash_checkpoints_v2": bash_checkpoints_v2}));
    });
    let repo_root = repo.canonical_path();
    let socket_paths = ColdDaemonSocketPaths::new(&repo);
    let bash_db_path = repo.test_home_path().join(if bash_checkpoints_v2 {
        "cold-bash-v2.sqlite"
    } else {
        "cold-bash-legacy.sqlite"
    });
    let bash_db_path_string = bash_db_path.to_string_lossy().into_owned();
    let session_id = if bash_checkpoints_v2 {
        "cold-bash-v2-session"
    } else {
        "cold-bash-legacy-session"
    };
    let tool_use_id = if bash_checkpoints_v2 {
        "cold-bash-v2-tool"
    } else {
        "cold-bash-legacy-tool"
    };

    assert!(
        !socket_paths.control.exists(),
        "the regression must start with a cold daemon"
    );

    let mut hook_outputs = Vec::new();
    for hook_event_name in ["PreToolUse", "PostToolUse"] {
        let hook_input = json!({
            "session_id": session_id,
            "cwd": repo_root,
            "hook_event_name": hook_event_name,
            "tool_name": "Bash",
            "tool_use_id": tool_use_id,
            "tool_input": { "command": "true" },
            "model": "test-model"
        })
        .to_string();
        let mut command = repo.git_ai_command_without_pre_sync_for_test(
            &["checkpoint", "codex", "--hook-input", &hook_input],
            &[],
        );
        let output = command
            .env("GIT_AI_TEST_ALLOW_DAEMON_AUTOSPAWN", "1")
            .env("GIT_AI_DAEMON_CONTROL_SOCKET", &socket_paths.control)
            .env("GIT_AI_DAEMON_TRACE_SOCKET", &socket_paths.trace)
            .env("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", &bash_db_path_string)
            .output()
            .expect("failed to invoke authorized Bash checkpoint");
        assert!(
            output.status.success(),
            "Bash hooks must preserve their exit-zero contract: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        hook_outputs.push(format!(
            "{hook_event_name}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    assert!(
        send_control_request(
            &socket_paths.control,
            &ControlRequest::StatusFamily {
                repo_working_dir: repo_workdir_string(&repo),
            },
        )
        .is_ok(),
        "an authorized Bash hook should make the cold daemon ready before persistence; {}",
        hook_outputs.join(" | ")
    );
    let db = BashHistoryDatabase::open_at_path(&bash_db_path)
        .expect("the Bash history database should be readable");
    let calls = db
        .all_calls_for_test()
        .expect("Bash history calls should be readable");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].session_id, session_id);
    assert_eq!(calls[0].tool_use_id, tool_use_id);
    assert!(calls[0].start_trace_id.is_some());
    assert!(calls[0].end_trace_id.is_some());

    let _ = send_control_request(&socket_paths.control, &ControlRequest::Shutdown);
    for _ in 0..200 {
        if !socket_paths.control.exists() && !socket_paths.trace.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[path = "checkpoint/checkpoint_application.rs"]
mod checkpoint_application;
#[path = "checkpoint/cold_bash_hooks.rs"]
mod cold_bash_hooks;
#[path = "checkpoint/daemon_startup.rs"]
mod daemon_startup;
#[path = "checkpoint/outbox_delivery.rs"]
mod outbox_delivery;
