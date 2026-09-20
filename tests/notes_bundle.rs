#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use git_ai::config::{NotesBackendConfig, NotesBackendKind};
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use repos::test_file::ExpectedLineExt;
use repos::test_repo::{DaemonTestScope, TestRepo, get_binary_path};
use std::fs;
use std::path::Path;

fn commit(repo: &TestRepo, message: &str) -> String {
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", message]).unwrap();
    repo.git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

fn source(kind: NotesBackendKind) -> (TestRepo, String, String) {
    let repo = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind,
            backend_url: None,
        });
    });
    fs::write(repo.path().join("source.txt"), "human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "source.txt"])
        .unwrap();
    fs::write(repo.path().join("source.txt"), "human\nAI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    let selected = commit(&repo, "selected");
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "AI".ai()]);
    fs::write(repo.path().join("other.txt"), "unrelated AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "other.txt"])
        .unwrap();
    let unrelated = commit(&repo, "unrelated");
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "AI".ai()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["unrelated AI".ai()]);
    (repo, selected, unrelated)
}

fn export(repo: &TestRepo, path: &Path, oids: &[&str]) -> Result<String, String> {
    let mut args = vec!["notes", "bundle", path.to_str().unwrap()];
    args.extend_from_slice(oids);
    repo.git_ai(&args)
}

fn roundtrip(kind: NotesBackendKind) {
    let (repo, selected, unrelated) = source(kind);
    let dir = tempfile::tempdir().unwrap();
    let code = dir.path().join("code.bundle");
    let metadata = dir.path().join("notes.bundle");
    let refs = repo
        .git_og(&["for-each-ref", "--format=%(refname) %(objectname)"])
        .unwrap();
    let index = fs::read(repo.path().join(".git/index")).unwrap();
    repo.git_og(&["bundle", "create", code.to_str().unwrap(), "HEAD"])
        .unwrap();
    let code_bytes = fs::read(&code).unwrap();
    export(&repo, &metadata, &[&selected]).unwrap();
    assert_eq!(fs::read(&code).unwrap(), code_bytes);
    assert_eq!(
        repo.git_og(&["for-each-ref", "--format=%(refname) %(objectname)"])
            .unwrap(),
        refs
    );
    assert_eq!(fs::read(repo.path().join(".git/index")).unwrap(), index);
    assert!(
        repo.git_og(&["bundle", "verify", metadata.to_str().unwrap()])
            .is_ok()
    );
    let heads = repo
        .git_og(&["bundle", "list-heads", metadata.to_str().unwrap()])
        .unwrap();
    assert_eq!(
        heads.split_whitespace().collect::<Vec<_>>().len(),
        2,
        "{heads}"
    );
    assert!(heads.trim().ends_with(" refs/notes/ai"), "{heads}");

    let receiver = TestRepo::new();
    receiver
        .git_og(&["fetch", code.to_str().unwrap(), "HEAD:refs/heads/imported"])
        .unwrap();
    receiver
        .git_og(&[
            "fetch",
            metadata.to_str().unwrap(),
            "refs/notes/ai:refs/notes/ai",
        ])
        .unwrap();
    assert!(receiver.read_authorship_note(&selected).is_some());
    assert!(receiver.read_authorship_note(&unrelated).is_none());
    receiver.git(&["switch", "--detach", &selected]).unwrap();
    receiver
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "AI".ai()]);
    receiver.git(&["switch", "--detach", &unrelated]).unwrap();
    receiver
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "AI".ai()]);
    receiver
        .filename("other.txt")
        .assert_committed_lines(lines!["unrelated AI".unattributed_human()]);
}

#[test]
fn notes_bundle_git_notes_roundtrip_filters_unselected_commit() {
    roundtrip(NotesBackendKind::GitNotes);
}

#[test]
fn notes_bundle_sqlite_roundtrip_filters_unselected_commit() {
    roundtrip(NotesBackendKind::Sqlite);
}

#[test]
fn notes_bundle_preserves_existing_output() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    fs::write(&output, "existing artifact").unwrap();
    assert!(export(&repo, &output, &[&selected]).is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), "existing artifact");
}

