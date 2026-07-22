//! Integration tests for the install manifest and fence-block rc file editing.
//!
//! Tests cover:
//! - Manifest written by `git-ai install-hooks` in an isolated $HOME
//! - Fence-block parse/removal round-trips (fenced and legacy unfenced)
//! - `git-ai uninstall` consuming the manifest to clean rc files

use crate::repos::test_repo::TestRepo;
use git_ai::operations::commands::install_manifest::{
    FENCE_CLOSE, FENCE_OPEN, InstallManifest, make_fence_block, remove_fence_block,
};
use std::fs;

// ── Unit-level fence tests ────────────────────────────────────────────────────

#[test]
fn fence_open_close_constants_are_distinct() {
    assert_ne!(FENCE_OPEN, FENCE_CLOSE);
    assert!(FENCE_OPEN.starts_with("# >>>"));
    assert!(FENCE_CLOSE.starts_with("# <<<"));
}

#[test]
fn make_fence_block_produces_parseable_block() {
    let block = make_fence_block("export PATH=\"/x:$PATH\"");
    assert!(block.contains(FENCE_OPEN));
    assert!(block.contains(FENCE_CLOSE));
    assert!(block.contains("export PATH"));
}

#[test]
fn remove_fence_block_round_trips_simple() {
    let original = "line_before\n";
    let block = make_fence_block("export PATH=\"$HOME/.git-ai/bin:$PATH\"");
    let combined = format!("{}{}", original, block);
    let result = remove_fence_block(&combined);
    assert!(result.removed_fence, "fence should be found and removed");
    assert_eq!(result.text, original);
}

#[test]
fn remove_fence_block_round_trips_content_before_and_after() {
    let before = "# existing\nexport A=1\n";
    let after = "alias gs='git status'\n";
    let block = make_fence_block("export PATH=\"$HOME/.git-ai/bin:$PATH\"");
    let combined = format!("{}{}{}", before, block, after);
    let result = remove_fence_block(&combined);
    assert!(result.removed_fence);
    assert_eq!(result.text, format!("{}{}", before, after));
}

#[test]
fn remove_fence_block_idempotent_on_unrelated_content() {
    let content = "export FOO=1\nexport BAR=2\n";
    let result = remove_fence_block(content);
    assert!(!result.removed_fence);
    assert_eq!(
        result.text, content,
        "unrelated content must not be altered"
    );
}

#[test]
fn remove_fence_block_strips_legacy_unfenced_installer_lines() {
    let content = concat!(
        "export FOO=1\n",
        "# Added by git-ai installer on Mon Jan 1\n",
        "export PATH=\"/x/.git-ai/bin:$PATH\"\n",
        "alias ll='ls -l'\n",
    );
    let result = remove_fence_block(content);
    // No fence block, but legacy lines are gone.
    assert!(!result.removed_fence);
    assert!(
        !result.text.contains(".git-ai"),
        "legacy lines should be stripped"
    );
    assert!(result.text.contains("export FOO=1"));
    assert!(result.text.contains("alias ll"));
}

#[test]
fn remove_fence_block_noop_when_nothing_to_remove() {
    let content = "export FOO=1\n";
    let result = remove_fence_block(content);
    assert!(!result.removed_fence);
    assert_eq!(result.text, content);
}

// ── Manifest serialisation ────────────────────────────────────────────────────

#[test]
fn manifest_roundtrip_through_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    // Point home at temp dir by manipulating the manifest path directly.
    let mut m = InstallManifest {
        version: 1,
        binary_path: Some(dir.path().join("bin/git-ai").to_string_lossy().into_owned()),
        ..Default::default()
    };
    m.add_rc_file(dir.path().join(".zshrc").to_str().unwrap());
    m.add_git_config_key("trace2.eventTarget");
    m.add_agent_hook("claude-code");

    // Serialize to JSON and deserialize back.
    let json = serde_json::to_string_pretty(&m).unwrap();
    let loaded: InstallManifest = serde_json::from_str(&json).unwrap();

    assert_eq!(loaded.version, 1);
    assert!(loaded.binary_path.is_some());
    assert_eq!(loaded.rc_files.len(), 1);
    assert_eq!(loaded.git_config_keys, vec!["trace2.eventTarget"]);
    assert_eq!(loaded.agent_hooks, vec!["claude-code"]);
}

#[test]
fn manifest_add_methods_deduplicate() {
    let mut m = InstallManifest {
        version: 1,
        ..Default::default()
    };
    m.add_rc_file("/home/user/.zshrc");
    m.add_rc_file("/home/user/.zshrc");
    m.add_git_config_key("trace2.eventTarget");
    m.add_git_config_key("trace2.eventTarget");
    m.add_agent_hook("cursor");
    m.add_agent_hook("cursor");
    assert_eq!(m.rc_files.len(), 1);
    assert_eq!(m.git_config_keys.len(), 1);
    assert_eq!(m.agent_hooks.len(), 1);
}

