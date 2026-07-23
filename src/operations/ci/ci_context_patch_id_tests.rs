use super::commit_ranges_have_same_patch_ids;
use crate::operations::git::test_utils::TmpRepo;

fn repo_with_distinct_patches() -> (TmpRepo, String, String) {
    let repo = TmpRepo::new().expect("test repo");
    repo.write_file("base.txt", "base\n", false)
        .expect("write base");
    repo.commit_all("base").expect("base commit");
    repo.write_file("one.txt", "one\n", false)
        .expect("write first patch");
    let first = repo.commit_all("first").expect("first commit");
    repo.write_file("two.txt", "two\n", false)
        .expect("write second patch");
    let second = repo.commit_all("second").expect("second commit");
    (repo, first, second)
}

fn repo_with_equivalent_patches() -> (TmpRepo, String, String) {
    let repo = TmpRepo::new().expect("test repo");
    repo.write_file("file.txt", "base\n", false)
        .expect("write base");
    let base = repo.commit_all("base").expect("base commit");

    repo.write_file("file.txt", "changed\n", false)
        .expect("write first patch");
    let first = repo.commit_all("first version").expect("first commit");

    repo.git_command(&["reset", "--hard", &base])
        .expect("reset to base");
    repo.write_file("file.txt", "changed\n", false)
        .expect("write equivalent patch");
    let equivalent = repo
        .commit_all("equivalent version")
        .expect("equivalent commit");

    assert_ne!(first, equivalent, "test requires distinct commit IDs");
    (repo, first, equivalent)
}

#[test]
fn ranges_match_equal_patches() {
    let (repo, first, equivalent) = repo_with_equivalent_patches();

    assert!(
        commit_ranges_have_same_patch_ids(repo.gitai_repo(), &[first], &[equivalent])
            .expect("compare equal patches")
    );
}

#[test]
fn ranges_reject_reordered_commits() {
    let (repo, first, second) = repo_with_distinct_patches();

    assert!(
        !commit_ranges_have_same_patch_ids(
            repo.gitai_repo(),
            &[first.clone(), second.clone()],
            &[second, first],
        )
        .expect("compare reordered ranges")
    );
}

#[test]
fn ranges_reject_changed_patches() {
    let (repo, first, second) = repo_with_distinct_patches();

    assert!(
        !commit_ranges_have_same_patch_ids(repo.gitai_repo(), &[first], &[second],)
            .expect("compare changed ranges")
    );
}

#[test]
fn ranges_reject_length_mismatch() {
    let (repo, first, _) = repo_with_distinct_patches();

    assert!(
        !commit_ranges_have_same_patch_ids(repo.gitai_repo(), &[first], &[])
            .expect("compare different-length ranges")
    );
}

#[test]
fn ranges_treat_distinct_empty_commits_as_equal() {
    let repo = TmpRepo::new().expect("test repo");
    repo.write_file("base.txt", "base\n", false)
        .expect("write base");
    repo.commit_all("base").expect("base commit");
    let first_empty = repo.commit_all("empty one").expect("first empty commit");
    let second_empty = repo.commit_all("empty two").expect("second empty commit");
    assert_ne!(
        first_empty, second_empty,
        "test requires distinct empty commits"
    );

    assert!(
        commit_ranges_have_same_patch_ids(repo.gitai_repo(), &[first_empty], &[second_empty])
            .expect("compare empty commits")
    );
}
