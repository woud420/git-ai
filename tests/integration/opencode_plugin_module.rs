use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use std::fs;

#[test]
fn opencode_install_replaces_legacy_plugin_with_named_module() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let plugin_path = repo
        .test_home_path()
        .join(".config/opencode/plugins/git-ai.ts");
    fs::create_dir_all(plugin_path.parent().unwrap()).unwrap();
    fs::write(&plugin_path, "export default async () => ({})\n").unwrap();

    repo.git_ai_without_pre_sync_for_test(&["install-hooks"])
        .expect("install should update the managed OpenCode plugin in the isolated home");

    let installed = fs::read_to_string(plugin_path).unwrap();
    assert!(
        installed.contains("id: \"git-ai\""),
        "installed plugin must expose its display ID"
    );
    // V1 compatibility entrypoint (OpenCode 1.x calls `server()`)
    assert!(installed.contains("async server("));
    // V2 plugin definition registered through the tool domain
    assert!(installed.contains("export const GitAiPlugin = Plugin.define("));
    assert!(installed.contains("ctx.tool.hook(\"execute.before\""));
    assert!(installed.contains("export default GitAiPluginModule"));
    assert!(!installed.contains("__GIT_AI_BINARY_PATH__"));
}

fn assert_v2_active_directory(worktree: bool) {
    use crate::repos::test_file::ExpectedLineExt;
    use std::process::Command;

    let repo = if worktree {
        TestRepo::new_worktree()
    } else {
        TestRepo::new()
    };
    let root = repo.canonical_path();
    let directory = if worktree {
        root.clone()
    } else {
        root.join("nested")
    };
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("edit.ts"), "// baseline\n").unwrap();
    if !worktree {
        fs::write(root.join("edit.ts"), "// untouched\n").unwrap();
    }
    repo.stage_all_and_commit("Initial files").unwrap();

    let project = if worktree {
        let git_dir = repo
            .git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .unwrap();
        std::path::Path::new(git_dir.trim())
            .parent()
            .unwrap()
            .to_path_buf()
    } else {
        root.clone()
    };
    let temp = tempfile::tempdir().unwrap();
    let command_env = repo.git_ai_command_without_pre_sync_for_test(&[], &[]);
    let binary = serde_json::to_string(command_env.get_program().to_str().unwrap()).unwrap();
    let plugin = include_str!("../../agent-support/opencode/git-ai.ts")
        .replace(
            "import { Plugin } from \"@opencode/plugin\"",
            "const Plugin = { define: (value) => value }",
        )
        .replace("\"__GIT_AI_BINARY_PATH__\"", &binary);
    let plugin_path = temp.path().join("git-ai.mts");
    fs::write(&plugin_path, plugin).unwrap();
    let mut command = Command::new("node");
    for (key, value) in command_env.get_envs() {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    let output = command
        .arg("--experimental-strip-types")
        .arg(crate::test_utils::fixture_path(
            "opencode-active-directory.mjs",
        ))
        .arg(&plugin_path)
        .arg(&directory)
        .arg(&project)
        .env(
            "GIT_AI_OPENCODE_STORAGE_PATH",
            crate::test_utils::fixture_path("opencode-sqlite"),
        )
        .env("GIT_AI_OPENCODE_DEBUG", "1")
        .current_dir(temp.path())
        .output()
        .expect("OpenCode plugin tests require Node.js 22.6 or newer");
    assert!(
        output.status.success(),
        "plugin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    repo.stage_all_and_commit("OpenCode edit").unwrap();
    let relative = directory
        .join("edit.ts")
        .strip_prefix(&root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    repo.filename(&relative)
        .assert_committed_lines(crate::lines![
            "// baseline".unattributed_human(),
            "// AI edit".ai(),
        ]);
    if !worktree {
        repo.filename("edit.ts")
            .assert_committed_lines(crate::lines!["// untouched".unattributed_human()]);
    }
}

#[test]
fn test_opencode_plugin_v2_uses_active_worktree_directory() {
    assert_v2_active_directory(true);
}

#[test]
fn test_opencode_plugin_v2_uses_active_subdirectory() {
    assert_v2_active_directory(false);
}
