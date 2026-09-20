#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use git_ai::config::{NotesBackendConfig, NotesBackendKind};
use repos::test_file::ExpectedLineExt;
use repos::test_repo::{DaemonTestScope, TestRepo, run_raw_git_plumbing};
use std::fs;

fn source(opt_in: Option<bool>) -> (TestRepo, String, String) {
    configured_source(opt_in, &[])
}

fn configured_source(opt_in: Option<bool>, env: &[(&str, &str)]) -> (TestRepo, String, String) {
    let repo = TestRepo::new_with_daemon_env_and_patch(env, |patch| {
        if let Some(value) = opt_in {
            patch.feature_flags = Some(serde_json::json!({"send_pack_notes_sync": value}));
        }
    });
    seed_source(repo)
}

fn seed_source(repo: TestRepo) -> (TestRepo, String, String) {
    fs::write(repo.path().join("source.txt"), "human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "source.txt"])
        .unwrap();
    fs::write(repo.path().join("source.txt"), "human\nAI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    let oid = repo.stage_all_and_commit("source").unwrap().commit_sha;
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "AI".ai()]);
    let note = repo.read_authorship_note(&oid).unwrap();
    (repo, oid, note)
}

fn send_pack(repo: &TestRepo, args: &[&str]) -> Result<String, String> {
    let output = repo.git_without_test_sync_for_test(args, &[]);
    repo.sync_daemon_force();
    output
}

fn publish(repo: &TestRepo, target: &TestRepo, oid: &str) {
    send_pack(
        repo,
        &[
            "send-pack",
            target.path().to_str().unwrap(),
            &format!("{oid}:refs/heads/main"),
        ],
    )
    .unwrap();
    assert_eq!(
        target
            .git_og(&["rev-parse", "refs/heads/main"])
            .unwrap()
            .trim(),
        oid
    );
}

#[test]
fn send_pack_sync_exports_notes_to_explicit_destination() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    publish(&repo, &target, &oid);
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid),
        Some(note.clone())
    );
    publish(&repo, &target, &oid);
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid),
        Some(note)
    );
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "AI".ai()]);
}

#[test]
fn send_pack_sync_resolves_global_c() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    send_pack(
        &repo,
        &[
            "-C",
            repo.path().to_str().unwrap(),
            "send-pack",
            target.path().to_str().unwrap(),
            &format!("{oid}:refs/heads/main"),
        ],
    )
    .unwrap();
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid),
        Some(note)
    );
}

#[test]
fn send_pack_sync_default_and_explicit_opt_out_preserve_native_export() {
    for opt_in in [None, Some(false)] {
        let (repo, oid, note) = source(opt_in);
        let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
        publish(&repo, &target, &oid);
        assert!(
            repo.read_authorship_note_in_git_dir(target.path(), &oid)
                .is_none()
        );
        assert_eq!(repo.read_authorship_note(&oid), Some(note));
    }
}

#[test]
fn send_pack_sync_uses_literal_destination_despite_url_rewrites() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    let other = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    for kind in ["insteadOf", "pushInsteadOf"] {
        repo.git_og(&[
            "config",
            &format!("url.{}.{kind}", other.path().display()),
            target.path().to_str().unwrap(),
        ])
        .unwrap();
    }
    publish(&repo, &target, &oid);
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid),
        Some(note)
    );
    assert!(
        repo.read_authorship_note_in_git_dir(other.path(), &oid)
            .is_none()
    );
    assert!(
        other
            .git_og(&["rev-parse", "--verify", "refs/heads/main"])
            .is_err()
    );
}

#[test]
fn send_pack_sync_dry_run_preserves_remote_refs() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    send_pack(
        &repo,
        &[
            "send-pack",
            "--dry-run",
            target.path().to_str().unwrap(),
            &format!("{oid}:refs/heads/main"),
        ],
    )
    .unwrap();
    assert!(
        target
            .git_og(&["rev-parse", "--verify", "refs/heads/main"])
            .is_err()
    );
    assert!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .is_none()
    );
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}

#[test]
fn send_pack_sync_unsupported_forms_retain_native_behavior() {
    let (repo, oid, note) = source(Some(true));
    for variant in 0..5 {
        let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
        let endpoint = target.path().to_str().unwrap();
        let spec = format!("{oid}:refs/heads/main");
        let args = match variant {
            0 => vec!["send-pack", "--force", endpoint, &spec],
            1 => vec!["send-pack", endpoint, "HEAD:refs/heads/main"],
            2 => vec!["-c", "user.name=Override", "send-pack", endpoint, &spec],
            3 => vec!["send-pack", endpoint, &spec, "HEAD:refs/heads/other"],
            _ => vec!["send-pack", "--atomic", endpoint, &spec],
        };
        send_pack(&repo, &args).unwrap();
        assert_eq!(
            target
                .git_og(&["rev-parse", "refs/heads/main"])
                .unwrap()
                .trim(),
            oid
        );
        assert!(
            repo.read_authorship_note_in_git_dir(target.path(), &oid)
                .is_none(),
            "{args:?}"
        );
    }
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}