#[test]
fn notes_bundle_unknown_commit_has_no_artifact() {
    let (repo, _, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    assert!(export(&repo, &output, &[&"a".repeat(40)]).is_err());
    assert!(!output.exists());
}

#[test]
fn notes_bundle_native_bundle_does_not_materialize_sqlite() {
    let (repo, _, _) = source(NotesBackendKind::Sqlite);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("native.bundle");
    let refs = repo
        .git_og(&["for-each-ref", "--format=%(refname) %(objectname)"])
        .unwrap();
    assert!(!refs.contains("refs/notes/ai"));
    repo.git(&["bundle", "create", output.to_str().unwrap(), "--all"])
        .unwrap();
    let heads = repo
        .git_og(&["bundle", "list-heads", output.to_str().unwrap()])
        .unwrap();
    assert!(!heads.contains("refs/notes/ai"));
    assert_eq!(
        repo.git_og(&["for-each-ref", "--format=%(refname) %(objectname)"])
            .unwrap(),
        refs
    );
}

#[test]
fn notes_bundle_reports_missing_notes_without_inventing_them() {
    let (repo, selected, unrelated) = source(NotesBackendKind::GitNotes);
    repo.git_og(&["notes", "--ref=ai", "remove", &unrelated])
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let text = export(&repo, &output, &[&selected, &unrelated]).unwrap();
    assert!(text.contains("1") && text.contains("missing"), "{text}");
    assert!(output.is_file());
}

#[test]
fn notes_bundle_rejects_repository_environment_before_writing() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let git_dir = repo.path().join(".git");
    let config = fs::read(git_dir.join("config")).unwrap();
    let refs = repo.git_og(&["show-ref"]).unwrap();
    let result = repo.git_ai_with_env(
        &["notes", "bundle", output.to_str().unwrap(), &selected],
        &[("GIT_COMMON_DIR", git_dir.to_str().unwrap())],
    );
    assert_eq!(fs::read(git_dir.join("config")).unwrap(), config);
    assert_eq!(repo.git_og(&["show-ref"]).unwrap(), refs);
    assert!(result.unwrap_err().contains("GIT_COMMON_DIR"));
    assert!(!output.exists());
}

#[test]
fn notes_bundle_rejects_inherited_command_config() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let result = repo.git_ai_with_env(
        &["notes", "bundle", output.to_str().unwrap(), &selected],
        &[
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "core.logAllRefUpdates"),
            ("GIT_CONFIG_VALUE_0", "true"),
        ],
    );
    assert!(result.unwrap_err().contains("GIT_CONFIG_COUNT"));
    assert!(!output.exists());
}

#[test]
fn notes_bundle_runs_as_native_git_external_command() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let binary = get_binary_path();
    repo.git_without_test_sync_for_test(
        &["ai", "notes", "bundle", output.to_str().unwrap(), &selected],
        &[("GIT_EXEC_PATH", binary.parent().unwrap().to_str().unwrap())],
    )
    .unwrap();
    assert!(output.is_file());
}

#[test]
fn notes_bundle_rejects_invalid_selection_without_an_artifact() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let tree = repo.git_og(&["rev-parse", "HEAD^{tree}"]).unwrap();
    for ids in [
        vec![],
        vec!["HEAD"],
        vec![&selected[..8]],
        vec!["--all"],
        vec!["0000000000000000000000000000000000000000"],
        vec![tree.trim()],
        vec![selected.as_str(); 33],
    ] {
        assert!(export(&repo, &output, &ids).is_err(), "{ids:?}");
        assert!(!output.exists());
    }
    let text = export(&repo, &output, &[&selected, &selected.to_uppercase()]).unwrap();
    assert!(text.contains("Exported 1 note(s)"), "{text}");
}

#[test]
fn notes_bundle_no_available_notes_has_no_artifact() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    repo.git_og(&["notes", "--ref=ai", "remove", &selected])
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let error = export(&repo, &output, &[&selected]).unwrap_err();
    assert!(error.contains("no authorship notes"), "{error}");
    assert!(!output.exists());
}

