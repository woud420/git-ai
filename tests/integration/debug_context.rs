use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> TestRepo {
    TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon)
}

fn context_output(repo: &TestRepo, cwd: &Path) -> Output {
    repo.git_ai_command_without_pre_sync_for_test(&["debug", "context", "--json"], &[])
        .current_dir(cwd)
        .output()
        .unwrap()
}

fn context(repo: &TestRepo, cwd: &Path) -> Value {
    let output = context_output(repo, cwd);
    assert!(
        output.status.success(),
        "context failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("context must produce JSON only")
}

fn canonical(path: &Path) -> Value {
    json!(path.canonicalize().unwrap())
}

fn jj_layout(root: &Path) {
    fs::create_dir_all(root.join(".jj/working_copy")).unwrap();
    fs::write(root.join(".jj/working_copy/type"), "local").unwrap();
    fs::create_dir_all(root.join(".jj/repo/store")).unwrap();
    fs::write(root.join(".jj/repo/store/type"), "git").unwrap();
    fs::write(root.join(".jj/repo/store/git_target"), "../../../.git").unwrap();
}

pub(super) fn snapshot(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                result.insert(path.strip_prefix(root).unwrap().to_owned(), Vec::new());
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(path, path, &mut result);
    result
}

#[test]
fn debug_context_git_root_and_subdirectory_are_identical_without_daemon_or_storage() {
    let repo = repo();
    let nested = repo.path().join("directory with spaces/src");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("dirty.txt"), "uncommitted content\n").unwrap();
    let before = snapshot(repo.path());
    let result = context(&repo, repo.path());
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["capability"], "discovery_only");
    assert_eq!(result["vcs"], "git");
    assert_eq!(result["workspace_root"], canonical(repo.path()));
    assert_eq!(
        result["git"]["git_dir"],
        canonical(&repo.path().join(".git"))
    );
    assert_eq!(result["git"]["common_dir"], result["git"]["git_dir"]);
    assert_eq!(result["jj"], Value::Null);
    assert_eq!(result["colocated"], false);
    assert_eq!(context(&repo, &nested), result);
    assert_eq!(snapshot(repo.path()), before);
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
fn debug_context_git_linked_worktree_preserves_common_store_and_workspace() {
    let repo = TestRepo::new_worktree_with_daemon_scope(DaemonTestScope::NoDaemon);
    let result = context(&repo, repo.path());
    let git_dir = git_ai::operations::git::repo_state::git_dir_for_worktree(repo.path()).unwrap();
    let common = git_ai::operations::git::repo_state::common_dir_for_git_dir(&git_dir).unwrap();
    assert_eq!(result["workspace_root"], canonical(repo.path()));
    assert_eq!(result["git"]["git_dir"], canonical(&git_dir));
    assert_eq!(result["git"]["common_dir"], canonical(&common));
    assert_ne!(result["git"]["git_dir"], result["git"]["common_dir"]);
}

#[test]
fn debug_context_nested_git_takes_precedence_over_enclosing_jj_boundary() {
    let outer = repo();
    fs::create_dir(outer.path().join(".jj")).unwrap();
    let nested = outer.path().join("nested");
    let inner = TestRepo::new_at_path_with_daemon_scope(&nested, DaemonTestScope::NoDaemon);
    let result = context(&inner, inner.path());
    assert_eq!(result["vcs"], "git");
    assert_eq!(result["workspace_root"], canonical(inner.path()));
}

