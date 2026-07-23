//! Shared fixtures for the bash-tool test trio (provenance, conformance,
//! timeouts): AI-provenance tracking via bash pre/post snapshots.

use crate::repos::test_repo::TestRepo;
use git_ai::model::working_log::AgentId;
use git_ai::operations::commands::checkpoint_agent::bash_tool::{
    BashPostHookResult, handle_bash_post_tool_use, handle_bash_pre_tool_use_with_context,
    set_daemon_socket_for_test,
};

/// Stage and commit a file so it appears in `git ls-files` (tracked).
pub fn add_and_commit(repo: &TestRepo, rel_path: &str, contents: &str, message: &str) {
    repo.write_file(rel_path, contents);
    repo.git_og(&["add", rel_path])
        .expect("git add should succeed");
    repo.git_og(&["commit", "-m", message])
        .expect("git commit should succeed");
}

/// Canonical repo root path (resolves /tmp -> /private/tmp on macOS).
///
/// Side effect: also points the bash-tool snapshot machinery at this repo's
/// daemon control socket via `set_daemon_socket_for_test`, since the hooks
/// below reach the daemon directly rather than through `TestRepo::git_ai`.
pub fn repo_root(repo: &TestRepo) -> std::path::PathBuf {
    set_daemon_socket_for_test(repo.daemon_control_socket_path());
    repo.canonical_path()
}

pub fn dummy_agent_id() -> AgentId {
    AgentId {
        tool: "test".to_string(),
        id: "test".to_string(),
        model: String::new(),
    }
}

pub fn dummy_trace_id() -> &'static str {
    "t_test123456789a"
}

pub fn pre_hook(root: &std::path::Path, session_id: &str, tool_use_id: &str) {
    handle_bash_pre_tool_use_with_context(
        root,
        session_id,
        tool_use_id,
        &dummy_agent_id(),
        None,
        dummy_trace_id(),
        None,
    )
    .expect("pre-hook should succeed");
}

pub fn post_hook(
    root: &std::path::Path,
    session_id: &str,
    tool_use_id: &str,
) -> BashPostHookResult {
    handle_bash_post_tool_use(
        root,
        session_id,
        tool_use_id,
        &dummy_agent_id(),
        None,
        dummy_trace_id(),
        None,
    )
    .expect("post-hook should succeed")
}