#[test]
fn notes_bundle_rejects_corrupt_mismatched_and_oversized_notes() {
    let (repo, selected, unrelated) = source(NotesBackendKind::GitNotes);
    let original = repo.read_authorship_note(&selected).unwrap();
    let mut wrong_schema = AuthorshipLog::deserialize_from_string(&original).unwrap();
    wrong_schema.metadata.schema_version = "authorship/999.0.0".into();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let input = dir.path().join("input.note");
    let wrong_commit = repo.read_authorship_note(&unrelated).unwrap();
    for bad_note in [
        "invalid note".to_string(),
        wrong_schema.serialize_to_string().unwrap(),
        wrong_commit,
        "x".repeat(1024 * 1024 + 1),
    ] {
        fs::write(&input, &bad_note).unwrap();
        repo.git_og(&[
            "notes",
            "--ref=ai",
            "add",
            "-f",
            "-F",
            input.to_str().unwrap(),
            &selected,
        ])
        .unwrap();
        let before = repo.read_authorship_note(&selected);
        assert!(export(&repo, &output, &[&selected]).is_err());
        assert!(!output.exists());
        assert_eq!(repo.read_authorship_note(&selected), before);
    }
}

#[test]
fn notes_bundle_collection_opt_out_does_not_export() {
    let (mut repo, selected, _) = source(NotesBackendKind::GitNotes);
    repo.patch_git_ai_config(|patch| patch.allowed_repositories = Some(vec![]));
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let error = export(&repo, &output, &[&selected]).unwrap_err();
    assert!(error.contains("collection is disabled"), "{error}");
    assert!(!output.exists());
}

#[test]
fn notes_bundle_http_is_explicitly_unsupported_without_a_request() {
    let (mut repo, selected, _) = source(NotesBackendKind::GitNotes);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    repo.patch_git_ai_config(|patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Http,
            backend_url: Some(address),
        });
    });
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("notes.bundle");
    let error = export(&repo, &output, &[&selected]).unwrap_err();
    assert!(error.contains("supports git_notes and sqlite"), "{error}");
    assert!(!output.exists());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[cfg(unix)]
#[test]
fn notes_bundle_preserves_symlink_output_and_target() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let output = dir.path().join("notes.bundle");
    fs::write(&target, "keep this").unwrap();
    std::os::unix::fs::symlink(&target, &output).unwrap();
    assert!(export(&repo, &output, &[&selected]).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep this");
    assert_eq!(fs::read_link(&output).unwrap(), target);
}

