use super::*;
use crate::operations::git::test_utils::TmpRepo;

#[test]
fn test_ci_event_debug() {
    let event = CiEvent::Merge {
        merge_commit_sha: "abc123".to_string(),
        head_ref: "feature".to_string(),
        head_sha: "def456".to_string(),
        base_ref: "main".to_string(),
        base_sha: "ghi789".to_string(),
        fork_clone_url: None,
    };

    let debug_str = format!("{:?}", event);
    assert!(debug_str.contains("Merge"));
    assert!(debug_str.contains("abc123"));
    assert!(debug_str.contains("feature"));
}

#[test]
fn test_ci_run_result_debug() {
    let result = CiRunResult::SkippedSimpleMerge;
    let debug_str = format!("{:?}", result);
    assert!(debug_str.contains("SkippedSimpleMerge"));

    let result2 = CiRunResult::SkippedFastForward;
    let debug_str2 = format!("{:?}", result2);
    assert!(debug_str2.contains("SkippedFastForward"));

    let result3 = CiRunResult::NoAuthorshipAvailable;
    let debug_str3 = format!("{:?}", result3);
    assert!(debug_str3.contains("NoAuthorshipAvailable"));
}

#[test]
fn commit_is_ancestor_returns_false_for_unrelated_histories() {
    let repo = TmpRepo::new().expect("test repo");
    repo.write_file("main.txt", "main", false)
        .expect("write main");
    let main_sha = repo.commit_all("main commit").expect("main commit");

    repo.git_command(&["switch", "--orphan", "unrelated"])
        .expect("orphan branch");
    repo.git_command(&["rm", "-rf", "--ignore-unmatch", "."])
        .expect("clear tree");
    repo.write_file("unrelated.txt", "unrelated", false)
        .expect("write unrelated");
    let unrelated_sha = repo
        .commit_all("unrelated commit")
        .expect("unrelated commit");

    assert!(
        !commit_is_ancestor(repo.gitai_repo(), &main_sha, &unrelated_sha)
            .expect("unrelated histories should not error")
    );
}

#[test]
fn commit_is_ancestor_errors_for_invalid_descendant() {
    let repo = TmpRepo::new().expect("test repo");
    repo.write_file("main.txt", "main", false)
        .expect("write main");
    let main_sha = repo.commit_all("main commit").expect("main commit");

    assert!(commit_is_ancestor(repo.gitai_repo(), &main_sha, "not-a-sha").is_err());
}

#[test]
fn sync_fetch_uses_blobless_only_for_named_promisor_remote() {
    let repo = TmpRepo::new().expect("test repo");

    assert!(
        !sync_fetch_remote_supports_lazy_blobs(repo.gitai_repo(), "origin")
            .expect("missing promisor config should be false")
    );

    repo.git_command(&["config", "remote.origin.promisor", "true"])
        .expect("set promisor config");
    assert!(
        sync_fetch_remote_supports_lazy_blobs(repo.gitai_repo(), "origin")
            .expect("named promisor remote should allow blobless fetch")
    );

    assert!(
        !sync_fetch_remote_supports_lazy_blobs(
            repo.gitai_repo(),
            "https://github.com/acme/repo.git"
        )
        .expect("direct URL should not use blobless fetch")
    );
    assert!(
        !sync_fetch_remote_supports_lazy_blobs(repo.gitai_repo(), "git@github.com:acme/repo.git")
            .expect("direct SSH URL should not use blobless fetch")
    );
}
