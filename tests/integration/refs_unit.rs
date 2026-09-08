use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::clients::git_cli::{exec_git, exec_git_stdin};
use git_ai::error::GitAiError;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::operations::git::notes_api;
use git_ai::operations::git::notes_api::{read_authorship_v3, read_note, write_note};
use git_ai::operations::git::refs::git_backend_for_tests::{
    commits_with_authorship_notes, get_commits_with_notes_from_list, grep_ai_notes,
    note_blob_oids_for_commits, notes_add_batch, notes_add_blob_batch,
};
use git_ai::operations::git::refs::{
    AI_AUTHORSHIP_FORK_TRACKING_REF, CommitAuthorship, copy_missing_notes_for_commits_from_ref,
    copy_ref, get_reference_as_working_log, merge_notes_from_ref,
    note_blob_oids_for_commits_from_ref, ref_exists,
};
use git_ai::operations::git::repository::find_repository_in_path;
use std::fs;

// ---------------------------------------------------------------------------
// Repo-based tests (TestRepo replaces TmpRepo)
// ---------------------------------------------------------------------------

/// Helper: create a TestRepo and obtain a gitai Repository handle.
fn repo_with_handle() -> (TestRepo, git_ai::operations::git::repository::Repository) {
    let repo = TestRepo::new();
    let gitai_repo =
        find_repository_in_path(repo.path().to_str().unwrap()).expect("find repository");
    (repo, gitai_repo)
}

/// Helper: get the HEAD commit SHA from a TestRepo.
fn head_sha(repo: &TestRepo) -> String {
    repo.git_og(&["rev-parse", "HEAD"])
        .expect("rev-parse HEAD")
        .trim()
        .to_string()
}

fn git_stdin_stdout(
    repo: &git_ai::operations::git::repository::Repository,
    args: &[&str],
    stdin: &[u8],
) -> String {
    let mut git_args = repo.global_args_for_exec();
    git_args.extend(args.iter().map(|arg| arg.to_string()));
    let output = exec_git_stdin(&git_args, stdin).expect("git stdin command");
    String::from_utf8(output.stdout)
        .expect("git stdout utf8")
        .trim()
        .to_string()
}

fn commit_unattributed_file(
    repo: &TestRepo,
    filename: &str,
    content: &str,
    message: &str,
) -> String {
    fs::write(repo.path().join(filename), content).unwrap();
    repo.git_og(&["add", filename]).expect("add test file");
    repo.git_og(&["commit", "-m", message])
        .expect("commit test file");

    let mut file = repo.filename(filename);
    file.assert_committed_lines(crate::lines![
        content.trim_end_matches('\n').unattributed_human()
    ]);
    head_sha(repo)
}

fn install_note_at_paths(
    repo: &git_ai::operations::git::repository::Repository,
    paths: &[String],
    content: &str,
) {
    let mut stream = format!(
        "blob\nmark :1\ndata {}\n{}\ncommit refs/notes/ai\ncommitter Test <test@test.com> 1000000000 +0000\ndata 0\ndeleteall\n",
        content.len(),
        content
    );
    for path in paths {
        stream.push_str(&format!("M 100644 :1 {path}\n"));
    }
    stream.push_str("\ndone\n");

    git_stdin_stdout(
        repo,
        &["fast-import", "--quiet", "--done"],
        stream.as_bytes(),
    );
}

fn note_tree_paths(repo: &TestRepo) -> Vec<String> {
    repo.git_og(&["ls-tree", "-r", "--name-only", "refs/notes/ai"])
        .expect("list authorship note paths")
        .lines()
        .map(str::to_string)
        .collect()
}

fn deepest_note_path(commit_sha: &str) -> String {
    commit_sha
        .as_bytes()
        .chunks(2)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect::<Vec<_>>()
        .join("/")
}

mod note_queries;
mod note_writes;
mod ref_copying;
