use super::{
    ExpectedLineExt, GitAiRepository, PathBuf, fs, normalize_blame_for_format_parity, raw_git,
    stats_from_args, unique_worktree_path,
};

crate::worktree_test_wrappers! {
    fn repository_paths_and_storage_are_worktree_aware() {
        let repo = TestRepo::new();

        let common_dir = PathBuf::from(
            repo.git(&["rev-parse", "--git-common-dir"])
                .expect("resolve common dir")
                .trim(),
        );
        let git_dir = PathBuf::from(
            repo.git(&["rev-parse", "--git-dir"])
                .expect("resolve git dir")
                .trim(),
        );

        assert!(
            repo.path().join(".git").is_file(),
            "linked worktree should have a .git file"
        );

        let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find git-ai repository");
        assert_eq!(
            gitai_repo.workdir().unwrap().canonicalize().unwrap(),
            repo.path().canonicalize().unwrap(),
            "workdir should match linked worktree root"
        );
        assert_eq!(
            gitai_repo.path().canonicalize().unwrap(),
            git_dir.canonicalize().unwrap(),
            "git dir should match rev-parse --git-dir for linked worktree"
        );

        let expected_prefix = common_dir.join("ai").join("worktrees");
        assert!(
            gitai_repo.storage.working_logs.starts_with(&expected_prefix),
            "working logs should live under common-dir isolated storage: {}",
            gitai_repo.storage.working_logs.display()
        );
    }
}

crate::worktree_test_wrappers! {
    fn checkpoint_and_blame_support_absolute_paths_in_worktree() {
        let repo = TestRepo::new();
        let mut file = repo.filename("src/lib.rs");
        file.set_contents(crate::lines!["fn a() {}".human(), "fn ai() {}".ai()]);
        repo.stage_all_and_commit("add file with ai lines").unwrap();

        let abs_path = repo.path().join("src/lib.rs");
        let output = repo
            .git_ai(&["blame", abs_path.to_str().unwrap()])
            .expect("blame should work for absolute path in worktree");
        assert!(output.contains("fn ai() {}"));
    }
}

crate::worktree_test_wrappers! {
    fn blame_boundary_and_abbrev_match_git_in_worktree() {
        let repo = TestRepo::new();
        let mut file = repo.filename("boundary.txt");
        file.set_contents(crate::lines!["root line".human(), "line to change".human()]);
        repo.stage_all_and_commit("root commit").unwrap();

        file.set_contents(crate::lines!["root line".human(), "updated line".human()]);
        repo.stage_all_and_commit("second commit").unwrap();

        let git_output = repo
            .git(&["blame", "--abbrev=12", "-b", "boundary.txt"])
            .expect("git blame with boundary flags should succeed");
        let git_ai_output = repo
            .git_ai(&["blame", "--abbrev", "12", "-b", "boundary.txt"])
            .expect("git-ai blame with boundary flags should succeed");

        assert_eq!(
            normalize_blame_for_format_parity(&git_ai_output),
            normalize_blame_for_format_parity(&git_output),
            "git-ai blame should match git formatting for boundary and abbrev in worktrees"
        );

        let git_root_output = repo
            .git(&["blame", "--abbrev=12", "--root", "boundary.txt"])
            .expect("git blame --root should succeed");
        let git_ai_root_output = repo
            .git_ai(&["blame", "--abbrev", "12", "--root", "boundary.txt"])
            .expect("git-ai blame --root should succeed");

        assert_eq!(
            normalize_blame_for_format_parity(&git_ai_root_output),
            normalize_blame_for_format_parity(&git_root_output),
            "git-ai blame should match git formatting for --root and abbrev in worktrees"
        );
    }
}

crate::worktree_test_wrappers! {
    fn diff_works_in_worktree_context() {
        let repo = TestRepo::new();
        let mut file = repo.filename("diff.txt");
        file.set_contents(crate::lines!["old".human()]);
        repo.stage_all_and_commit("initial").unwrap();

        file.set_contents(crate::lines!["new".ai()]);
        let commit = repo.stage_all_and_commit("ai update").unwrap();

        let output = repo
            .git_ai(&["diff", &commit.commit_sha])
            .expect("git-ai diff should succeed in worktree");

        assert!(output.contains("diff.txt"));
        assert!(output.contains("+new"));
    }
}

crate::worktree_test_wrappers! {
    fn stash_pop_preserves_ai_authorship() {
        let repo = TestRepo::new();
        repo.human_edit("stash.txt", "base\n");
        let mut file = repo.filename("stash.txt");
        repo.stage_all_and_commit("base").unwrap();

        file.set_contents(crate::lines!["base".human(), "ai stash line".ai()]);
        repo.git(&["stash", "push", "-u", "-m", "wip"]).unwrap();
        repo.git(&["stash", "pop"]).unwrap();
        repo.stage_all_and_commit("apply stash").unwrap();

        file.assert_lines_and_blame(crate::lines!["base".human(), "ai stash line".ai()]);
    }
}

crate::worktree_test_wrappers! {
    fn reset_mixed_reconstructs_working_log() {
        let repo = TestRepo::new();
        repo.human_edit("reset.txt", "base\n");
        let mut file = repo.filename("reset.txt");
        repo.stage_all_and_commit("base").unwrap();

        file.set_contents(crate::lines!["base".human(), "ai reset line".ai()]);
        repo.stage_all_and_commit("ai commit").unwrap();

        repo.git(&["reset", "--mixed", "HEAD~1"])
            .expect("mixed reset should succeed");
        repo.stage_all_and_commit("recommit after reset").unwrap();

        file.assert_lines_and_blame(crate::lines!["base".human(), "ai reset line".ai()]);
    }
}

