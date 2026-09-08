use super::super::*;

#[test]
#[serial]
fn daemon_pure_trace_socket_pull_fast_forward_tracks_pull_command() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let default_branch = repo.current_branch();

    let run_git = |args: &[&str]| -> String {
        let output = Command::new(real_git_executable())
            .args(args)
            .output()
            .expect("git command should execute");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    fs::write(repo.path().join("pull-case.txt"), "base\n").expect("failed to write base");
    repo.git_og_with_env(&["add", "pull-case.txt"], &env_refs)
        .expect("add should succeed");
    repo.git_og_with_env(&["commit", "-m", "base"], &env_refs)
        .expect("base commit should succeed");

    let remote_root = tempfile::tempdir().expect("remote tempdir should be created");
    let bare_remote = remote_root.path().join("origin.git");
    let remote_clone = remote_root.path().join("origin-work");
    let bare_remote_str = bare_remote.to_string_lossy().to_string();
    let remote_clone_str = remote_clone.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&bare_remote);
    let _ = fs::remove_dir_all(&remote_clone);

    run_git(&["init", "--bare", bare_remote_str.as_str()]);
    repo.git_og_with_env(
        &["remote", "add", "origin", bare_remote_str.as_str()],
        &env_refs,
    )
    .expect("adding origin remote should succeed");
    repo.git_og_with_env(
        &["push", "-u", "origin", default_branch.as_str()],
        &env_refs,
    )
    .expect("pushing base branch should succeed");

    run_git(&[
        "clone",
        "--branch",
        default_branch.as_str(),
        bare_remote_str.as_str(),
        remote_clone_str.as_str(),
    ]);
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
    fs::write(remote_clone.join("pull-case.txt"), "base\nremote update\n")
        .expect("failed to write remote update");
    run_git(&["-C", remote_clone_str.as_str(), "add", "pull-case.txt"]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "commit",
        "-m",
        "remote update",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "push",
        "origin",
        format!("HEAD:{}", default_branch).as_str(),
    ]);

    repo.git_og_with_env(
        &["pull", "--ff-only", "origin", default_branch.as_str()],
        &env_refs,
    )
    .expect("fast-forward pull should succeed");

    wait_for_expected_top_level_completions(&repo, 0, 5);

    let pull_entries = completion_entries_for_command(&repo, "pull");
    let saw_pull_success = pull_entries.iter().any(|entry| entry.exit_code == Some(0));
    assert!(saw_pull_success, "pull success should be tracked");
    assert!(
        fs::read_to_string(repo.path().join("pull-case.txt"))
            .expect("pulled file should be readable")
            .contains("remote update"),
        "pull fast-forward should update the worktree contents"
    );
}

#[test]
#[serial]
fn daemon_pure_trace_socket_pull_rebase_tracks_pull_and_rebase_completion() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let default_branch = repo.current_branch();

    let run_git = |args: &[&str]| -> String {
        let output = Command::new(real_git_executable())
            .args(args)
            .output()
            .expect("git command should execute");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    fs::write(repo.path().join("pull-rebase-base.txt"), "base\n").expect("failed to write base");
    repo.git_og_with_env(&["add", "pull-rebase-base.txt"], &env_refs)
        .expect("add should succeed");
    repo.git_og_with_env(&["commit", "-m", "base"], &env_refs)
        .expect("base commit should succeed");

    let root = repo
        .path()
        .parent()
        .expect("test repo path should have parent")
        .to_path_buf();
    let unique = repo
        .path()
        .file_name()
        .expect("test repo path should have filename")
        .to_string_lossy();
    let bare_remote = root.join(format!("origin-rebase-{unique}.git"));
    let remote_clone = root.join(format!("origin-rebase-work-{unique}"));
    let bare_remote_str = bare_remote.to_string_lossy().to_string();
    let remote_clone_str = remote_clone.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&bare_remote);
    let _ = fs::remove_dir_all(&remote_clone);

    run_git(&["init", "--bare", bare_remote_str.as_str()]);
    repo.git_og_with_env(
        &["remote", "add", "origin", bare_remote_str.as_str()],
        &env_refs,
    )
    .expect("adding origin remote should succeed");
    repo.git_og_with_env(
        &["push", "-u", "origin", default_branch.as_str()],
        &env_refs,
    )
    .expect("pushing base branch should succeed");

    run_git(&[
        "clone",
        "--branch",
        default_branch.as_str(),
        bare_remote_str.as_str(),
        remote_clone_str.as_str(),
    ]);
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
        .expect("failed to write remote file");
    run_git(&["-C", remote_clone_str.as_str(), "add", "remote-only.txt"]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "commit",
        "-m",
        "remote commit",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "push",
        "origin",
        format!("HEAD:{}", default_branch).as_str(),
    ]);

    fs::write(repo.path().join("local-only.txt"), "local\n").expect("failed to write local file");
    repo.git_og_with_env(&["add", "local-only.txt"], &env_refs)
        .expect("local add should succeed");
    repo.git_og_with_env(&["commit", "-m", "local commit"], &env_refs)
        .expect("local commit should succeed");

    repo.git_og_with_env(
        &["pull", "--rebase", "origin", default_branch.as_str()],
        &env_refs,
    )
    .expect("pull --rebase should succeed");

    wait_for_expected_top_level_completions(&repo, 0, 7);

    let pull_entries = completion_entries_for_command(&repo, "pull");
    let saw_pull_rebase_success = pull_entries.iter().any(|entry| entry.exit_code == Some(0));
    assert!(
        saw_pull_rebase_success,
        "pull --rebase success should be tracked"
    );
}

