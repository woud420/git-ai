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
    assert!(installed.contains("server: GitAiPlugin"));
    assert!(installed.contains("export const GitAiPlugin: Plugin"));
    assert!(installed.contains("export default GitAiPluginModule"));
    assert!(!installed.contains("__GIT_AI_BINARY_PATH__"));
}
