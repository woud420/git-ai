use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use std::fs;
use std::path::Path;

#[test]
fn eng_394_documented_nix_cleanup_removes_runtime_state_before_the_package() {
    let readme =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README-nix.md")).unwrap();
    let uninstall = readme
        .split("## Uninstall")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap();
    let first_block = uninstall
        .split("```bash")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let command = first_block
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap();
    let words = command.split_whitespace().collect::<Vec<_>>();
    assert_eq!(words[0], "git-ai");
    assert!(matches!(words[1], "uninstall" | "uninstall-hooks"));
    assert!(
        !words.contains(&"--purge"),
        "default cleanup must retain data"
    );

    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let home = repo.test_home_path();
    let daemon = home.join(".git-ai/internal/daemon");
    fs::create_dir_all(&daemon).unwrap();
    fs::write(daemon.join("test.lock"), "stale lock").unwrap();
    let data = home.join(".git-ai/internal/retained-data");
    fs::write(&data, "local data").unwrap();
    repo.git(&[
        "config",
        "--global",
        "trace2.eventTarget",
        "af_unix:stream:/tmp/.git-ai/daemon.sock",
    ])
    .unwrap();

    repo.git_ai_without_pre_sync_for_test(&words[1..]).unwrap();
    assert!(
        repo.git(&["config", "--global", "--get", "trace2.eventTarget"])
            .is_err()
    );
    assert!(!daemon.exists());
    assert_eq!(fs::read_to_string(data).unwrap(), "local data");
    let package_removal = uninstall.find("nix profile remove").unwrap();
    assert!(uninstall.find(command).unwrap() < package_removal);
    assert!(uninstall.find("git-ai uninstall --yes --purge").unwrap() < package_removal);
}
