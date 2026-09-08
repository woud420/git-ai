use crate::debug_context::{context_output, jj_layout, snapshot};
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::operations::workspace_context::{ContextError, WorkspaceContext, discover};
use std::fs;
use std::path::Path;

fn repo() -> TestRepo {
    TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon)
}

fn same_as_cli(repo: &TestRepo, cwd: &Path) -> WorkspaceContext {
    let discovered = discover(cwd).unwrap();
    let output = context_output(repo, cwd);
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        format!("{}\n", serde_json::to_string_pretty(&discovered).unwrap()).into_bytes()
    );
    discovered
}

fn same_error_as_cli(repo: &TestRepo, cwd: &Path) -> ContextError {
    let error = discover(cwd).unwrap_err();
    let output = context_output(repo, cwd);
    assert!(!output.status.success());
    let expected = serde_json::json!({ "error": &error });
    assert_eq!(output.stdout, format!("{expected}\n").into_bytes());
    error
}

#[test]
fn workspace_context_public_git_paths_match_diagnostic_without_storage_changes() {
    let repo = repo();
    let nested = repo.path().join("directory with spaces/src");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("dirty.txt"), b"uncommitted\n").unwrap();
    let before = snapshot(repo.path());
    let root = same_as_cli(&repo, repo.path());
    let from_nested = same_as_cli(&repo, &nested);
    assert_eq!(root.schema_version, 1);
    assert_eq!(root.capability, "discovery_only");
    assert_eq!(root.vcs, "git");
    assert_eq!(root.workspace_root, repo.path().canonicalize().unwrap());
    assert_eq!(
        root.git.git_dir,
        repo.path().join(".git").canonicalize().unwrap()
    );
    assert_eq!(root.git.common_dir, root.git.git_dir);
    assert!(root.jj.is_none());
    assert!(!root.colocated);
    assert_eq!(
        serde_json::to_value(from_nested).unwrap(),
        serde_json::to_value(root).unwrap()
    );
    assert_eq!(snapshot(repo.path()), before);
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
fn workspace_context_public_git_worktree_preserves_distinct_common_directory() {
    let repo = TestRepo::new_worktree_with_daemon_scope(DaemonTestScope::NoDaemon);
    let before = snapshot(repo.path());
    let discovered = same_as_cli(&repo, repo.path());
    let git_dir = git_ai::operations::git::repo_state::git_dir_for_worktree(repo.path()).unwrap();
    let common = git_ai::operations::git::repo_state::common_dir_for_git_dir(&git_dir).unwrap();
    assert_eq!(discovered.git.git_dir, git_dir.canonicalize().unwrap());
    assert_eq!(discovered.git.common_dir, common.canonicalize().unwrap());
    assert_ne!(discovered.git.git_dir, discovered.git.common_dir);
    assert_eq!(
        discovered.workspace_root,
        repo.path().canonicalize().unwrap()
    );
    assert_eq!(snapshot(repo.path()), before);
}

#[test]
fn workspace_context_jj_locators_do_not_require_or_certify_native_operation_state() {
    let repo = repo();
    jj_layout(repo.path());
    fs::write(repo.path().join("dirty.txt"), b"not snapshotted\n").unwrap();
    let before = snapshot(repo.path());
    let discovered = same_as_cli(&repo, repo.path());
    assert_eq!(discovered.capability, "discovery_only");
    assert_eq!(discovered.vcs, "jj");
    assert!(discovered.colocated);
    let jj = discovered.jj.unwrap();
    assert_eq!(
        jj.repo_dir,
        repo.path().join(".jj/repo").canonicalize().unwrap()
    );
    assert_eq!(
        jj.store_dir,
        repo.path().join(".jj/repo/store").canonicalize().unwrap()
    );
    assert_eq!(
        discovered.git.git_dir,
        repo.path().join(".git").canonicalize().unwrap()
    );
    assert!(!jj.repo_dir.join("op_store").exists());
    assert!(!jj.repo_dir.join("op_heads").exists());
    assert!(!repo.path().join(".jj/working_copy/checkout").exists());
    assert_eq!(snapshot(repo.path()), before);
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
fn workspace_context_linked_jj_locator_reuses_source_paths_with_its_own_workspace() {
    let repo = repo();
    jj_layout(repo.path());
    let linked = repo.path().join("linked workspace");
    let nested = linked.join("src/nested");
    fs::create_dir_all(linked.join(".jj/working_copy")).unwrap();
    fs::create_dir_all(&nested).unwrap();
    fs::write(linked.join(".jj/working_copy/type"), b"local").unwrap();
    fs::write(linked.join(".jj/repo"), b"../../.jj/repo").unwrap();
    fs::write(nested.join("dirty.txt"), b"not snapshotted\n").unwrap();
    let before = snapshot(repo.path());
    let primary = same_as_cli(&repo, repo.path());
    let secondary = same_as_cli(&repo, &nested);
    assert_eq!(secondary.workspace_root, linked.canonicalize().unwrap());
    assert_ne!(primary.workspace_root, secondary.workspace_root);
    assert!(primary.colocated);
    assert!(!secondary.colocated);
    assert_eq!(secondary.capability, "discovery_only");
    assert_eq!(secondary.vcs, "jj");
    assert_eq!(primary.git.git_dir, secondary.git.git_dir);
    assert_eq!(primary.git.common_dir, secondary.git.common_dir);
    assert_eq!(
        primary.jj.as_ref().unwrap().repo_dir,
        secondary.jj.as_ref().unwrap().repo_dir
    );
    assert_eq!(
        primary.jj.as_ref().unwrap().store_dir,
        secondary.jj.as_ref().unwrap().store_dir
    );
    assert_eq!(snapshot(repo.path()), before);
    assert!(!repo.path().join(".git/ai").exists());
}

#[test]
fn workspace_context_public_errors_preserve_diagnostic_json_and_nested_boundaries() {
    for case in ["malformed", "backend", "oversize"] {
        let repo = repo();
        let cwd = if case == "malformed" {
            let nested = repo.path().join("nested");
            fs::create_dir_all(nested.join(".jj")).unwrap();
            nested
        } else {
            jj_layout(repo.path());
            if case == "backend" {
                fs::write(repo.path().join(".jj/repo/store/type"), b"future-backend").unwrap();
            } else {
                fs::write(
                    repo.path().join(".jj/repo/store/git_target"),
                    vec![b'x'; 16_385],
                )
                .unwrap();
            }
            repo.path().to_owned()
        };
        let before = snapshot(repo.path());
        let error = same_error_as_cli(&repo, &cwd);
        assert_eq!(
            error.code,
            if case == "backend" {
                "unsupported_backend"
            } else {
                "invalid_metadata"
            }
        );
        assert!(error.message.contains(".jj"));
        if case == "oversize" {
            assert!(error.message.contains("exceeds 16384 bytes"));
        }
        assert_eq!(snapshot(repo.path()), before);
        assert!(!repo.path().join(".git/ai").exists());
    }
}