#[test]
fn debug_context_malformed_jj_never_falls_through_to_enclosing_git() {
    let repo = repo();
    let nested = repo.path().join("nested");
    fs::create_dir_all(nested.join(".jj")).unwrap();
    let output = context_output(&repo, &nested);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "invalid_metadata");
    assert!(error["error"]["message"].as_str().unwrap().contains(".jj"));
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
fn debug_context_unsupported_jj_backend_is_an_explicit_error() {
    let repo = repo();
    jj_layout(repo.path());
    fs::write(repo.path().join(".jj/repo/store/type"), "future-backend").unwrap();
    let output = context_output(&repo, repo.path());
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "unsupported_backend");
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
fn debug_context_rejects_unknown_working_copy_and_noncanonical_store_types() {
    for (path, value) in [
        (".jj/working_copy/type", "remote"),
        (".jj/repo/store/type", "git\n"),
    ] {
        let repo = repo();
        jj_layout(repo.path());
        fs::write(repo.path().join(path), value).unwrap();
        let output = context_output(&repo, repo.path());
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["code"], "unsupported_backend");
    }
}

#[test]
fn debug_context_rejects_oversized_jj_pointer_metadata() {
    let repo = repo();
    jj_layout(repo.path());
    fs::write(
        repo.path().join(".jj/repo/store/git_target"),
        vec![b'x'; 16_385],
    )
    .unwrap();
    let output = context_output(&repo, repo.path());
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "invalid_metadata");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exceeds")
    );
}

#[cfg(unix)]
#[test]
fn debug_context_symlinked_workspace_has_the_same_identity() {
    let repo = repo();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("workspace link");
    std::os::unix::fs::symlink(repo.path(), &link).unwrap();
    assert_eq!(context(&repo, &link), context(&repo, repo.path()));
}

#[cfg(unix)]
#[test]
fn debug_context_dangling_nested_repository_links_never_fall_through() {
    for marker in [".git", ".jj"] {
        let repo = repo();
        let nested = repo.path().join("nested");
        fs::create_dir(&nested).unwrap();
        std::os::unix::fs::symlink("missing-metadata", nested.join(marker)).unwrap();
        let output = context_output(&repo, &nested);
        assert!(
            !output.status.success(),
            "dangling {marker} resolved outer repository"
        );
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["code"], "invalid_metadata");
    }
}

#[test]
fn debug_context_rejects_empty_and_dangling_jj_repository_pointers() {
    for pointer in ["", "missing-repository"] {
        let repo = repo();
        fs::create_dir_all(repo.path().join(".jj/working_copy")).unwrap();
        fs::write(repo.path().join(".jj/working_copy/type"), "local").unwrap();
        fs::write(repo.path().join(".jj/repo"), pointer).unwrap();
        let output = context_output(&repo, repo.path());
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["code"], "invalid_metadata");
    }
}

#[test]
fn debug_context_outside_repository_reports_no_repository() {
    let repo = repo();
    let directory = tempfile::tempdir().unwrap();
    let output = context_output(&repo, directory.path());
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "no_repository");
}

