use super::{TestRepo, fs};

/// Spawn-scaling guard: a multi-commit `git revert` must trigger a CONSTANT
/// number of daemon git spawns regardless of how many commits are reverted.
/// Reverting N commits in one command and 3*N commits in another must produce
/// nearly the same revert-side-effect spawn count (the batched path issues a
/// fixed set of rev-parse/diff-tree/cat-file/notes spawns, not per-commit ones).
/// `#[ignore]` because it shells a dedicated daemon and inspects a spawn log;
/// run explicitly with `--ignored`.
#[test]
#[ignore]
fn revert_spawn_count_is_constant_in_commit_count() {
    fn revert_n(n: usize) -> usize {
        let log_dir =
            std::env::temp_dir().join(format!("git-ai-spawnlog-{}-{}", std::process::id(), n));
        let _ = fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("spawns.log");
        let _ = fs::remove_file(&log_path);
        let repo =
            TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log_path.to_str().unwrap())]);

        // Base with n files.
        for i in 0..n {
            fs::write(repo.path().join(format!("f{i}.txt")), format!("base {i}\n")).unwrap();
        }
        repo.stage_all_and_commit("base").unwrap();

        // Add an AI line to each, one commit.
        for i in 0..n {
            fs::write(
                repo.path().join(format!("f{i}.txt")),
                format!("base {i}\nAI {i}\n"),
            )
            .unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &format!("f{i}.txt")])
                .unwrap();
        }
        repo.stage_all_and_commit("add ai").unwrap();

        // Delete each AI line in its own commit → n delete commits.
        let mut del_commits = Vec::new();
        for i in 0..n {
            fs::write(repo.path().join(format!("f{i}.txt")), format!("base {i}\n")).unwrap();
            repo.git_ai(&["checkpoint", "mock_known_human", &format!("f{i}.txt")])
                .unwrap();
            repo.stage_all_and_commit(&format!("del {i}")).unwrap();
            del_commits.push(repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string());
        }

        repo.sync_daemon();
        let before = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);

        // One revert command over all n delete commits.
        let mut args = vec!["revert".to_string(), "--no-edit".to_string()];
        args.extend(del_commits);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        repo.git(&arg_refs).unwrap();
        repo.sync_daemon();

        let after = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);
        let _ = fs::remove_dir_all(&log_dir);
        after - before
    }

    let small = revert_n(2);
    let large = revert_n(8);
    eprintln!("revert spawns: n=2 -> {small}, n=8 -> {large}");
    // If revert work were per-commit, large would be ~4x small. With batching the
    // counts should be close (allow a small constant slack for git's own
    // revert-time invocations, which are not git-ai daemon spawns anyway).
    assert!(
        large <= small + 4,
        "revert spawn count scales with commit count: n=2 -> {small}, n=8 -> {large}"
    );
}

/// Spawn-scaling guard for the rebase path: rebasing a feature branch of N AI
/// commits onto an advanced main must trigger a CONSTANT number of daemon git
/// spawns regardless of N (the note-shift and conflict-resolution work is
/// batched). `#[ignore]`; run with `--ignored`.
#[test]
#[ignore]
fn rebase_spawn_count_is_constant_in_commit_count() {
    fn rebase_n(n: usize) -> usize {
        let log_dir = std::env::temp_dir().join(format!(
            "git-ai-spawnlog-rebase-{}-{}",
            std::process::id(),
            n
        ));
        let _ = fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("spawns.log");
        let _ = fs::remove_file(&log_path);
        let repo =
            TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log_path.to_str().unwrap())]);

        fs::write(repo.path().join("base.txt"), "base\n").unwrap();
        repo.stage_all_and_commit("base").unwrap();
        let default_branch = repo.current_branch();

        // Feature branch with N AI commits, each adding a line to its own file.
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        for i in 0..n {
            fs::write(
                repo.path().join(format!("feat{i}.txt")),
                format!("AI feat {i}\n"),
            )
            .unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &format!("feat{i}.txt")])
                .unwrap();
            repo.stage_all_and_commit(&format!("feat {i}")).unwrap();
        }

        // Advance main so the rebase is a real non-fast-forward.
        repo.git(&["checkout", &default_branch]).unwrap();
        fs::write(repo.path().join("main.txt"), "main work\n").unwrap();
        repo.stage_all_and_commit("main advance").unwrap();

        repo.git(&["checkout", "feature"]).unwrap();
        repo.sync_daemon();
        let before = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);

        repo.git(&["rebase", &default_branch]).unwrap();
        repo.sync_daemon();

        let after = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);
        let _ = fs::remove_dir_all(&log_dir);
        after - before
    }

    let small = rebase_n(2);
    let large = rebase_n(8);
    eprintln!("rebase spawns: n=2 -> {small}, n=8 -> {large}");
    assert!(
        large <= small + 4,
        "rebase spawn count scales with commit count: n=2 -> {small}, n=8 -> {large}"
    );
}
