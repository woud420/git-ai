use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj, snapshot};
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::model::jj_observation::{JjOperationEvidence, MAX_JJ_OBSERVATION_OPERATION_BYTES};
use git_ai::operations::jj::operation::decode_operation;
use std::fs;
use std::path::Path;

fn head(root: &Path) -> String {
    let heads: Vec<_> = fs::read_dir(root.join(".jj/repo/op_heads/heads"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(heads.len(), 1);
    heads[0].clone()
}

fn evidence(root: &Path, id: &str) -> JjOperationEvidence {
    let store = root.join(".jj/repo/op_store");
    let operation_bytes = read_jj_fixture(
        &store.join("operations").join(id),
        MAX_JJ_OBSERVATION_OPERATION_BYTES,
    );
    let operation = decode_operation(PROFILE, id, &operation_bytes).unwrap();
    let view_bytes = read_jj_fixture(
        &store.join("views").join(&operation.view_id),
        MAX_JJ_OBSERVATION_OPERATION_BYTES,
    );
    JjOperationEvidence {
        operation_id: id.to_owned(),
        parent_ids: operation.parent_ids,
        view_id: operation.view_id,
        operation_bytes,
        view_bytes,
    }
}

fn qualify(colocated: bool) {
    require_pinned_jj();
    for family in [
        "snapshot", "new", "commit", "describe", "rebase", "split", "squash", "absorb", "abandon",
        "undo", "restore", "import",
    ] {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let root = if colocated {
            jj(&repo, repo.path(), &["git", "init", "--colocate"]);
            repo.path().to_owned()
        } else {
            let root = repo.path().join("jj workspace");
            jj(
                &repo,
                repo.path(),
                &["git", "init", "--no-colocate", root.to_str().unwrap()],
            );
            root
        };
        for message in ["base", "middle"] {
            fs::write(root.join("a"), format!("alpha\n{message}\ngamma\n")).unwrap();
            fs::write(root.join("b"), format!("one\n{message}\nthree\n")).unwrap();
            jj(&repo, &root, &["commit", "-m", message]);
        }
        let restore_target = head(&root);
        if matches!(family, "snapshot" | "commit" | "absorb") {
            fs::write(root.join("a"), "alpha\nchanged\ngamma\n").unwrap();
        }
        // A subsequent undo/restore must visibly change the working-copy OID.
        if matches!(family, "undo" | "restore") {
            jj(&repo, &root, &["describe", "-m", "to be restored"]);
        }
        let commands: &[&str] = match family {
            "snapshot" => &["status"],
            "new" => &["new"],
            "commit" => &["commit", "-m", "final"],
            "describe" => &["describe", "-r", "@-", "-m", "renamed"],
            "rebase" => &["rebase", "-r", "@-", "-d", "root()"],
            "split" => &["split", "-r", "@-", "a", "-m", "split a"],
            "squash" => &["squash", "--from", "@-", "--into", "@--", "-m", "squashed"],
            "absorb" => &["absorb"],
            "abandon" => &["abandon", "-r", "@-"],
            "undo" => &["undo"],
            "restore" => &["op", "restore", &restore_target],
            // A noncolocated Git import may be a no-op. Add an external Git ref
            // below so both layouts exercise actual imported immutable commits.
            "import" => &["git", "import"],
            _ => unreachable!(),
        };
        if family == "import" {
            let git_dir = if colocated {
                root.join(".git")
            } else {
                root.join(".jj/repo/store/git")
            };
            let output = jj(
                &repo,
                &root,
                &[
                    "--ignore-working-copy",
                    "log",
                    "--no-graph",
                    "-r",
                    "@-",
                    "-T",
                    "commit_id",
                ],
            );
            let commit = String::from_utf8(output.stdout).unwrap();
            repo.git_without_test_sync_for_test(
                &[
                    "--git-dir",
                    git_dir.to_str().unwrap(),
                    "update-ref",
                    "refs/heads/imported",
                    commit.trim(),
                ],
                &[],
            )
            .unwrap();
        }
        jj(&repo, &root, commands);
        fs::write(
            root.join("unsnapshotted"),
            "observer must leave this dirty\n",
        )
        .unwrap();
        let before = snapshot(repo.path());
        let current = evidence(&root, &head(&root));
        let parents: Vec<_> = current
            .parent_ids
            .iter()
            .filter(|id| id.as_str() != "00".repeat(64))
            .map(|id| evidence(&root, id))
            .collect();
        let refs: Vec<_> = parents.iter().collect();
        let proof = verify_transition(PROFILE, &current, &refs).unwrap();
        let default = proof
            .workspaces()
            .iter()
            .find(|movement| movement.workspace_name == "default")
            .unwrap();
        assert_eq!(default.before.len(), current.parent_ids.len());
        assert!(default.after.is_some());
        let candidates = proof.recorded_predecessors().unwrap();
        match family {
            "snapshot" => {
                assert!(proof.operation().is_snapshot);
                assert_eq!(
                    candidates[default.after.as_ref().unwrap()],
                    [default.before[0].clone().unwrap()]
                );
            }
            "new" => assert!(candidates[default.after.as_ref().unwrap()].is_empty()),
            "split" => {
                let values: Vec<_> = candidates.values().collect();
                assert!(values.iter().any(|value| value.len() == 1
                    && values.iter().filter(|other| *other == value).count() > 1));
            }
            "squash" | "absorb" => assert!(candidates.values().any(|value| value.len() >= 2)),
            "undo" | "restore" => {
                assert!(
                    candidates.is_empty(),
                    "{family} does not record rewrite relations"
                );
                assert_ne!(
                    default.before[0], default.after,
                    "view movement must remain visible"
                );
            }
            "import" => assert!(
                candidates.is_empty(),
                "existing imported commits have no rewrite map"
            ),
            _ => assert!(!candidates.is_empty(), "{family}"),
        }
        assert_eq!(
            snapshot(repo.path()),
            before,
            "{family} changed metadata or files"
        );
    }
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_lineage_real_colocated_operation_families_preserve_evidence_without_writes() {
    qualify(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_lineage_real_noncolocated_operation_families_preserve_evidence_without_writes() {
    qualify(false);
}