#[test]
fn send_pack_sync_collection_opt_out_preserves_notes() {
    let (mut repo, oid, note) = source(Some(true));
    repo.patch_git_ai_config(|patch| patch.allowed_repositories = Some(Vec::new()));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    publish(&repo, &target, &oid);
    assert!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .is_none()
    );
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}

#[test]
fn send_pack_sync_unsupported_backends_preserve_git_transport() {
    for kind in [NotesBackendKind::Sqlite, NotesBackendKind::Http] {
        let (mut repo, oid, note) = source(Some(true));
        repo.patch_git_ai_config(|patch| {
            patch.notes_backend = Some(NotesBackendConfig {
                kind,
                backend_url: Some("http://127.0.0.1:1".to_string()),
            });
        });
        let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
        publish(&repo, &target, &oid);
        assert!(
            repo.read_authorship_note_in_git_dir(target.path(), &oid)
                .is_none()
        );
        assert_eq!(repo.read_authorship_note(&oid), Some(note));
    }
}

#[test]
fn send_pack_sync_notes_rejection_preserves_native_success() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    target
        .git_og(&["config", "receive.hideRefs", "refs/notes"])
        .unwrap();
    publish(&repo, &target, &oid);
    assert!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .is_none()
    );
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}

#[test]
fn send_pack_sync_partial_rejection_preserves_native_result() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    let (other, _, _) = source(None);
    fs::write(other.path().join("source.txt"), "human\nAI\nother\n").unwrap();
    other
        .git_ai(&["checkpoint", "mock_known_human", "source.txt"])
        .unwrap();
    let other_head = other
        .stage_all_and_commit("other history")
        .unwrap()
        .commit_sha;
    other.filename("source.txt").assert_committed_lines(lines![
        "human".human(),
        "AI".ai(),
        "other".human()
    ]);
    other
        .git_og(&[
            "send-pack",
            target.path().to_str().unwrap(),
            &format!("{other_head}:refs/heads/main"),
        ])
        .unwrap();
    let result = send_pack(
        &repo,
        &[
            "send-pack",
            target.path().to_str().unwrap(),
            &format!("{oid}:refs/heads/main"),
            &format!("{oid}:refs/heads/new"),
        ],
    );
    assert!(result.is_err());
    assert_eq!(
        target
            .git_og(&["rev-parse", "refs/heads/main"])
            .unwrap()
            .trim(),
        other_head
    );
    assert_eq!(
        target
            .git_og(&["rev-parse", "refs/heads/new"])
            .unwrap()
            .trim(),
        oid
    );
    assert!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .is_none()
    );
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}

#[test]
fn send_pack_sync_preserves_pending_attribution_and_local_refs() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.git_ai(&["checkpoint", "human", "source.txt"]).unwrap();
    fs::write(repo.path().join("source.txt"), "human\nAI\npending AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    let logs = repo.current_working_logs();
    let before = fs::read(logs.checkpoints_file()).unwrap();
    let refs = repo
        .git_og(&["for-each-ref", "--format=%(refname) %(objectname)"])
        .unwrap();
    publish(&repo, &target, &oid);
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid),
        Some(note)
    );
    assert_eq!(fs::read(logs.checkpoints_file()).unwrap(), before);
    assert_eq!(
        repo.git_og(&["for-each-ref", "--format=%(refname) %(objectname)"])
            .unwrap(),
        refs
    );
    repo.stage_all_and_commit("pending edit").unwrap();
    repo.filename("source.txt").assert_committed_lines(lines![
        "human".human(),
        "AI".ai(),
        "pending AI".ai()
    ]);
}

#[test]
fn send_pack_sync_divergent_remote_notes_are_never_overwritten() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    target
        .git_og(&["config", "user.useConfigOnly", "true"])
        .unwrap();
    repo.git_og(&[
        "send-pack",
        target.path().to_str().unwrap(),
        &format!("{oid}:refs/heads/main"),
    ])
    .unwrap();
    run_raw_git_plumbing(
        target.path(),
        &["notes", "--ref=ai", "add", "-m", "remote authority", &oid],
        None,
    );
    let remote_ref = target.git_og(&["rev-parse", "refs/notes/ai"]).unwrap();
    publish(&repo, &target, &oid);
    assert_eq!(
        target.git_og(&["rev-parse", "refs/notes/ai"]).unwrap(),
        remote_ref
    );
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .unwrap()
            .trim(),
        "remote authority"
    );
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}

#[test]
fn send_pack_sync_missing_notes_preserve_native_success() {
    let (repo, oid, _) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.git_og(&["update-ref", "-d", "refs/notes/ai"]).unwrap();
    publish(&repo, &target, &oid);
    assert!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .is_none()
    );
    assert!(repo.read_authorship_note(&oid).is_none());
}

