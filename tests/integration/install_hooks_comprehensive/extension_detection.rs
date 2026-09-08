#[cfg(not(windows))]
use super::{DaemonTestScope, TestRepo, fs};

#[test]
#[cfg(not(windows))]
fn install_hooks_detects_cline_from_editor_extension_manifests() {
    let manifest_layouts = [
        ".vscode/extensions/extensions.json",
        ".vscode-server/extensions/extensions.json",
        ".cursor/extensions/extensions.json",
        ".windsurf/extensions/extensions.json",
    ];

    for relative_path in manifest_layouts {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let home = repo.test_home_path();
        let manifest_path = home.join(relative_path);
        fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::write(
            manifest_path,
            r#"[
  {
    "identifier": {
      "id": "saoudrizwan.claude-dev",
      "uuid": "test-uuid"
    },
    "version": "4.0.11",
    "relativeLocation": "saoudrizwan.claude-dev-4.0.11"
  }
]"#,
        )
        .unwrap();

        let output = repo
            .git_ai_command_without_pre_sync_for_test(&["install-hooks", "--dry-run"], &[])
            .output()
            .expect("run git-ai install-hooks --dry-run");
        assert!(
            output.status.success(),
            "install-hooks failed for {relative_path}:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Cline: Pending updates"),
            "Cline was not detected from {relative_path}:\n{stdout}"
        );
        assert!(
            !home.join("Documents/Cline/Hooks").exists(),
            "dry-run must not create the Cline hooks directory"
        );
    }
}

#[test]
#[cfg(not(windows))]
fn install_hooks_ignores_unrelated_extension_manifest_entries() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let home = repo.test_home_path();
    let manifest_path = home.join(".vscode/extensions/extensions.json");
    fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    fs::write(
        manifest_path,
        r#"[
  {
    "identifier": {
      "id": "git-ai.git-ai-vscode"
    },
    "relativeLocation": "saoudrizwan.claude-dev-4.0.11"
  }
]"#,
    )
    .unwrap();

    let output = repo
        .git_ai_command_without_pre_sync_for_test(&["install-hooks", "--dry-run"], &[])
        .output()
        .expect("run git-ai install-hooks --dry-run");
    assert!(
        output.status.success(),
        "install-hooks failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Cline: Pending updates"),
        "an unrelated extension id must not read as a Cline install:\n{stdout}"
    );
}
