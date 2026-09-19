use super::*;
use git_ai::config::{NotesBackendConfig, NotesBackendKind};
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use repos::test_file::ExpectedLineExt;

#[test]
fn pull_note_import_failure_still_migrates_uncommitted_attribution() {
    let (local, upstream) = TestRepo::new_with_remote();
    fs::write(local.path().join("base.txt"), "base\n").unwrap();
    local.stage_all_and_commit("base").unwrap();
    local
        .filename("base.txt")
        .assert_committed_lines(crate::lines!["base".unattributed_human()]);
    local.git(&["push", "-u", "origin", "HEAD"]).unwrap();
    local.sync_daemon_force();
    let clone_dir = tempfile::tempdir().unwrap();
    let clone_path = clone_dir.path().join("contributor");
    local
        .git_og(&[
            "clone",
            upstream.path().to_str().unwrap(),
            clone_path.to_str().unwrap(),
        ])
        .unwrap();
    let contributor = TestRepo::new_at_path_with_daemon_scope(&clone_path, DaemonTestScope::Shared);
    fs::write(contributor.path().join("remote.txt"), "remote\n").unwrap();
    let remote_commit = contributor.stage_all_and_commit("remote").unwrap();
    contributor
        .filename("base.txt")
        .assert_committed_lines(crate::lines!["base".unattributed_human()]);
    contributor
        .filename("remote.txt")
        .assert_committed_lines(crate::lines!["remote".unattributed_human()]);
    contributor.git(&["push", "origin", "HEAD"]).unwrap();
    contributor.sync_daemon_force();

    local.git_ai(&["checkpoint", "human", "local.txt"]).unwrap();
    fs::write(local.path().join("local.txt"), "local AI\n").unwrap();
    local
        .git_ai(&["checkpoint", "mock_ai", "local.txt"])
        .unwrap();
    let repo =
        git_ai::operations::git::find_repository_in_path(local.path().to_str().unwrap()).unwrap();
    let notes_dir = repo.common_dir().join("refs/notes");
    fs::create_dir_all(&notes_dir).unwrap();
    let lock = notes_dir.join("ai.lock");
    fs::write(&lock, "locked\n").unwrap();
    let completions = local.daemon_total_completion_count();
    local
        .git_without_test_sync_for_test(&["pull", "--ff-only", "origin", "HEAD"], &[])
        .unwrap();
    let sync = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        local.wait_for_daemon_total_completion_count(completions, completions + 1);
    }));
    fs::remove_file(lock).unwrap();
    let message = panic_payload_to_string(sync.expect_err("notes import should fail"));
    assert!(
        message.contains("daemon completion log reported an error"),
        "{message}"
    );
    let log = repo
        .storage
        .working_log_for_base_commit(&remote_commit.commit_sha)
        .unwrap();
    assert!(
        log.read_all_checkpoints()
            .unwrap()
            .iter()
            .any(|checkpoint| checkpoint
                .entries
                .iter()
                .any(|entry| entry.file == "local.txt")),
        "successful pull must migrate the local AI checkpoint even when importing notes fails"
    );
    local.stage_all_and_commit("preserved local work").unwrap();
    local
        .filename("base.txt")
        .assert_committed_lines(crate::lines!["base".unattributed_human()]);
    local
        .filename("remote.txt")
        .assert_committed_lines(crate::lines!["remote".unattributed_human()]);
    local
        .filename("local.txt")
        .assert_committed_lines(crate::lines!["local AI".ai()]);
}

