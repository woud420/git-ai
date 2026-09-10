use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj, snapshot};
use git_ai::operations::jj::operation::{MAX_OPERATION_BYTES, decode_operation};
use std::fs;
use std::path::{Path, PathBuf};

fn bounded_files(directory: &Path, maximum: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        assert!(files.len() < maximum, "fixture file count limit exceeded");
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        files.push(entry.path());
    }
    files.sort();
    files
}

fn operation_head(root: &Path) -> String {
    let heads = bounded_files(&root.join(".jj/repo/op_heads/heads"), 2);
    assert_eq!(heads.len(), 1);
    let id = heads[0].file_name().unwrap().to_str().unwrap();
    assert_eq!(id.len(), 128);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    id.to_owned()
}

fn qualify_view_store(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    fs::write(repo.path().join(".gitignore"), "fixture auxiliary/\n").unwrap();
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("view workspace");
        jj(
            &repo,
            repo.path(),
            &["git", "init", "--no-colocate", root.to_str().unwrap()],
        );
        root
    };
    fs::write(root.join("example.txt"), "fixture line\n").unwrap();
    jj(&repo, &root, &["commit", "-m", "view fixture base"]);
    assert_eq!(
        fs::read_to_string(root.join("example.txt")).unwrap(),
        "fixture line\n"
    );
    jj(
        &repo,
        &root,
        &["bookmark", "create", "fixture-base", "-r", "@-"],
    );
    jj(
        &repo,
        &root,
        &["tag", "set", "fixture-v1", "-r", "fixture-base"],
    );

    let remote = repo.path().join("fixture auxiliary/remote.git");
    fs::create_dir_all(remote.parent().unwrap()).unwrap();
    repo.git(&["init", "--bare", remote.to_str().unwrap()])
        .unwrap();
    jj(
        &repo,
        &root,
        &[
            "git",
            "remote",
            "add",
            "fixture-origin",
            remote.to_str().unwrap(),
        ],
    );
    jj(
        &repo,
        &root,
        &[
            "git",
            "push",
            "--remote",
            "fixture-origin",
            "--bookmark",
            "fixture-base",
            "--tag",
            "fixture-v1",
        ],
    );
    jj(&repo, &root, &["new", "fixture-base", "-m", "left branch"]);
    jj(&repo, &root, &["bookmark", "create", "fixture-left"]);
    jj(&repo, &root, &["new", "fixture-base", "-m", "right branch"]);
    jj(&repo, &root, &["bookmark", "create", "fixture-right"]);
    jj(
        &repo,
        &root,
        &["bookmark", "create", "fixture-topic", "-r", "fixture-base"],
    );
    let linked = repo.path().join("fixture auxiliary/linked workspace");
    jj(
        &repo,
        &root,
        &[
            "workspace",
            "add",
            "--name",
            "fixture-linked",
            linked.to_str().unwrap(),
        ],
    );

    // A shared immutable starting operation makes both bookmark moves concurrent.
    let starting_operation = operation_head(&root);
    for target in ["fixture-left", "fixture-right"] {
        jj(
            &repo,
            &root,
            &[
                "--at-operation",
                &starting_operation,
                "bookmark",
                "set",
                "fixture-topic",
                "-r",
                target,
            ],
        );
    }
    assert_eq!(
        bounded_files(&root.join(".jj/repo/op_heads/heads"), 3).len(),
        2
    );
    jj(&repo, &root, &["status"]);
    let conflict = jj(&repo, &root, &["bookmark", "list", "--conflicted"]);
    let conflict = String::from_utf8(conflict.stdout).unwrap();
    assert!(conflict.contains("fixture-topic"));
    assert!(conflict.contains("conflict"));
    let integrated_operation = operation_head(&root);

    let dirty = root.join("unsnapshotted.txt");
    let linked_dirty = linked.join("unsnapshotted-linked.txt");
    fs::write(&dirty, "left dirty during decode\n").unwrap();
    fs::write(&linked_dirty, "also left dirty during decode\n").unwrap();
    let store = root.join(".jj/repo/op_store");
    let operations = bounded_files(&store.join("operations"), 32);
    let views = bounded_files(&store.join("views"), 32);
    assert!((10..=32).contains(&operations.len()));
    assert!((10..=32).contains(&views.len()));
    for path in &operations {
        assert!(fs::metadata(path).unwrap().len() <= MAX_OPERATION_BYTES as u64);
    }
    for path in &views {
        assert!(fs::metadata(path).unwrap().len() <= MAX_VIEW_BYTES as u64);
    }
    let before = snapshot(repo.path());
    let mut saw_integrated_join = false;
    let mut decoded_view_ids = std::collections::BTreeSet::new();
    for path in operations {
        let operation_id = path.file_name().unwrap().to_str().unwrap();
        let operation = decode_operation(
            JJ_OBSERVATION_READER_PROFILE,
            operation_id,
            &read_jj_fixture(&path, MAX_OPERATION_BYTES),
        )
        .unwrap();
        let view_path = store.join("views").join(&operation.view_id);
        let view = decode_view(
            JJ_OBSERVATION_READER_PROFILE,
            &operation.view_id,
            &read_jj_fixture(&view_path, MAX_VIEW_BYTES),
        )
        .unwrap();
        assert_eq!(view.view_id, operation.view_id);
        assert!(!view.head_ids.is_empty());
        assert!(view.head_ids.iter().all(|id| id.len() == 40));
        assert!(view.wc_commit_ids.values().all(|id| id.len() == 40));
        decoded_view_ids.insert(view.view_id.clone());
        if operation_id == integrated_operation {
            assert_eq!(operation.parent_ids.len(), 2);
            assert_eq!(view.wc_commit_ids.len(), 2);
            assert!(view.wc_commit_ids.contains_key("default"));
            assert!(view.wc_commit_ids.contains_key("fixture-linked"));
            assert!(view.commit_references > view.head_ids.len() + view.wc_commit_ids.len());
            saw_integrated_join = true;
        }
    }
    assert!(saw_integrated_join);
    assert_eq!(decoded_view_ids.len(), views.len());
    assert_eq!(
        fs::read_to_string(dirty).unwrap(),
        "left dirty during decode\n"
    );
    assert_eq!(
        fs::read_to_string(linked_dirty).unwrap(),
        "also left dirty during decode\n"
    );
    assert_eq!(snapshot(repo.path()), before);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_view_real_colocated_store_qualifies_refs_workspaces_and_conflicts_without_writes() {
    qualify_view_store(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_view_real_noncolocated_store_qualifies_refs_workspaces_and_conflicts_without_writes() {
    qualify_view_store(false);
}
