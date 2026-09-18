use super::*;
use crate::clients::git_cli::{exec_git, exec_git_stdin};

fn committed_repo() -> (tempfile::TempDir, Repository, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let args = vec!["-C".to_string(), dir.path().to_string_lossy().into_owned()];
    let run = |tail: &[&str]| {
        let mut command = args.clone();
        command.extend(tail.iter().map(|s| (*s).to_string()));
        String::from_utf8(exec_git(&command).unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    run(&["init"]);
    let tree = run(&["write-tree"]);
    let mut commit_args = args;
    commit_args.extend(
        [
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit-tree",
            &tree,
        ]
        .map(str::to_string),
    );
    let commit = String::from_utf8(
        exec_git_stdin(&commit_args, b"batch fixture\n")
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let repo =
        crate::operations::git::find_repository_in_path(dir.path().to_str().unwrap()).unwrap();
    (dir, repo, commit, tree)
}

#[test]
fn tree_batch_exceeds_platform_argv_limits() {
    let (_dir, repo, commit, tree) = committed_repo();
    // More than macOS ARG_MAX and the Windows command-line limit. Repeated
    // inputs isolate transport size without manufacturing thousands of objects.
    let trees = diff_tree::resolve_tree_shas(&repo, &vec![commit.clone(); 65536]).unwrap();
    assert_eq!(trees, HashMap::from([(commit, tree)]));
}

#[test]
fn parent_batch_exceeds_platform_argv_limits() {
    let (_dir, repo, commit, _) = committed_repo();
    let parents = range_diff::get_commit_parents_batch(&repo, &vec![commit.clone(); 65536]);
    assert_eq!(parents, HashMap::from([(commit, Vec::<String>::new())]));
}