#[test]
fn notes_sync_http_backend_pull_rebase_preserves_force_pushed_target_note() {
    let server = ReferenceServer::start("127.0.0.1:0").expect("start notes reference server");
    let backend_url = server.base_url();
    let api_key = "notes-sync-http-rebase-test-key";
    let mut local = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", backend_url.as_str()),
        ("GIT_AI_API_KEY", api_key),
    ]);
    local.patch_git_ai_config(|patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Http,
            backend_url: Some(backend_url.clone()),
        });
    });
    let notes_db_path = local
        .test_home_path()
        .join(".git-ai")
        .join("internal")
        .join("notes-db");
    let upstream = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    let upstream_str = upstream.path().to_string_lossy().to_string();

    local
        .git_og(&["remote", "add", "origin", upstream_str.as_str()])
        .expect("add origin");
    let feature_path = local.path().join("feature.txt");
    fs::write(&feature_path, "base\n").expect("write base");
    local.git_og(&["add", "feature.txt"]).expect("add base");
    local
        .git_og(&["commit", "-m", "base commit"])
        .expect("commit base");
    let mut local_file = local.filename("feature.txt");
    local_file.assert_committed_lines(crate::lines!["base".unattributed_human()]);

    local
        .git_ai(&["checkpoint", "human", "feature.txt"])
        .expect("checkpoint before AI edit");
    fs::write(&feature_path, "base\nold AI line\n").expect("write old feature");
    local
        .git_ai(&["checkpoint", "mock_ai", "feature.txt"])
        .expect("checkpoint AI edit");
    local.git(&["add", "feature.txt"]).expect("add old feature");
    local
        .git(&["commit", "-m", "old feature version"])
        .expect("commit old feature");
    local.sync_daemon_force();
    local_file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "old AI line".ai(),
    ]);
    let old_commit = local
        .git_og(&["rev-parse", "HEAD"])
        .expect("read old feature commit")
        .trim()
        .to_string();
    let old_note = NotesDatabase::open_at_path(&notes_db_path)
        .expect("open notes db")
        .get_note(&old_commit)
        .expect("read old note")
        .expect("local rewrite source should have an HTTP-backed note");
    local
        .git_og(&["push", "-u", "origin", "HEAD"])
        .expect("push old feature");

    let remote_clone = unique_temp_path("notes-sync-http-rebase-remote");
    let remote_clone_str = remote_clone.to_string_lossy().to_string();
    run_git(&["clone", upstream_str.as_str(), remote_clone_str.as_str()]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "config",
        "user.name",
        "Test User",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "config",
        "user.email",
        "test@example.com",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "reset",
        "--hard",
        &format!("{old_commit}^"),
    ]);
    fs::write(remote_clone.join("feature.txt"), "base\nold AI line\n")
        .expect("write force-pushed feature");
    run_git(&["-C", remote_clone_str.as_str(), "add", "feature.txt"]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "commit",
        "-m",
        "force-pushed feature version",
    ]);
    let remote_sha = run_git(&["-C", remote_clone_str.as_str(), "rev-parse", "HEAD"]);
    let remote_repo =
        TestRepo::new_at_path_with_daemon_scope(&remote_clone, DaemonTestScope::NoDaemon);
    remote_repo
        .filename("feature.txt")
        .assert_committed_lines(crate::lines![
            "base".unattributed_human(),
            "old AI line".unattributed_human(),
        ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "push",
        "--force",
        "origin",
        "HEAD",
    ]);

    let mut remote_log =
        AuthorshipLog::deserialize_from_string(&old_note).expect("parse old authorship note");
    remote_log.metadata.base_commit_sha = "authoritative-remote-target".to_string();
    let remote_note = remote_log
        .serialize_to_string()
        .expect("serialize remote authorship note");
    server.store().put(remote_sha.clone(), remote_note.clone());
    assert_eq!(
        NotesDatabase::open_at_path(&notes_db_path)
            .expect("open notes db before pull")
            .get_note(&remote_sha)
            .expect("read target note before pull"),
        None,
        "precondition: local cache must be missing User A's rewritten note"
    );

    local
        .git(&["pull", "--rebase"])
        .expect("pull --rebase should succeed");
    local.sync_daemon_force();

    assert_eq!(
        local.git_og(&["rev-parse", "HEAD"]).unwrap().trim(),
        remote_sha,
        "Git should recognize the local patch in the force-pushed target"
    );
    assert_eq!(
        NotesDatabase::open_at_path(&notes_db_path)
            .expect("open notes db after pull")
            .get_note(&remote_sha)
            .expect("read target note after pull"),
        Some(remote_note),
        "transport hydration must cache User A's target note before rewrite shifting"
    );
    local_file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "old AI line".ai(),
    ]);
}

