use super::*;

worktree_test_wrappers! {
    fn notes_sync_clone_fetches_authorship_notes_from_origin() {

        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("clone-seed.txt"), "seed\n")
            .expect("failed to write clone seed file");
        local
            .git_og(&["add", "clone-seed.txt"])
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
                "clone-seed-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("pushing branch should succeed");
        local
            .git_og(&["push", "origin", "refs/notes/ai"])
            .expect("pushing notes should succeed");

        let clone_dir = unique_temp_path("notes-sync-clone");
        let clone_dir_str = clone_dir.to_string_lossy().to_string();
        let upstream_str = upstream.path().to_string_lossy().to_string();
        let _ = fs::remove_dir_all(&clone_dir);

        local
            .git(&["clone", upstream_str.as_str(), clone_dir_str.as_str()])
            .expect("clone should succeed");

        let cloned_note = read_note_from_worktree(&clone_dir, &seed_sha);
        assert!(
            cloned_note.is_some(),
            "cloned repository should have fetched authorship notes for commit {}",
            seed_sha
        );
    }
}

worktree_test_wrappers! {
    fn notes_sync_clone_reports_local_note_update_failure() {
        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("clone-locked-seed.txt"), "seed\n")
            .expect("failed to write clone locked seed file");
        local
            .git_og(&["add", "clone-locked-seed.txt"])
            .expect("add should succeed");
        local
            .git_og(&["commit", "-m", "clone locked seed commit"])
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
                "clone-locked-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("pushing branch should succeed");
        local
            .git_og(&["push", "origin", "refs/notes/ai"])
            .expect("pushing notes should succeed");

        let template_dir = unique_temp_path("notes-sync-clone-template");
        let template_notes_dir = template_dir.join("refs/notes");
        fs::create_dir_all(&template_notes_dir).expect("template notes dir should be creatable");
        fs::write(template_notes_dir.join("ai.lock"), "stale lock\n")
            .expect("template notes lock should be writable");

        let clone_dir = unique_temp_path("notes-sync-clone-locked");
        let clone_dir_str = clone_dir.to_string_lossy().to_string();
        let upstream_str = upstream.path().to_string_lossy().to_string();
        let template_str = template_dir.to_string_lossy().to_string();
        let _ = fs::remove_dir_all(&clone_dir);

        let cloned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            local.git(&[
                "clone",
                "--template",
                template_str.as_str(),
                upstream_str.as_str(),
                clone_dir_str.as_str(),
            ])
        }));
        let panic_message = panic_payload_to_string(cloned.expect_err(
            "clone target daemon sync must fail when notes import cannot update refs/notes/ai",
        ));
        assert!(
            panic_message.contains("daemon completion log reported an error"),
            "clone target daemon sync must report notes import failure instead of timing out or silently losing authorship for {}; got: {}",
            seed_sha,
            panic_message
        );
    }
}

worktree_test_wrappers! {
    fn notes_sync_fetch_does_not_import_authorship_notes() {
        let (local, _upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("fetch-seed.txt"), "seed\n")
            .expect("failed to write fetch seed file");
        local
            .git_og(&["add", "fetch-seed.txt"])
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
                "fetch-seed-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("pushing branch should succeed");
        local
            .git_og(&["push", "origin", "refs/notes/ai"])
            .expect("pushing notes should succeed");

        let _ = local.git_og(&["update-ref", "-d", "refs/notes/ai"]);
        assert!(
            local.read_authorship_note(&seed_sha).is_none(),
            "local note should be absent before fetch"
        );

        local
            .git(&["fetch", "origin"])
            .expect("fetch should succeed");

        let fetched_note = local.read_authorship_note(&seed_sha);
        assert!(
            fetched_note.is_none(),
            "plain git fetch should not import authorship note for commit {}",
            seed_sha
        );
    }
}