#[test]
fn notes_bundle_source_hooks_do_not_run_for_temporary_refs() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let hooks = dir.path().join("hooks");
    fs::create_dir(&hooks).unwrap();
    let marker = dir.path().join("hook-ran");
    let hook = hooks.join("reference-transaction");
    repos::write_executable_script(
        &hook,
        format!(
            "#!/bin/sh\nprintf ran > '{}'\nexit 1\n",
            marker.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    repo.git_og(&[
        "config",
        "--global",
        "core.hooksPath",
        hooks.to_str().unwrap(),
    ])
    .unwrap();
    let output = dir.path().join("notes.bundle");
    export(&repo, &output, &[&selected]).unwrap();
    assert!(!marker.exists());
}

#[test]
fn notes_bundle_preserves_pending_index_and_working_log() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    fs::write(repo.path().join("source.txt"), "human\nAI\npending AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    repo.git(&["add", "source.txt"]).unwrap();
    repo.sync_daemon_force();
    let logs = repo.path().join(".git/ai/working_logs");
    let snapshot = || {
        fn collect(path: &Path, files: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    collect(&entry.path(), files);
                } else {
                    files.push((entry.path(), fs::read(entry.path()).unwrap()));
                }
            }
        }
        let mut files = Vec::new();
        collect(&logs, &mut files);
        files.sort();
        files
    };
    let before = snapshot();
    assert!(!before.is_empty());
    let index = fs::read(repo.path().join(".git/index")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    export(&repo, &dir.path().join("notes.bundle"), &[&selected]).unwrap();
    assert_eq!(snapshot(), before);
    assert_eq!(fs::read(repo.path().join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(repo.path().join("source.txt")).unwrap(),
        "human\nAI\npending AI\n"
    );
}

#[test]
fn notes_bundle_exports_from_linked_worktree_subdirectory() {
    let (repo, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let linked = dir.path().join("linked");
    repo.git_og(&[
        "worktree",
        "add",
        "--detach",
        linked.to_str().unwrap(),
        &selected,
    ])
    .unwrap();
    let cwd = linked.join("subdir");
    fs::create_dir(&cwd).unwrap();
    let output = dir.path().join("notes.bundle");
    repo.git_ai_from_working_dir(
        &cwd,
        &["notes", "bundle", output.to_str().unwrap(), &selected],
    )
    .unwrap();
    assert!(output.is_file());
    repo.git_og(&["bundle", "verify", output.to_str().unwrap()])
        .unwrap();
}

#[test]
fn notes_bundle_sqlite_authority_wins_over_conflicting_git_note() {
    let (repo, selected, _) = source(NotesBackendKind::Sqlite);
    repo.git_og(&[
        "notes",
        "--ref=ai",
        "add",
        "-m",
        "invalid conflicting note",
        &selected,
    ])
    .unwrap();
    let before = repo.read_authorship_note(&selected);
    let dir = tempfile::tempdir().unwrap();
    export(&repo, &dir.path().join("notes.bundle"), &[&selected]).unwrap();
    assert_eq!(repo.read_authorship_note(&selected), before);
    assert!(
        repo.git_og(&["notes", "--ref=ai", "show", &selected])
            .unwrap()
            .contains("invalid conflicting note")
    );
}

#[test]
fn notes_bundle_sha256_roundtrip_preserves_note_bytes() {
    let (original, selected, _) = source(NotesBackendKind::GitNotes);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sha256");
    original
        .git_og(&["init", "--object-format=sha256", path.to_str().unwrap()])
        .unwrap();
    let repo = TestRepo::new_at_path_with_daemon_scope(&path, DaemonTestScope::NoDaemon);
    fs::write(path.join("source.txt"), "human\nAI\n").unwrap();
    repo.git_og(&["add", "source.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "source"]).unwrap();
    let oid = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(oid.len(), 64);
    repo.filename("source.txt").assert_committed_lines(lines![
        "human".unattributed_human(),
        "AI".unattributed_human()
    ]);
    let mut note =
        AuthorshipLog::deserialize_from_string(&original.read_authorship_note(&selected).unwrap())
            .unwrap();
    note.metadata.base_commit_sha = oid.clone();
    let input = dir.path().join("input.note");
    fs::write(&input, note.serialize_to_string().unwrap()).unwrap();
    repo.git_og(&[
        "notes",
        "--ref=ai",
        "add",
        "-F",
        input.to_str().unwrap(),
        &oid,
    ])
    .unwrap();
    let expected = repo.git_og(&["notes", "--ref=ai", "show", &oid]).unwrap();
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "AI".ai()]);
    let output = dir.path().join("notes.bundle");
    export(&repo, &output, &[&oid]).unwrap();
    repo.git_og(&["bundle", "verify", output.to_str().unwrap()])
        .unwrap();
    repo.git_og(&[
        "fetch",
        output.to_str().unwrap(),
        "refs/notes/ai:refs/notes/received",
    ])
    .unwrap();
    assert_eq!(
        repo.git_og(&["notes", "--ref=received", "show", &oid])
            .unwrap(),
        expected
    );
}

#[test]
fn notes_bundle_git_process_count_is_independent_of_selection_size() {
    for kind in [NotesBackendKind::GitNotes, NotesBackendKind::Sqlite] {
        let (repo, first, second) = source(kind);
        let mut ids = vec![first, second];
        for i in 2..8 {
            let text = format!("AI {i}");
            fs::write(repo.path().join("other.txt"), format!("{text}\n")).unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", "other.txt"])
                .unwrap();
            ids.push(commit(&repo, &format!("commit {i}")));
            repo.filename("source.txt")
                .assert_committed_lines(lines!["human".human(), "AI".ai()]);
            repo.filename("other.txt")
                .assert_committed_lines(lines![text.ai()]);
        }
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("spawns.log");
        let mut counts = Vec::new();
        for count in [1, 8] {
            let output = dir.path().join(format!("notes-{count}.bundle"));
            fs::write(&log, "").unwrap();
            let mut args = vec!["notes", "bundle", output.to_str().unwrap()];
            args.extend(ids[..count].iter().map(String::as_str));
            repo.git_ai_with_env(&args, &[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())])
                .unwrap();
            let spawns = fs::read_to_string(&log).unwrap();
            let commands: Vec<_> = spawns.lines().collect();
            println!("notes bundle {kind:?}, {count} selected commits: {commands:?}");
            for expected in ["init", "fast-import", "bundle"] {
                assert_eq!(
                    commands
                        .iter()
                        .filter(|command| **command == expected)
                        .count(),
                    1
                );
            }
            assert!(commands.len() <= 12, "{spawns}");
            counts.push(commands.len());
        }
        assert_eq!(counts[0], counts[1], "{kind:?}: {counts:?}");
    }
}
