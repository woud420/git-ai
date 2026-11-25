use super::*;
use crate::operations::git::test_utils::TmpRepo;
use std::fs;
use std::process::Command;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["-c", "protocol.file.allow=always"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Every layout gives the answer Git gives; the common ones without asking it.
#[test]
fn bareness_matches_git_and_skips_it_for_plain_work_trees() {
    let tmp = TmpRepo::new().unwrap();
    tmp.commit_all("initial").unwrap();
    let root = tmp.path();
    let dir = root.parent().unwrap().join(format!(
        "{}-layouts",
        root.file_name().unwrap().to_string_lossy()
    ));
    fs::create_dir_all(root.join("sub")).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();

    git(root, &["worktree", "add", "-q", &path("linked")]);
    git(
        root,
        &["submodule", "add", "-q", &root.to_string_lossy(), "module"],
    );
    git(
        root,
        &[
            "init",
            "-q",
            "--separate-git-dir",
            &path("sep.git"),
            &path("sep"),
        ],
    );
    git(root, &["clone", "-q", "--bare", ".", &path("bare.git")]);
    git(
        &dir.join("bare.git"),
        &["worktree", "add", "-q", &path("bare-linked")],
    );
    git(root, &["init", "-q", &path("core-bare")]);
    git(&dir.join("core-bare"), &["config", "core.bare", "true"]);
    git(root, &["init", "-q", &path("include")]);
    git(&dir.join("include"), &["config", "include.path", "extra"]);
    git(root, &["init", "-q", &path("wt-config")]);
    git(
        &dir.join("wt-config"),
        &["config", "extensions.worktreeConfig", "true"],
    );
    git(
        &dir.join("wt-config"),
        &["config", "--worktree", "core.bare", "true"],
    );

    for (path, answered_without_git) in [
        (root.to_path_buf(), true),
        (root.join("sub"), true),
        (root.join("module"), true),
        (dir.join("linked"), true),
        (dir.join("sep"), true),
        (dir.join("bare-linked"), false),
        (dir.join("core-bare"), false),
        (dir.join("include"), false),
        (dir.join("wt-config"), false),
    ] {
        let repo = discover_repository_in_path_no_git_exec(&path).unwrap();
        let expected = git(&path, &["rev-parse", "--is-bare-repository"]) == "true";
        assert_eq!(
            repo.discovered_is_bare.is_some(),
            answered_without_git,
            "{}",
            path.display()
        );
        assert_eq!(
            repo.is_bare_repository().unwrap(),
            expected,
            "{}",
            path.display()
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}