crate::worktree_test_wrappers! {
    fn rebase_preserves_ai_authorship() {
        let repo = TestRepo::new();
        repo.human_edit("rebase.txt", "base\n");
        let mut file = repo.filename("rebase.txt");
        repo.stage_all_and_commit("base").unwrap();
        repo.git(&["checkout", "-b", "integration"]).unwrap();

        repo.git(&["checkout", "-b", "feature", "integration"]).unwrap();
        file.set_contents(crate::lines!["base".human(), "feature ai line".ai()]);
        repo.stage_all_and_commit("feature ai").unwrap();

        repo.git(&["checkout", "integration"]).unwrap();
        let mut main_only = repo.filename("main-only.txt");
        main_only.set_contents(crate::lines!["main human".human()]);
        repo.stage_all_and_commit("main human commit").unwrap();

        repo.git(&["checkout", "feature"]).unwrap();
        repo.git(&["rebase", "integration"]).unwrap();

        file.assert_lines_and_blame(crate::lines!["base".human(), "feature ai line".ai()]);
    }
}

crate::worktree_test_wrappers! {
    fn cherry_pick_preserves_ai_authorship() {
        let repo = TestRepo::new();
        repo.human_edit("cherry.txt", "base\n");
        let mut file = repo.filename("cherry.txt");
        repo.stage_all_and_commit("base").unwrap();
        repo.git(&["checkout", "-b", "integration"]).unwrap();

        repo.git(&["checkout", "-b", "feature", "integration"]).unwrap();
        file.set_contents(crate::lines!["base".human(), "feature ai".ai()]);
        let ai_commit = repo.stage_all_and_commit("feature ai").unwrap();

        repo.git(&["checkout", "integration"]).unwrap();
        repo.git(&["cherry-pick", &ai_commit.commit_sha]).unwrap();

        file.assert_lines_and_blame(crate::lines!["base".human(), "feature ai".ai()]);
    }
}

crate::worktree_test_wrappers! {
    fn multi_worktree_storage_isolation_prevents_cross_talk() {
        let repo = TestRepo::new();
        let common_dir = PathBuf::from(
            repo.git(&["rev-parse", "--git-common-dir"])
                .expect("resolve common dir")
                .trim(),
        );
        let main_repo_dir = common_dir.parent().expect("main repo dir");
        let second_worktree = unique_worktree_path();

        raw_git(
            main_repo_dir,
            &["worktree", "add", second_worktree.to_str().unwrap()],
        );

        let repo_one = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
            .expect("find first worktree repo");
        let repo_two =
            GitAiRepository::find_repository_in_path(second_worktree.to_str().unwrap())
                .expect("find second worktree repo");

        let expected_prefix = common_dir.join("ai").join("worktrees");
        assert!(repo_one.storage.working_logs.starts_with(&expected_prefix));
        assert!(repo_two.storage.working_logs.starts_with(&expected_prefix));
        assert_ne!(
            repo_one.storage.working_logs,
            repo_two.storage.working_logs,
            "distinct linked worktrees must not share the same working_logs path"
        );

        let wl_one = repo_one.storage.working_log_for_base_commit("initial").unwrap();
        let wl_two = repo_two.storage.working_log_for_base_commit("initial").unwrap();
        fs::write(wl_one.dir.join("sentinel"), "one").expect("write sentinel one");
        assert!(
            !wl_two.dir.join("sentinel").exists(),
            "worktree-local storage should remain isolated"
        );
    }
}

crate::worktree_test_wrappers! {
    fn stats_head_arg_uses_worktree_head() {
        // Regression test for issue #285: `git-ai stats head` in a worktree was
        // resolving HEAD to the *main* repository's HEAD instead of the worktree's
        // own HEAD, so it reported 100% human even for AI-heavy commits.
        let repo = TestRepo::new();

        // Make an AI-only commit in the worktree. At this point the worktree's
        // HEAD has diverged from the main repo's initial (empty) commit.
        let mut file = repo.filename("ai_work.txt");
        file.set_contents(crate::lines!["line_a".ai(), "line_b".ai()]);
        let commit = repo.stage_all_and_commit("ai commit in worktree").unwrap();

        // Sanity: the explicit-SHA path works correctly.
        let stats_by_sha = stats_from_args(
            &repo,
            &["stats", &commit.commit_sha, "--json"],
        );
        assert!(
            stats_by_sha.git_diff_added_lines > 0,
            "explicit SHA stats should see the 2 added lines"
        );

        // `stats HEAD` (uppercase) must resolve to the worktree's HEAD, not the
        // main repo's initial empty commit.
        let stats_head_upper = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
        assert_eq!(
            stats_head_upper.git_diff_added_lines,
            stats_by_sha.git_diff_added_lines,
            "`stats HEAD` (uppercase) must match `stats <sha>`: got {} vs {}",
            stats_head_upper.git_diff_added_lines,
            stats_by_sha.git_diff_added_lines,
        );

        // `stats head` (lowercase) must behave identically to `stats HEAD` on
        // all platforms.  Before this fix, on case-insensitive filesystems
        // (macOS) 'head' could resolve to the *main* repo's HEAD instead of the
        // worktree's own HEAD; on case-sensitive Linux it was rejected outright.
        // The fix normalises 'head' → 'HEAD' before the git call.
        let stats_head_lower = stats_from_args(&repo, &["stats", "head", "--json"]);
        assert_eq!(
            stats_head_lower.git_diff_added_lines,
            stats_by_sha.git_diff_added_lines,
            "`stats head` (lowercase) must match `stats <sha>`: got {} vs {}",
            stats_head_lower.git_diff_added_lines,
            stats_by_sha.git_diff_added_lines,
        );
    }
}