#[test]
#[serial]
fn daemon_pure_trace_socket_pull_autostash_preserves_local_changes_and_tracks_command() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let default_branch = repo.current_branch();

    let run_git = |args: &[&str]| -> String {
        let output = Command::new(real_git_executable())
            .args(args)
            .output()
            .expect("git command should execute");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    fs::write(repo.path().join("autostash-local.txt"), "base\n").expect("failed to write base");
    repo.git_og_with_env(&["add", "autostash-local.txt"], &env_refs)
        .expect("add should succeed");
    repo.git_og_with_env(&["commit", "-m", "base"], &env_refs)
        .expect("base commit should succeed");

    let root = repo
        .path()
        .parent()
        .expect("test repo path should have parent")
        .to_path_buf();
    let bare_remote = root.join("origin-autostash.git");
    let remote_clone = root.join("origin-autostash-work");
    let bare_remote_str = bare_remote.to_string_lossy().to_string();
    let remote_clone_str = remote_clone.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&bare_remote);
    let _ = fs::remove_dir_all(&remote_clone);

    run_git(&["init", "--bare", bare_remote_str.as_str()]);
    repo.git_og_with_env(
        &["remote", "add", "origin", bare_remote_str.as_str()],
        &env_refs,
    )
    .expect("adding origin remote should succeed");
    repo.git_og_with_env(
        &["push", "-u", "origin", default_branch.as_str()],
        &env_refs,
    )
    .expect("pushing base branch should succeed");

    run_git(&[
        "clone",
        "--branch",
        default_branch.as_str(),
        bare_remote_str.as_str(),
        remote_clone_str.as_str(),
    ]);
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
    fs::write(remote_clone.join("autostash-remote.txt"), "remote\n")
        .expect("failed to write remote update file");
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "add",
        "autostash-remote.txt",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "commit",
        "-m",
        "remote update",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "push",
        "origin",
        format!("HEAD:{}", default_branch).as_str(),
    ]);

    fs::write(
        repo.path().join("autostash-local.txt"),
        "base\nlocal dirty change\n",
    )
    .expect("failed to write local dirty change");

    repo.git_og_with_env(
        &[
            "pull",
            "--rebase",
            "--autostash",
            "origin",
            default_branch.as_str(),
        ],
        &env_refs,
    )
    .expect("pull --rebase --autostash should succeed");

    wait_for_expected_top_level_completions(&repo, 0, 5);

    let local_contents = fs::read_to_string(repo.path().join("autostash-local.txt"))
        .expect("local file should remain readable");
    assert!(
        local_contents.contains("local dirty change"),
        "autostash pull should preserve local dirty change content"
    );

    let pull_entries = completion_entries_for_command(&repo, "pull");
    let saw_pull_autostash_success = pull_entries.iter().any(|entry| entry.exit_code == Some(0));
    assert!(
        saw_pull_autostash_success,
        "pull --rebase --autostash success should be tracked"
    );
}
