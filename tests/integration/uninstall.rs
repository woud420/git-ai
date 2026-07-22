//! `git-ai uninstall` removes the machine-level integration artifacts.
//! TestRepo isolates $HOME, so these tests plant realistic artifacts in the
//! test home and assert the inverse-of-install inventory.

use crate::repos::test_repo::TestRepo;
use std::fs;

fn plant_artifacts(repo: &TestRepo) -> std::path::PathBuf {
    let home = repo.test_home_path().clone();

    // Installed binary + git shim.
    let bin_dir = home.join(".git-ai").join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("git-ai"), "fake-binary").unwrap();
    fs::write(bin_dir.join("git"), "fake-shim").unwrap();

    // Installer-added PATH lines.
    fs::write(
        home.join(".zshrc"),
        "export FOO=1\n# Added by git-ai installer on Sun Jul 20\nexport PATH=\"$HOME/.git-ai/bin:$PATH\"\n",
    )
    .unwrap();

    // Data that must survive a non-purge uninstall.
    let internal = home.join(".git-ai").join("internal");
    fs::create_dir_all(&internal).unwrap();
    fs::write(internal.join("some-db"), "data").unwrap();

    home
}

#[test]
fn test_uninstall_removes_artifacts_but_keeps_data() {
    let repo = TestRepo::new_dedicated_daemon();
    let home = plant_artifacts(&repo);

    // Point the global git config trace2 target at a git-ai socket path.
    repo.git(&[
        "config",
        "--global",
        "trace2.eventTarget",
        "af_unix:stream:/tmp/.git-ai/daemon.sock",
    ])
    .unwrap();

    repo.git_ai(&["uninstall", "--yes"])
        .expect("uninstall should succeed");

    assert!(
        !home.join(".git-ai").join("bin").exists(),
        "binary dir should be removed"
    );
    let zshrc = fs::read_to_string(home.join(".zshrc")).unwrap();
    assert!(
        !zshrc.contains(".git-ai") && !zshrc.contains("Added by git-ai"),
        "installer PATH lines should be stripped, got: {zshrc}"
    );
    assert!(zshrc.contains("export FOO=1"), "user lines survive");
    assert!(
        home.join(".git-ai")
            .join("internal")
            .join("some-db")
            .exists(),
        "data must be kept without --purge"
    );
    let trace2 = repo
        .git(&["config", "--global", "--get", "trace2.eventTarget"])
        .unwrap_or_default();
    assert!(
        trace2.trim().is_empty(),
        "trace2 config should be removed, got: {trace2}"
    );
}

#[test]
fn test_uninstall_purge_removes_data_dir() {
    let repo = TestRepo::new_dedicated_daemon();
    let home = plant_artifacts(&repo);

    repo.git_ai(&["uninstall", "--yes", "--purge"])
        .expect("uninstall --purge should succeed");

    assert!(
        !home.join(".git-ai").exists(),
        "~/.git-ai should be fully removed with --purge"
    );
}

#[test]
fn test_uninstall_leaves_foreign_trace2_config_alone() {
    let repo = TestRepo::new_dedicated_daemon();
    plant_artifacts(&repo);

    repo.git(&[
        "config",
        "--global",
        "trace2.eventTarget",
        "/tmp/my-own-trace-target",
    ])
    .unwrap();

    repo.git_ai(&["uninstall", "--yes"])
        .expect("uninstall should succeed");

    let trace2 = repo
        .git(&["config", "--global", "--get", "trace2.eventTarget"])
        .unwrap();
    assert_eq!(
        trace2.trim(),
        "/tmp/my-own-trace-target",
        "a user's own trace2 target must not be touched"
    );
}

/// Windows named-pipe targets written by the git-ai daemon must be removed.
/// We simulate this on all platforms by writing the pipe-format string
/// directly to the global git config.
#[test]
fn test_uninstall_removes_windows_pipe_trace2_target() {
    let repo = TestRepo::new_dedicated_daemon();
    plant_artifacts(&repo);

    // The daemon writes \\.\pipe\git-ai-<hash16>-trace2 on Windows.
    repo.git(&[
        "config",
        "--global",
        "trace2.eventTarget",
        r"\\.\pipe\git-ai-abcdef1234567890-trace2",
    ])
    .unwrap();

    repo.git_ai(&["uninstall", "--yes"])
        .expect("uninstall should succeed");

    let trace2 = repo
        .git(&["config", "--global", "--get", "trace2.eventTarget"])
        .unwrap_or_default();
    assert!(
        trace2.trim().is_empty(),
        "Windows named-pipe trace2 target must be removed by uninstall, got: {trace2}"
    );
}