// ── Integration: install-hooks writes manifest, uninstall consumes it ─────────

/// Verify that `git-ai install-hooks --dry-run` does not write the manifest.
#[test]
fn install_hooks_dry_run_does_not_write_manifest() {
    let repo = TestRepo::new_dedicated_daemon();
    let home = repo.test_home_path().clone();
    let manifest_path = home.join(".git-ai").join("install-manifest.json");

    // Dry-run must succeed and must not create the manifest.
    repo.git_ai(&["install-hooks", "--dry-run=true"])
        .expect("install-hooks --dry-run should succeed");

    assert!(
        !manifest_path.exists(),
        "manifest must not be created during a dry-run install-hooks"
    );
}

/// Verify that a real `git-ai install-hooks` run writes git_config_keys and
/// agent_hooks into the manifest.
#[test]
fn install_hooks_writes_trace2_keys_to_manifest() {
    let repo = TestRepo::new_dedicated_daemon();
    let home = repo.test_home_path().clone();
    let manifest_path = home.join(".git-ai").join("install-manifest.json");

    repo.git_ai(&["install-hooks"])
        .expect("install-hooks should succeed");

    assert!(
        manifest_path.exists(),
        "manifest must be created by install-hooks"
    );

    let content = fs::read_to_string(&manifest_path).expect("manifest must be readable");
    let manifest: InstallManifest =
        serde_json::from_str(&content).expect("manifest must be valid JSON");

    assert!(
        manifest
            .git_config_keys
            .contains(&"trace2.eventTarget".to_string()),
        "manifest must record trace2.eventTarget; got: {:?}",
        manifest.git_config_keys
    );
    assert!(
        manifest
            .git_config_keys
            .contains(&"trace2.eventNesting".to_string()),
        "manifest must record trace2.eventNesting; got: {:?}",
        manifest.git_config_keys
    );
}

/// End-to-end: plant a fenced rc file, write a manifest pointing at it,
/// then run `git-ai uninstall --yes` and verify the fence block is removed.
#[test]
fn uninstall_removes_fenced_rc_block_from_manifest() {
    let repo = TestRepo::new_dedicated_daemon();
    let home = repo.test_home_path().clone();

    // Plant a .zshrc with a fenced block.
    let zshrc = home.join(".zshrc");
    let block = make_fence_block("export PATH=\"$HOME/.git-ai/bin:$PATH\"");
    fs::write(&zshrc, format!("export FOO=1\n{}", block)).unwrap();

    // Plant a fake binary dir so removal steps don't fail on missing paths.
    let bin_dir = home.join(".git-ai").join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("git-ai"), "fake").unwrap();

    // Write a manifest that records the rc file.
    let manifest_dir = home.join(".git-ai");
    fs::create_dir_all(&manifest_dir).unwrap();
    let manifest_content = format!(
        r#"{{"version":1,"binary_path":"{}/git-ai","rc_files":[{{"path":"{}"}}]}}"#,
        bin_dir.to_string_lossy().replace('\\', "/"),
        zshrc.to_string_lossy().replace('\\', "/"),
    );
    fs::write(
        manifest_dir.join("install-manifest.json"),
        &manifest_content,
    )
    .unwrap();

    // Run uninstall.
    repo.git_ai(&["uninstall", "--yes"])
        .expect("uninstall should succeed");

    // The fenced block must be gone, but user content must survive.
    let cleaned = fs::read_to_string(&zshrc).unwrap();
    assert!(
        !cleaned.contains(FENCE_OPEN),
        "fence open marker should be gone"
    );
    assert!(
        !cleaned.contains(FENCE_CLOSE),
        "fence close marker should be gone"
    );
    assert!(!cleaned.contains(".git-ai"), "PATH entry should be gone");
    assert!(
        cleaned.contains("export FOO=1"),
        "user content must survive"
    );
}

/// Legacy path: no manifest, bare installer lines in .zshrc.
/// Uninstall should strip them with best-effort even without a manifest.
#[test]
fn uninstall_strips_legacy_unfenced_lines_without_manifest() {
    let repo = TestRepo::new_dedicated_daemon();
    let home = repo.test_home_path().clone();

    let zshrc = home.join(".zshrc");
    fs::write(
        &zshrc,
        "export FOO=1\n# Added by git-ai installer on Sun Jul 20\nexport PATH=\"$HOME/.git-ai/bin:$PATH\"\nalias ll='ls -l'\n",
    ).unwrap();

    // Plant a fake binary dir.
    let bin_dir = home.join(".git-ai").join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("git-ai"), "fake").unwrap();

    repo.git_ai(&["uninstall", "--yes"])
        .expect("uninstall should succeed");

    let cleaned = fs::read_to_string(&zshrc).unwrap();
    assert!(
        !cleaned.contains(".git-ai"),
        "legacy lines should be stripped"
    );
    assert!(
        cleaned.contains("export FOO=1"),
        "user content must survive"
    );
    assert!(cleaned.contains("alias ll"), "user content must survive");
}
