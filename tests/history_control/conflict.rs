use super::*;

fn abort_discards_resolution(operation: &str) {
    let repo = conflicting_repo(operation);
    write_ai(&repo, "conflict.txt", "discarded resolution\n");
    repo.git(&["add", "conflict.txt"]).unwrap();
    repo.git(&[operation, "--abort"]).unwrap();
    fs::write(repo.path().join("conflict.txt"), "discarded resolution\n").unwrap();
    repo.stage_all_and_commit("recreate without checkpoint")
        .unwrap();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["discarded resolution".unattributed_human()]);
}

fn quit_retains_resolution(operation: &str) {
    let repo = conflicting_repo(operation);
    write_ai(&repo, "conflict.txt", "retained resolution\n");
    repo.git(&["add", "conflict.txt"]).unwrap();
    repo.git(&[operation, "--quit"]).unwrap();
    repo.stage_all_and_commit("commit after quit").unwrap();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["retained resolution".ai()]);
}

fn continue_commits_resolution(operation: &str) {
    let repo = conflicting_repo(operation);
    write_ai(&repo, "conflict.txt", "committed resolution\n");
    write_ai(&repo, "unrelated.txt", "pending unrelated AI\n");
    repo.git(&["add", "conflict.txt"]).unwrap();
    repo.git_with_env(&[operation, "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["committed resolution".ai()]);
    repo.stage_all_and_commit("commit unrelated pending edit")
        .unwrap();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["committed resolution".ai()]);
    repo.filename("unrelated.txt")
        .assert_committed_lines(lines!["pending unrelated AI".ai()]);
}

macro_rules! control_cases {
    ($module:ident, $operation:literal) => {
        mod $module {
            use super::*;
            #[test]
            fn quit_retains_ai_resolution_for_plain_commit() {
                quit_retains_resolution($operation);
            }
            #[test]
            fn continue_attributes_new_ai_resolution() {
                continue_commits_resolution($operation);
            }
        }
    };
}

control_cases!(merge, "merge");
control_cases!(rebase, "rebase");
control_cases!(cherry_pick, "cherry-pick");
control_cases!(revert, "revert");

#[test]
fn rebase_abort_does_not_reuse_discarded_ai_resolution() {
    abort_discards_resolution("rebase");
}
