use super::*;

worktree_test_wrappers! {
    fn notes_sync_pull_fast_forward_imports_authorship_notes() {
        let (local, upstream) = TestRepo::new_with_remote();
        let default_branch = local.current_branch();

        fs::write(local.path().join("pull-base.txt"), "base\n")
            .expect("failed to write pull base file");
        local
            .git_og(&["add", "pull-base.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "base commit"])
            .expect("base commit should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("initial push should succeed");

        let remote_clone = unique_temp_path("notes-sync-pull-remote");
        let remote_clone_str = remote_clone.to_string_lossy().to_string();
        let upstream_str = upstream.path().to_string_lossy().to_string();
        let _ = fs::remove_dir_all(&remote_clone);

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

        fs::write(remote_clone.join("pull-remote.txt"), "remote\n")
            .expect("failed to write remote pull file");
        run_git(&["-C", remote_clone_str.as_str(), "add", "pull-remote.txt"]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "commit",
            "-m",
            "remote pull commit",
        ]);

        let remote_sha = run_git(&["-C", remote_clone_str.as_str(), "rev-parse", "HEAD"]);

        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "notes",
            "--ref=ai",
            "add",
            "-m",
            "pull-remote-note",
            remote_sha.as_str(),
        ]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "push",
            "origin",
            default_branch.as_str(),
        ]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "push",
            "origin",
            "refs/notes/ai",
        ]);

        assert!(
            local.read_authorship_note(&remote_sha).is_none(),
            "local note should be absent before pull"
        );

        local
            .git(&["pull", "--ff-only", "origin", default_branch.as_str()])
            .expect("pull --ff-only should succeed");

        let pulled_note = local.read_authorship_note(&remote_sha);
        assert!(
            pulled_note.is_some(),
            "pull should import authorship note for remote commit {}",
            remote_sha
        );
    }
}

worktree_test_wrappers! {
    fn notes_sync_pull_reports_local_note_update_failure() {
        let (local, upstream) = TestRepo::new_with_remote();
        let default_branch = local.current_branch();

        fs::write(local.path().join("pull-base.txt"), "base\n")
            .expect("failed to write pull base file");
        local
            .git_og(&["add", "pull-base.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "base commit"])
            .expect("base commit should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("initial push should succeed");

        let remote_clone = unique_temp_path("notes-sync-pull-locked-remote");
        let remote_clone_str = remote_clone.to_string_lossy().to_string();
        let upstream_str = upstream.path().to_string_lossy().to_string();
        let _ = fs::remove_dir_all(&remote_clone);

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

        fs::write(remote_clone.join("pull-locked.txt"), "remote\n")
            .expect("failed to write remote pull file");
        run_git(&["-C", remote_clone_str.as_str(), "add", "pull-locked.txt"]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "commit",
            "-m",
            "remote pull commit with locked notes",
        ]);

        let remote_sha = run_git(&["-C", remote_clone_str.as_str(), "rev-parse", "HEAD"]);

        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "notes",
            "--ref=ai",
            "add",
            "-m",
            "pull-locked-note",
            remote_sha.as_str(),
        ]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "push",
            "origin",
            default_branch.as_str(),
        ]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "push",
            "origin",
            "refs/notes/ai",
        ]);

        assert!(
            local.read_authorship_note(&remote_sha).is_none(),
            "local note should be absent before pull"
        );

        let notes_dir = local.path().join(".git/refs/notes");
        fs::create_dir_all(&notes_dir).expect("notes dir should be creatable");
        fs::write(notes_dir.join("ai.lock"), "stale lock\n")
            .expect("notes lock should be writable");

        local
            .git(&["pull", "--ff-only", "origin", default_branch.as_str()])
            .expect("pull --ff-only should succeed before daemon notes side effect runs");

        let sync = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            local.sync_daemon_force();
        }));
        let panic_message = panic_payload_to_string(
            sync.expect_err("daemon sync must fail when pull notes import cannot update refs/notes/ai"),
        );
        assert!(
            panic_message.contains("daemon completion log reported an error"),
            "daemon sync must report notes side-effect failure instead of silently losing authorship for {}; got: {}",
            remote_sha,
            panic_message
        );
    }
}