#[test]
fn test_pull_rebase_after_collaborator_restack_preserves_both_users_notes() {
    // User B has A(old) + B locally. User A restacks and force-pushes A(new)
    // with a new note. B's pull --rebase must hydrate A(new)'s note before
    // replaying B, while still shifting B's locally available source note.
    let (local, upstream) = TestRepo::new_with_remote();
    let file_path = local.path().join("stack.txt");

    std::fs::write(&file_path, "base\n").unwrap();
    local.stage_all_and_commit("initial").unwrap();
    let mut local_file = local.filename("stack.txt");
    local_file.assert_committed_lines(crate::lines!["base".unattributed_human()]);

    local.git_ai(&["checkpoint", "human", "stack.txt"]).unwrap();
    std::fs::write(&file_path, "base\nA old AI line\n").unwrap();
    local
        .git_ai(&["checkpoint", "mock_ai", "stack.txt"])
        .unwrap();
    let old_a = local.stage_all_and_commit("A before restack").unwrap();
    local_file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "A old AI line".ai(),
    ]);
    local.git(&["push", "-u", "origin", "main"]).unwrap();

    let b_path = local.path().join("b.txt");
    local.git_ai(&["checkpoint", "human", "b.txt"]).unwrap();
    std::fs::write(&b_path, "B AI line\n").unwrap();
    local.git_ai(&["checkpoint", "mock_ai", "b.txt"]).unwrap();
    let old_b = local.stage_all_and_commit("B local work").unwrap();
    local_file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "A old AI line".ai(),
    ]);
    let mut b_file = local.filename("b.txt");
    b_file.assert_committed_lines(crate::lines!["B AI line".ai()]);

    let contributor_parent = tempfile::tempdir().expect("contributor temp dir");
    let contributor_path = contributor_parent.path().join("contributor");
    local
        .git_og(&[
            "clone",
            upstream.path().to_str().unwrap(),
            contributor_path.to_str().unwrap(),
        ])
        .unwrap();
    let contributor =
        TestRepo::new_at_path_with_daemon_scope(&contributor_path, DaemonTestScope::Shared);
    contributor
        .git(&["reset", "--hard", &format!("{}^", old_a.commit_sha)])
        .unwrap();

    let contributor_file_path = contributor.path().join("stack.txt");
    contributor
        .git_ai(&["checkpoint", "human", "stack.txt"])
        .unwrap();
    std::fs::write(
        &contributor_file_path,
        "base\nA old AI line\nA restack AI line\n",
    )
    .unwrap();
    contributor
        .git_ai(&["checkpoint", "mock_ai", "stack.txt"])
        .unwrap();
    let new_a = contributor.stage_all_and_commit("A after restack").unwrap();
    let mut contributor_file = contributor.filename("stack.txt");
    contributor_file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "A old AI line".ai(),
        "A restack AI line".ai(),
    ]);
    contributor
        .git(&["push", "--force", "origin", "HEAD:main"])
        .unwrap();
    contributor.sync_daemon_force();
    contributor
        .git_og(&["push", "--force", "origin", "refs/notes/ai:refs/notes/ai"])
        .unwrap();

    assert!(
        local.read_authorship_note(&new_a.commit_sha).is_none(),
        "precondition: B must be missing A's restacked note"
    );
    local.git(&["pull", "--rebase"]).unwrap();

    let new_b = local
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    assert_ne!(
        new_b, old_b.commit_sha,
        "B's local commit should be replayed"
    );
    assert_ne!(
        new_b, new_a.commit_sha,
        "B's replayed commit should remain on top of A"
    );
    assert_eq!(
        local.git(&["rev-parse", "HEAD^"]).unwrap().trim(),
        new_a.commit_sha,
        "B's replayed commit should be based on A's restacked commit"
    );
    local_file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "A old AI line".ai(),
        "A restack AI line".ai(),
    ]);
    b_file.assert_committed_lines(crate::lines!["B AI line".ai()]);
}
