use super::*;

worktree_test_wrappers! {
    fn notes_sync_push_reports_remote_note_update_failure() {
        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("push-locked.txt"), "local\n")
            .expect("failed to write push file");
        local
            .git_og(&["add", "push-locked.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "push locked notes commit"])
            .expect("commit should succeed");
        let commit_sha = local
            .git_og(&["rev-parse", "HEAD"])
            .expect("rev-parse should succeed")
            .trim()
            .to_string();
        local
            .git_og(&[
                "notes",
                "--ref=ai",
                "add",
                "-m",
                "push-locked-note",
                commit_sha.as_str(),
            ])
            .expect("adding local note should succeed");

        let remote_notes_dir = upstream.path().join("refs/notes");
        fs::create_dir_all(&remote_notes_dir).expect("remote notes dir should be creatable");
        fs::write(remote_notes_dir.join("ai.lock"), "stale lock\n")
            .expect("remote notes lock should be writable");

        local
            .git(&["push", "-u", "origin", "HEAD"])
            .expect("branch push should succeed before daemon notes side effect runs");

        let sync = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            local.sync_daemon_force();
        }));
        let panic_message = panic_payload_to_string(
            sync.expect_err("daemon sync must fail when notes push cannot update remote refs/notes/ai"),
        );
        assert!(
            panic_message.contains("daemon completion log reported an error"),
            "daemon sync must report notes push side-effect failure instead of silently leaving remote authorship missing for {}; got: {}",
            commit_sha,
            panic_message
        );
    }
}

worktree_test_wrappers! {
    fn notes_sync_push_to_explicit_path_pushes_authorship_to_same_destination() {
        let (local, _origin) = TestRepo::new_with_remote();
        let explicit_destination = repos::test_repo::TestRepo::new_bare();

        fs::write(local.path().join("push-explicit-path.txt"), "local\n")
            .expect("failed to write explicit path push file");
        local
            .git_og(&["add", "push-explicit-path.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "push explicit path notes commit"])
            .expect("commit should succeed");
        let commit_sha = local
            .git_og(&["rev-parse", "HEAD"])
            .expect("rev-parse should succeed")
            .trim()
            .to_string();
        local
            .git_og(&[
                "notes",
                "--ref=ai",
                "add",
                "-m",
                "push-explicit-path-note",
                commit_sha.as_str(),
            ])
            .expect("adding local note should succeed");

        let explicit_destination_path = explicit_destination.path().to_string_lossy().to_string();
        local
            .git(&[
                "push",
                explicit_destination_path.as_str(),
                "HEAD:refs/heads/main",
            ])
            .expect("branch push to explicit path should succeed");

        // Read through `local` so the daemon that ran the push side effect is
        // synced first: with concurrent family drains, the destination repo's
        // own family-scoped sync no longer (accidentally) fences the pushing
        // family's in-flight side effects.
        let pushed_note =
            local.read_authorship_note_in_git_dir(explicit_destination.path(), &commit_sha);
        assert!(
            pushed_note.is_some(),
            "git push to an explicit repository path must push authorship notes to that same destination for {}",
            commit_sha
        );
    }
}

worktree_test_wrappers! {
    fn notes_sync_push_propagates_authorship_notes_to_remote() {
        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("push-seed.txt"), "seed\n")
            .expect("failed to write push seed file");
        local
            .git_og(&["add", "push-seed.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "seed commit"])
            .expect("seed commit should succeed");

        let seed_sha = local
            .git_og(&["rev-parse", "HEAD"])
            .expect("rev-parse should succeed")
            .trim()
            .to_string();

        local
            .git_og(&[
                "notes",
                "--ref=ai",
                "add",
                "-m",
                "push-seed-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");

        local
            .git(&["push", "-u", "origin", "HEAD"])
            .expect("push should succeed");

        let remote_note = local.read_authorship_note_in_git_dir(upstream.path(), &seed_sha);
        assert!(
            remote_note.is_some(),
            "push should propagate authorship note for commit {} to upstream",
            seed_sha
        );
    }
}