worktree_test_wrappers! {
    fn notes_sync_pull_fast_forward_syncs_only_selected_remote() {
        let (local, upstream) = TestRepo::new_with_remote();
        let backup = repos::test_repo::TestRepo::new_bare();
        let default_branch = local.current_branch();

        fs::write(local.path().join("pull-base.txt"), "base\n")
            .expect("failed to write pull base file");
        local
            .git_og(&["add", "pull-base.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "base commit"])
            .expect("base commit should succeed");

        let base_sha = local
            .git_og(&["rev-parse", "HEAD"])
            .expect("rev-parse should succeed")
            .trim()
            .to_string();

        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("initial push to origin should succeed");

        let backup_path = backup.path().to_string_lossy().to_string();
        local
            .git_og(&["remote", "add", "backup", backup_path.as_str()])
            .expect("adding backup remote should succeed");
        local
            .git_og(&["push", "backup", "HEAD"])
            .expect("initial push to backup should succeed");

        let backup_clone = unique_temp_path("notes-sync-pull-backup-remote");
        let backup_clone_str = backup_clone.to_string_lossy().to_string();
        let _ = fs::remove_dir_all(&backup_clone);

        run_git(&["clone", backup_path.as_str(), backup_clone_str.as_str()]);
        run_git(&[
            "-C",
            backup_clone_str.as_str(),
            "config",
            "user.name",
            "Test User",
        ]);
        run_git(&[
            "-C",
            backup_clone_str.as_str(),
            "config",
            "user.email",
            "test@example.com",
        ]);
        run_git(&[
            "-C",
            backup_clone_str.as_str(),
            "notes",
            "--ref=ai",
            "add",
            "-m",
            "backup-remote-note",
            base_sha.as_str(),
        ]);
        run_git(&[
            "-C",
            backup_clone_str.as_str(),
            "push",
            "origin",
            "refs/notes/ai",
        ]);

        let origin_clone = unique_temp_path("notes-sync-pull-origin-remote");
        let origin_clone_str = origin_clone.to_string_lossy().to_string();
        let upstream_str = upstream.path().to_string_lossy().to_string();
        let _ = fs::remove_dir_all(&origin_clone);

        run_git(&["clone", upstream_str.as_str(), origin_clone_str.as_str()]);
        run_git(&[
            "-C",
            origin_clone_str.as_str(),
            "config",
            "user.name",
            "Test User",
        ]);
        run_git(&[
            "-C",
            origin_clone_str.as_str(),
            "config",
            "user.email",
            "test@example.com",
        ]);

        fs::write(origin_clone.join("pull-selected-remote.txt"), "remote\n")
            .expect("failed to write selected remote file");
        run_git(&["-C", origin_clone_str.as_str(), "add", "pull-selected-remote.txt"]);
        run_git(&[
            "-C",
            origin_clone_str.as_str(),
            "commit",
            "-m",
            "remote pull commit",
        ]);

        let remote_sha = run_git(&["-C", origin_clone_str.as_str(), "rev-parse", "HEAD"]);

        run_git(&[
            "-C",
            origin_clone_str.as_str(),
            "notes",
            "--ref=ai",
            "add",
            "-m",
            "origin-remote-note",
            remote_sha.as_str(),
        ]);
        run_git(&[
            "-C",
            origin_clone_str.as_str(),
            "push",
            "origin",
            default_branch.as_str(),
        ]);
        run_git(&[
            "-C",
            origin_clone_str.as_str(),
            "push",
            "origin",
            "refs/notes/ai",
        ]);

        assert!(
            local.read_authorship_note(&base_sha).is_none(),
            "backup remote note should be absent before pull"
        );
        assert!(
            local.read_authorship_note(&remote_sha).is_none(),
            "origin remote note should be absent before pull"
        );

        local
            .git(&["pull", "--ff-only", "origin", default_branch.as_str()])
            .expect("pull --ff-only should succeed");

        let pulled_origin_note = local.read_authorship_note(&remote_sha);
        assert!(
            pulled_origin_note.is_some(),
            "pull should import authorship note for selected remote commit {}",
            remote_sha
        );

        let leaked_backup_note = local.read_authorship_note(&base_sha);
        assert!(
            leaked_backup_note.is_none(),
            "pull from origin should not import backup remote note for commit {}",
            base_sha
        );
    }
}

worktree_test_wrappers! {
    fn notes_sync_pull_rebase_imports_authorship_notes() {
        let (local, upstream) = TestRepo::new_with_remote();
        let default_branch = local.current_branch();

        fs::write(local.path().join("rebase-base.txt"), "base\n")
            .expect("failed to write rebase base file");
        local
            .git_og(&["add", "rebase-base.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "base commit"])
            .expect("base commit should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("initial push should succeed");

        fs::write(local.path().join("local-only.txt"), "local\n")
            .expect("failed to write local-only file");
        local
            .git_og(&["add", "local-only.txt"])
            .expect("add local-only should succeed");
        local
            .git_og(&["commit", "-m", "local commit"])
            .expect("local commit should succeed");

        let remote_clone = unique_temp_path("notes-sync-rebase-remote");
        let remote_clone_str = remote_clone.to_string_lossy().to_string();
        let upstream_str = upstream.path().to_string_lossy().to_string();
        let _ = fs::remove_dir_all(&remote_clone);

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

        fs::write(remote_clone.join("remote-only.txt"), "remote\n")
            .expect("failed to write remote-only file");
        run_git(&["-C", remote_clone_str.as_str(), "add", "remote-only.txt"]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "commit",
            "-m",
            "remote commit",
        ]);

        let remote_sha = run_git(&["-C", remote_clone_str.as_str(), "rev-parse", "HEAD"]);

        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "notes",
            "--ref=ai",
            "add",
            "-m",
            "pull-rebase-remote-note",
            remote_sha.as_str(),
        ]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "push",
            "origin",
            default_branch.as_str(),
        ]);
        run_git(&[
            "-C",
            remote_clone_str.as_str(),
            "push",
            "origin",
            "refs/notes/ai",
        ]);

        assert!(
            local.read_authorship_note(&remote_sha).is_none(),
            "local note should be absent before pull --rebase"
        );

        local
            .git(&["pull", "--rebase", "origin", default_branch.as_str()])
            .expect("pull --rebase should succeed");

        let pulled_note = local.read_authorship_note(&remote_sha);
        assert!(
            pulled_note.is_some(),
            "pull --rebase should import authorship note for remote commit {}",
            remote_sha
        );
    }
}
