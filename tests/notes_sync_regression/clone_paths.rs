use super::*;

worktree_test_wrappers! {
    fn notes_sync_clone_relative_target_from_external_cwd_fetches_authorship_notes() {
        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("clone-relative-seed.txt"), "seed\n")
            .expect("failed to write clone-relative seed file");
        local
            .git_og(&["add", "clone-relative-seed.txt"])
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
                "clone-relative-seed-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("pushing branch should succeed");
        local
            .git_og(&["push", "origin", "refs/notes/ai"])
            .expect("pushing notes should succeed");

        let external_cwd = unique_temp_path("notes-sync-clone-relative-cwd");
        let _ = fs::remove_dir_all(&external_cwd);
        fs::create_dir_all(&external_cwd).expect("failed to create external cwd");

        let relative_target = "nested/relative-clone";
        let upstream_str = upstream.path().to_string_lossy().to_string();

        local
            .git_from_working_dir(&external_cwd, &["clone", upstream_str.as_str(), relative_target])
            .expect("clone from external cwd should succeed");

        let clone_dir = external_cwd.join(relative_target);
        assert!(
            clone_dir.exists(),
            "relative clone target should exist at {}",
            clone_dir.display()
        );

        let cloned_note = read_note_from_worktree(&clone_dir, &seed_sha);
        assert!(
            cloned_note.is_some(),
            "cloned repository should have fetched authorship notes for commit {}",
            seed_sha
        );
    }
}

// Regression test: clone from a non-repo directory must be handled from trace2
// alone because there is no existing repository context for the clone target.
worktree_test_wrappers! {
    fn notes_sync_clone_from_non_repo_directory_fetches_authorship_notes() {
        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("non-repo-clone-seed.txt"), "seed\n")
            .expect("failed to write seed file");
        local
            .git_og(&["add", "non-repo-clone-seed.txt"])
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
                "non-repo-clone-seed-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("pushing branch should succeed");
        local
            .git_og(&["push", "origin", "refs/notes/ai"])
            .expect("pushing notes should succeed");

        // Clone from a non-repo directory (not inside any git repository).
        // This is the common production scenario for first-time clones.
        let external_cwd = unique_temp_path("notes-sync-clone-non-repo-cwd");
        let _ = fs::remove_dir_all(&external_cwd);
        fs::create_dir_all(&external_cwd).expect("failed to create non-repo cwd");

        let clone_target = "cloned-repo";
        let upstream_str = upstream.path().to_string_lossy().to_string();

        local
            .git_from_working_dir(
                &external_cwd,
                &["clone", upstream_str.as_str(), clone_target],
            )
            .expect("clone from non-repo cwd should succeed");

        let clone_dir = external_cwd.join(clone_target);
        assert!(
            clone_dir.exists(),
            "clone target should exist at {}",
            clone_dir.display()
        );

        let cloned_note = read_note_from_worktree(&clone_dir, &seed_sha);
        assert!(
            cloned_note.is_some(),
            "cloned repository should have fetched authorship notes for commit {} (clone from non-repo directory)",
            seed_sha
        );
    }
}

// Regression test: clone with an absolute target path from a non-repo CWD.
// Exercises the side-effect target resolution path where the clone target is
// specified as an absolute path (common in scripted / CI workflows and when
// the user types `git clone URL /some/absolute/path`).
worktree_test_wrappers! {
    fn notes_sync_clone_absolute_target_from_non_repo_cwd_fetches_authorship_notes() {

        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("abs-clone-seed.txt"), "seed\n")
            .expect("failed to write seed file");
        local
            .git_og(&["add", "abs-clone-seed.txt"])
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
                "abs-clone-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("pushing branch should succeed");
        local
            .git_og(&["push", "origin", "refs/notes/ai"])
            .expect("pushing notes should succeed");

        // Clone from a non-repo directory using an absolute target path.
        let external_cwd = unique_temp_path("notes-sync-abs-target-cwd");
        let _ = fs::remove_dir_all(&external_cwd);
        fs::create_dir_all(&external_cwd).expect("failed to create external cwd");

        let clone_dir = unique_temp_path("notes-sync-abs-target-clone");
        let _ = fs::remove_dir_all(&clone_dir);
        let clone_dir_str = clone_dir.to_string_lossy().to_string();
        let upstream_str = upstream.path().to_string_lossy().to_string();

        local
            .git_from_working_dir(
                &external_cwd,
                &["clone", upstream_str.as_str(), clone_dir_str.as_str()],
            )
            .expect("clone with absolute target should succeed");

        assert!(
            clone_dir.exists(),
            "clone target should exist at {}",
            clone_dir.display()
        );

        let cloned_note = read_note_from_worktree(&clone_dir, &seed_sha);
        assert!(
            cloned_note.is_some(),
            "cloned repository should have fetched authorship notes for commit {} (absolute target from non-repo CWD)",
            seed_sha
        );
    }
}

// Regression test: clone with NO explicit target directory (implicit target
// derived from the source URL/path).  This is the common user scenario:
//   cd ~/projects && git clone https://github.com/user/repo
// In trace2, the root process emits def_repo with the correct clone destination,
// but child processes (remote-https, index-pack) emit def_repo with the CWD as
// worktree.  The normalizer must prefer the root def_repo and ignore children.
worktree_test_wrappers! {
    fn notes_sync_clone_implicit_target_from_non_repo_cwd_fetches_authorship_notes() {

        let (local, upstream) = TestRepo::new_with_remote();

        fs::write(local.path().join("implicit-seed.txt"), "seed\n")
            .expect("failed to write seed file");
        local
            .git_og(&["add", "implicit-seed.txt"])
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
                "implicit-clone-note",
                seed_sha.as_str(),
            ])
            .expect("adding notes should succeed");
        local
            .git_og(&["push", "-u", "origin", "HEAD"])
            .expect("pushing branch should succeed");
        local
            .git_og(&["push", "origin", "refs/notes/ai"])
            .expect("pushing notes should succeed");

        // Clone from a non-repo CWD with NO explicit target — git derives the
        // directory name from the source path (the upstream bare repo's basename).
        let external_cwd = unique_temp_path("notes-sync-clone-implicit-cwd");
        let _ = fs::remove_dir_all(&external_cwd);
        fs::create_dir_all(&external_cwd).expect("failed to create external cwd");

        let upstream_str = upstream.path().to_string_lossy().to_string();
        // Derive the expected directory name the same way git does: basename of the source.
        let expected_dir_name = Path::new(&upstream_str)
            .file_name()
            .expect("upstream path should have a filename")
            .to_string_lossy()
            .to_string();
        // Strip .git suffix if present (matches git's behavior)
        let expected_dir_name = expected_dir_name
            .strip_suffix(".git")
            .unwrap_or(&expected_dir_name);

        local
            .git_from_working_dir(&external_cwd, &["clone", upstream_str.as_str()])
            .expect("clone with implicit target should succeed");

        let clone_dir = external_cwd.join(expected_dir_name);
        assert!(
            clone_dir.exists(),
            "implicit clone target should exist at {}",
            clone_dir.display()
        );

        let cloned_note = read_note_from_worktree(&clone_dir, &seed_sha);
        assert!(
            cloned_note.is_some(),
            "cloned repository should have fetched authorship notes for commit {} (implicit target from non-repo CWD)",
            seed_sha
        );
    }
}
