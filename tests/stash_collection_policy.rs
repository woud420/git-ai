#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn pending_edits() -> TestRepo {
    let repo = TestRepo::new_dedicated_daemon();
    for name in ["selected.txt", "retained.txt"] {
        fs::write(repo.path().join(name), "base\n").unwrap();
        repo.git_ai(&["checkpoint", "mock_known_human", name])
            .unwrap();
    }
    repo.stage_all_and_commit("Base files").unwrap();
    for name in ["selected.txt", "retained.txt"] {
        repo.filename(name)
            .assert_committed_lines(lines!["base".human()]);
        repo.git_ai(&["checkpoint", "human", name]).unwrap();
        fs::write(repo.path().join(name), "base\nAI change\n").unwrap();
        repo.git_ai(&["checkpoint", "mock_ai", name]).unwrap();
    }
    repo
}

fn attribution_state(repo: &TestRepo) -> BTreeMap<PathBuf, Vec<u8>> {
    fn read_tree(root: &Path, path: &Path, state: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                read_tree(root, &path, state);
            } else {
                state.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    repo.sync_daemon_force();
    let repository =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let root = &repository.storage.ai_dir;
    let mut state = BTreeMap::new();
    for name in ["working_logs", "stashes_v2"] {
        read_tree(root, &root.join(name), &mut state);
    }
    state
}

fn assert_push_opt_out(pathspec_file: bool) {
    let mut repo = pending_edits();
    let before = attribution_state(&repo);
    repo.patch_git_ai_config(|patch| patch.allowed_repositories = Some(Vec::new()));
    if pathspec_file {
        let paths = tempfile::NamedTempFile::new().unwrap();
        fs::write(paths.path(), "selected.txt\n").unwrap();
        let option = format!("--pathspec-from-file={}", paths.path().display());
        repo.git(&["stash", "push", &option]).unwrap();
    } else {
        repo.git(&["stash", "push", "--", "selected.txt"]).unwrap();
    }
    assert_eq!(
        fs::read_to_string(repo.path().join("selected.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("retained.txt")).unwrap(),
        "base\nAI change\n"
    );
    assert_eq!(attribution_state(&repo), before);
}

#[test]
fn stash_policy_push_preserves_stored_evidence_when_collection_is_disabled() {
    assert_push_opt_out(false);
}

#[test]
fn stash_policy_pathspec_file_preserves_stored_evidence_when_collection_is_disabled() {
    assert_push_opt_out(true);
}

fn assert_existing_stash_opt_out(operation: &str) {
    let mut repo = pending_edits();
    repo.git(&["stash", "push", "--", "selected.txt"]).unwrap();
    let before = attribution_state(&repo);
    assert!(before.keys().any(|path| path.starts_with("stashes_v2")));
    repo.patch_git_ai_config(|patch| patch.allowed_repositories = Some(Vec::new()));
    if operation == "branch" {
        repo.git(&["stash", "branch", "restored"]).unwrap();
    } else {
        repo.git(&["stash", operation]).unwrap();
    }
    let selected = if operation == "drop" {
        "base\n"
    } else {
        "base\nAI change\n"
    };
    assert_eq!(
        fs::read_to_string(repo.path().join("selected.txt")).unwrap(),
        selected
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("retained.txt")).unwrap(),
        "base\nAI change\n"
    );
    assert_eq!(attribution_state(&repo), before);
}

#[test]
fn stash_policy_pop_preserves_stored_evidence_when_collection_is_disabled() {
    assert_existing_stash_opt_out("pop");
}

#[test]
fn stash_policy_apply_preserves_stored_evidence_when_collection_is_disabled() {
    assert_existing_stash_opt_out("apply");
}

#[test]
fn stash_policy_branch_preserves_stored_evidence_when_collection_is_disabled() {
    assert_existing_stash_opt_out("branch");
}

#[test]
fn stash_policy_drop_preserves_stored_evidence_when_collection_is_disabled() {
    assert_existing_stash_opt_out("drop");
}

#[test]
fn stash_policy_enabled_collection_preserves_attribution_after_pop() {
    let repo = pending_edits();
    repo.git(&["stash", "push", "--", "selected.txt"]).unwrap();
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("Commit restored and retained edits")
        .unwrap();
    for name in ["selected.txt", "retained.txt"] {
        repo.filename(name)
            .assert_committed_lines(lines!["base".human(), "AI change".ai()]);
    }
}