#[test]
fn send_pack_sync_exports_from_linked_worktree() {
    let mut repo = TestRepo::new_worktree_with_daemon_scope(DaemonTestScope::Dedicated);
    repo.patch_git_ai_config(|patch| {
        patch.feature_flags = Some(serde_json::json!({"send_pack_notes_sync": true}));
    });
    assert!(repo.path().join(".git").is_file());
    let (repo, oid, note) = seed_source(repo);
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    publish(&repo, &target, &oid);
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid),
        Some(note)
    );
}

#[test]
fn send_pack_sync_process_count_does_not_grow_with_commits() {
    let mut counts = Vec::new();
    for commits in [1, 8] {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("spawns.log");
        let (repo, mut oid, _) =
            configured_source(Some(true), &[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
        for number in 2..=commits {
            let line = format!("AI {number}");
            fs::write(repo.path().join("source.txt"), format!("human\n{line}\n")).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", "source.txt"])
                .unwrap();
            oid = repo
                .stage_all_and_commit(&format!("source {number}"))
                .unwrap()
                .commit_sha;
            repo.filename("source.txt")
                .assert_committed_lines(lines!["human".human(), line.as_str().ai()]);
        }
        let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
        let note = repo.read_authorship_note(&oid).unwrap();
        fs::write(&log, "").unwrap();
        publish(&repo, &target, &oid);
        let spawns = fs::read_to_string(&log).unwrap();
        let commands: Vec<_> = spawns.lines().collect();
        println!("send-pack sync Git processes for {commits} commits: {commands:?}");
        assert_eq!(
            commands
                .iter()
                .filter(|command| **command == "send-pack")
                .count(),
            1,
            "{spawns}"
        );
        assert!(commands.len() <= 8, "{spawns}");
        counts.push(commands.len());
        assert_eq!(
            repo.read_authorship_note_in_git_dir(target.path(), &oid),
            Some(note)
        );
    }
    println!("send-pack sync Git process counts for 1/8 commits: {counts:?}");
    assert_eq!(counts[0], counts[1]);
}

#[test]
fn send_pack_sync_does_not_duplicate_ordinary_push_export() {
    let directory = tempfile::tempdir().unwrap();
    let log = directory.path().join("spawns.log");
    let (repo, oid, note) =
        configured_source(Some(true), &[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    fs::write(&log, "").unwrap();
    repo.git(&[
        "push",
        target.path().to_str().unwrap(),
        &format!("{oid}:refs/heads/main"),
    ])
    .unwrap();
    repo.sync_daemon_force();
    let spawns = fs::read_to_string(&log).unwrap();
    assert_eq!(
        spawns.lines().filter(|command| *command == "push").count(),
        1,
        "{spawns}"
    );
    assert!(
        !spawns.lines().any(|command| command == "send-pack"),
        "{spawns}"
    );
    assert_eq!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid),
        Some(note)
    );
}

#[test]
fn send_pack_sync_failed_single_ref_preserves_remote() {
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    let (other, _, _) = source(None);
    fs::write(other.path().join("source.txt"), "human\nAI\nremote\n").unwrap();
    other
        .git_ai(&["checkpoint", "mock_known_human", "source.txt"])
        .unwrap();
    let remote_oid = other
        .stage_all_and_commit("remote advance")
        .unwrap()
        .commit_sha;
    other.filename("source.txt").assert_committed_lines(lines![
        "human".human(),
        "AI".ai(),
        "remote".human()
    ]);
    other
        .git_og(&[
            "send-pack",
            target.path().to_str().unwrap(),
            &format!("{remote_oid}:refs/heads/main"),
        ])
        .unwrap();
    assert!(
        send_pack(
            &repo,
            &[
                "send-pack",
                target.path().to_str().unwrap(),
                &format!("{oid}:refs/heads/main")
            ]
        )
        .is_err()
    );
    assert_eq!(
        target
            .git_og(&["rev-parse", "refs/heads/main"])
            .unwrap()
            .trim(),
        remote_oid
    );
    assert!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .is_none()
    );
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}

#[cfg(unix)]
#[test]
fn send_pack_sync_respects_receiver_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let (repo, oid, note) = source(Some(true));
    let target = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    let hooks = target.path().join("hooks");
    let hook = hooks.join("update");
    fs::write(&hook, "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$0.calls\"\ncase \"$1\" in refs/notes/ai) exit 1;; esac\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    target
        .git_og(&["config", "core.hooksPath", hooks.to_str().unwrap()])
        .unwrap();
    publish(&repo, &target, &oid);
    assert_eq!(
        fs::read_to_string(hooks.join("update.calls")).unwrap(),
        "refs/heads/main\nrefs/notes/ai\n"
    );
    assert!(
        repo.read_authorship_note_in_git_dir(target.path(), &oid)
            .is_none()
    );
    assert_eq!(repo.read_authorship_note(&oid), Some(note));
}
