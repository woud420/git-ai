#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::{TestRepo, real_git_executable};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn bare_repository(path: &Path) {
    let output = Command::new(real_git_executable())
        .args(["init", "--bare"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn remote_note(path: &Path, commit: &str) -> Option<String> {
    let output = Command::new(real_git_executable())
        .arg("-C")
        .arg(path)
        .args(["notes", "--ref=ai", "show", commit])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).unwrap())
}

const PUSH_ARGS: &[&str] = &["push", "origin", "HEAD:refs/heads/main"];

fn delayed_push_keeps_destinations(mirrored: bool, relative_quoted: bool, args: &[&str]) {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("push-gate");
    let gate_spec = format!("push={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &gate_spec)]);
    let first = if relative_quoted {
        repo.path().join("remote's space.git")
    } else {
        temp.path().join("original.git")
    };
    let second = temp.path().join("mirror.git");
    let decoy = temp.path().join("later-config.git");
    for remote in [&first, &second, &decoy] {
        bare_repository(remote);
    }
    let mut file = repo.filename("source.txt");
    file.set_contents(lines!["published AI".ai()]);
    repo.git(&["add", "source.txt"]).unwrap();
    repo.commit("seed attributed content").unwrap();
    file.assert_committed_lines(lines!["published AI".ai()]);
    let commit = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let expected = repo.read_authorship_note(&commit).unwrap();
    let destination = if relative_quoted {
        "./remote's space.git"
    } else {
        first.to_str().unwrap()
    };
    repo.git_og(&["remote", "add", "origin", destination])
        .unwrap();
    if mirrored {
        repo.git_og(&["config", "--add", "remote.origin.pushurl", destination])
            .unwrap();
        repo.git_og(&[
            "config",
            "--add",
            "remote.origin.pushurl",
            second.to_str().unwrap(),
        ])
        .unwrap();
    }
    fs::create_dir_all(repo.path().join("nested")).unwrap();
    fs::write(&gate, "").unwrap();
    let before = repo.daemon_total_completion_count();
    repo.git_without_test_sync_for_test(args, &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "push did not enter side-effect gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    repo.git_og(&["remote", "set-url", "origin", decoy.to_str().unwrap()])
        .unwrap();
    if mirrored {
        repo.git_og(&["config", "--unset-all", "remote.origin.pushurl"])
            .unwrap();
    }
    fs::remove_file(&gate).unwrap();
    repo.sync_daemon_force();
    repo.wait_for_daemon_total_completion_count(before, before + 1);
    assert_eq!(
        remote_note(&first, &commit).as_deref().map(str::trim),
        Some(expected.trim())
    );
    if mirrored {
        assert_eq!(
            remote_note(&second, &commit).as_deref().map(str::trim),
            Some(expected.trim())
        );
    }
    assert_eq!(
        remote_note(&decoy, &commit),
        None,
        "later config must not redirect historical notes delivery"
    );
    file.assert_committed_lines(lines!["published AI".ai()]);
}

#[test]
fn delayed_notes_push_uses_the_destination_of_the_completed_user_push() {
    delayed_push_keeps_destinations(false, false, PUSH_ARGS);
}

#[test]
fn delayed_notes_push_delivers_to_every_captured_push_url() {
    delayed_push_keeps_destinations(true, false, PUSH_ARGS);
}

#[test]
fn delayed_notes_push_preserves_quoted_relative_file_destination() {
    delayed_push_keeps_destinations(false, true, PUSH_ARGS);
}

#[test]
fn relative_push_url_is_resolved_from_git_repository_root_with_c_flag() {
    delayed_push_keeps_destinations(
        false,
        true,
        &["-C", "nested", "push", "origin", "HEAD:refs/heads/main"],
    );
}

#[test]
fn delayed_push_alias_uses_the_captured_destination() {
    delayed_push_keeps_destinations(
        false,
        false,
        &[
            "-c",
            "alias.publish=push",
            "publish",
            "origin",
            "HEAD:refs/heads/main",
        ],
    );
}

#[cfg(unix)]
#[path = "notes_push/queued_delivery.rs"]
mod queued_delivery;