#[cfg(unix)]
#[test]
fn debug_context_does_not_spawn_git_or_jj() {
    use std::os::unix::fs::PermissionsExt;
    let mut repo = repo();
    let executables = tempfile::tempdir().unwrap();
    let marker = executables.path().join("spawned");
    for name in ["git", "jj"] {
        let executable = executables.path().join(name);
        fs::write(
            &executable,
            "#!/bin/sh\nprintf spawned >> \"$GIT_AI_CONTEXT_SPAWN_MARKER\"\nexit 99\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    repo.patch_git_ai_config(|patch| {
        patch.git_path = Some(
            executables
                .path()
                .join("git")
                .to_string_lossy()
                .into_owned(),
        );
    });
    let output = repo
        .git_ai_command_without_pre_sync_for_test(&["debug", "context", "--json"], &[])
        .env("PATH", executables.path())
        .env("GIT_AI_CONTEXT_SPAWN_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "context invoked Git or jj");
}

pub(super) fn jj(repo: &TestRepo, cwd: &Path, args: &[&str]) {
    let binary = std::env::var_os("GIT_AI_TEST_JJ_BINARY")
        .expect("the explicit jj test lane requires GIT_AI_TEST_JJ_BINARY");
    assert!(
        Path::new(&binary).is_file(),
        "GIT_AI_TEST_JJ_BINARY must name an installed jj binary"
    );
    let output = Command::new(binary)
        .current_dir(cwd)
        .env("HOME", repo.test_home_path())
        .env("XDG_CONFIG_HOME", repo.test_home_path().join(".config"))
        .env("JJ_CONFIG", "")
        .env_remove("GIT_TRACE2_EVENT")
        .args([
            "--config",
            "user.name=Context Test",
            "--config",
            "user.email=context@example.invalid",
        ])
        .args(args)
        .output()
        .expect("failed to execute the configured jj binary");
    assert!(
        output.status.success(),
        "jj {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn debug_context_real_jj_colocated_is_read_only() {
    let repo = repo();
    jj(&repo, repo.path(), &["git", "init", "--colocate"]);
    fs::write(repo.path().join("dirty.txt"), "not snapshotted\n").unwrap();
    let before = snapshot(repo.path());
    let result = context(&repo, repo.path());
    assert_eq!(result["vcs"], "jj");
    assert_eq!(result["capability"], "discovery_only");
    assert_eq!(result["colocated"], true);
    assert_eq!(result["workspace_root"], canonical(repo.path()));
    assert_eq!(
        result["jj"]["repo_dir"],
        canonical(&repo.path().join(".jj/repo"))
    );
    assert_eq!(
        result["jj"]["store_dir"],
        canonical(&repo.path().join(".jj/repo/store"))
    );
    assert_eq!(
        result["git"]["git_dir"],
        canonical(&repo.path().join(".git"))
    );
    assert_eq!(snapshot(repo.path()), before);
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn debug_context_real_jj_nested_noncolocated_workspace_and_relative_repo_pointer() {
    let repo = repo();
    let primary = repo.path().join("jj project");
    jj(
        &repo,
        repo.path(),
        &["git", "init", "--no-colocate", primary.to_str().unwrap()],
    );
    let secondary = repo.path().join("second workspace");
    jj(
        &repo,
        &primary,
        &["workspace", "add", secondary.to_str().unwrap()],
    );
    let nested = secondary.join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    let before = snapshot(repo.path());
    let first = context(&repo, &primary);
    let second = context(&repo, &nested);
    assert_eq!(first["vcs"], "jj");
    assert_eq!(first["colocated"], false);
    assert_eq!(first["workspace_root"], canonical(&primary));
    assert_eq!(second["workspace_root"], canonical(&secondary));
    assert_eq!(first["jj"], second["jj"]);
    assert_eq!(first["git"], second["git"]);
    assert_eq!(
        first["git"]["git_dir"],
        canonical(&primary.join(".jj/repo/store/git"))
    );
    assert_eq!(context(&repo, &secondary), second);
    assert_eq!(snapshot(repo.path()), before);
    assert!(!primary.join(".jj/repo/store/git/ai").exists());
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn debug_context_real_jj_colocated_separate_git_directory_is_not_a_linked_worktree() {
    let repo = repo();
    let external = tempfile::tempdir().unwrap();
    let git_dir = external.path().join("worktrees/store");
    fs::create_dir_all(git_dir.parent().unwrap()).unwrap();
    repo.git(&["init", "--separate-git-dir", git_dir.to_str().unwrap()])
        .unwrap();
    jj(&repo, repo.path(), &["git", "init", "--colocate"]);
    assert!(repo.path().join(".git").is_file());
    let before = snapshot(repo.path());
    let store_before = snapshot(&git_dir);
    let result = context(&repo, repo.path());
    assert_eq!(result["vcs"], "jj");
    assert_eq!(result["colocated"], true);
    assert_eq!(result["workspace_root"], canonical(repo.path()));
    assert_eq!(result["git"]["git_dir"], canonical(&git_dir));
    assert_eq!(result["git"]["common_dir"], canonical(&git_dir));
    assert_eq!(snapshot(repo.path()), before);
    assert_eq!(snapshot(&git_dir), store_before);
    assert!(!git_dir.join("ai").exists());
}
