#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;

#[path = "history_control/conflict.rs"]
mod conflict;

fn write_ai(repo: &TestRepo, path: &str, contents: &str) {
    repo.git_ai(&["checkpoint", "human", path]).unwrap();
    fs::write(repo.path().join(path), contents).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", path]).unwrap();
}

fn conflicting_repo(operation: &str) -> TestRepo {
    conflicting_repo_from(TestRepo::new(), operation)
}

fn conflicting_repo_from(repo: TestRepo, operation: &str) -> TestRepo {
    fs::write(repo.path().join("conflict.txt"), "base\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    let main = repo.current_branch();
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    write_ai(&repo, "conflict.txt", "feature AI\n");
    let feature = repo.stage_all_and_commit("feature").unwrap().commit_sha;
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["feature AI".ai()]);
    if operation != "revert" {
        repo.git(&["checkout", &main]).unwrap();
    }
    fs::write(repo.path().join("conflict.txt"), "main\n").unwrap();
    repo.stage_all_and_commit("divergent edit").unwrap();
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["main".unattributed_human()]);
    let target = match operation {
        "merge" => "feature".to_owned(),
        "rebase" => {
            repo.git(&["checkout", "feature"]).unwrap();
            main
        }
        _ => feature,
    };
    assert!(repo.git(&[operation, &target]).is_err());
    assert!(
        fs::read_to_string(repo.path().join("conflict.txt"))
            .unwrap()
            .contains("<<<<<<<")
    );
    repo
}

#[path = "history_control/delayed.rs"]
mod delayed;
