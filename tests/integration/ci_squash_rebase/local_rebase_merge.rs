use super::{AuthorshipLog, ExpectedLineExt, direct_test_repo};

#[test]
fn test_ci_local_rebase_merge_with_abbreviated_merge_sha() {
    let repo = direct_test_repo();

    // --- Initial commit on main ---
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();
    let base_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // --- Feature branch: two commits touching different files ---
    repo.git_og(&["checkout", "-b", "feature"]).unwrap();
    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["ai content in file_a".ai()]);
    let _feature_sha1 = repo.stage_all_and_commit("Add file_a").unwrap().commit_sha;
    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["ai content in file_b".ai()]);
    let feature_sha2 = repo.stage_all_and_commit("Add file_b").unwrap().commit_sha;

    // --- Advance main so the rebase produces new commit SHAs ---
    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_file = repo.filename("main_only.txt");
    main_file.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Advance main"]).unwrap();

    // --- Rebase feature onto main (bypassing the local hook), then ff main ---
    repo.git_og(&["checkout", "feature"]).unwrap();
    repo.git_og(&["rebase", "main"]).unwrap();
    let new_sha2 = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let new_sha1 = repo
        .git_og(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--ff-only", "feature"]).unwrap();

    // --- Bare origin so push_authorship inside CiContext can succeed ---
    let origin_dir = tempfile::tempdir().unwrap();
    let origin_path = origin_dir.path().join("origin.git");
    repo.git_og(&[
        "clone",
        "--bare",
        repo.path().to_str().unwrap(),
        origin_path.to_str().unwrap(),
    ])
    .unwrap();
    repo.git_og(&["remote", "add", "origin", origin_path.to_str().unwrap()])
        .unwrap();

    // --- Run `ci local merge` with an ABBREVIATED merge-commit-sha ---
    let abbreviated_merge_sha = &new_sha2[..12];
    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "merge",
            "--merge-commit-sha",
            abbreviated_merge_sha,
            "--head-ref",
            "feature",
            "--head-sha",
            feature_sha2.as_str(),
            "--base-ref",
            "main",
            "--base-sha",
            base_sha.as_str(),
            "--skip-fetch-notes",
            "--skip-fetch-base",
        ])
        .expect("ci local merge should succeed");

    assert!(
        output.contains("authorship rewritten successfully"),
        "expected authorship rewritten, got: {output}"
    );

    // --- Each rebased commit must still carry its own note (rebase path kept) ---
    let note1 = repo
        .read_authorship_note(&new_sha1)
        .expect("rebased commit 1 should have a note (rebase must not be misclassified as squash)");
    let note2 = repo
        .read_authorship_note(&new_sha2)
        .expect("rebased commit 2 should have a note");

    let files = |note: &str| -> Vec<String> {
        AuthorshipLog::deserialize_from_string(note)
            .unwrap()
            .attestations
            .iter()
            .map(|a| a.file_path.clone())
            .collect()
    };
    let files1 = files(&note1);
    let files2 = files(&note2);

    assert!(
        files1.iter().any(|f| f.contains("file_a")) && !files1.iter().any(|f| f.contains("file_b")),
        "rebased commit 1 should reference only file_a.txt, got: {files1:?}"
    );
    assert!(
        files2.iter().any(|f| f.contains("file_b")) && !files2.iter().any(|f| f.contains("file_a")),
        "rebased commit 2 should reference only file_b.txt, got: {files2:?}"
    );
}

/// Verify that `git-ai ci local merge` correctly pairs original commits with
/// their rebased counterparts (oldest-first) after a real `git rebase`.
///
/// Creates a two-commit feature branch (commit 1 → file_a.txt, commit 2 →
/// file_b.txt), advances main by one commit so the rebase produces genuinely
/// new SHAs, then rebases the feature branch onto main via plain `git rebase`
/// (bypassing the local hook).  After fast-forwarding main, the test invokes
/// `git-ai ci local merge` exactly as CI would and checks that:
///
/// - The first rebased commit's authorship note references only file_a.txt
/// - The second rebased commit's authorship note references only file_b.txt
///
/// Before the `.reverse()` fix in `ci_context.rs` the pairing was inverted:
/// original_commits came back newest-first from `CommitRange::all_commits()`
/// while new_commits were oldest-first, so each note landed on the wrong commit.
#[test]
fn test_ci_local_rebase_merge_two_commits() {
    use git_ai::model::authorship_log_serialization::AuthorshipLog;

    let repo = direct_test_repo();

    // --- Initial commit on main ---
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();
    let base_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // --- Feature branch: two commits touching different files ---
    repo.git_og(&["checkout", "-b", "feature"]).unwrap();

    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["ai content in file_a".ai()]);
    let feature_sha1 = repo.stage_all_and_commit("Add file_a").unwrap().commit_sha;

    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["ai content in file_b".ai()]);
    let feature_sha2 = repo.stage_all_and_commit("Add file_b").unwrap().commit_sha;

    // --- Advance main so the rebase produces new commit SHAs ---
    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_file = repo.filename("main_only.txt");
    main_file.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Advance main"]).unwrap();

    // --- Rebase feature onto main, bypassing the local rebase hook ---
    repo.git_og(&["checkout", "feature"]).unwrap();
    repo.git_og(&["rebase", "main"]).unwrap();

    let new_sha2 = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let new_sha1 = repo
        .git_og(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    assert_ne!(
        new_sha1, feature_sha1,
        "rebase must produce a new SHA for commit 1"
    );
    assert_ne!(
        new_sha2, feature_sha2,
        "rebase must produce a new SHA for commit 2"
    );

    // --- Fast-forward main to the rebased feature HEAD ---
    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--ff-only", "feature"]).unwrap();

    // --- Bare clone so push_authorship("origin") inside CiContext can succeed ---
    let origin_dir = tempfile::tempdir().unwrap();
    let origin_path = origin_dir.path().join("origin.git");
    repo.git_og(&[
        "clone",
        "--bare",
        repo.path().to_str().unwrap(),
        origin_path.to_str().unwrap(),
    ])
    .unwrap();
    repo.git_og(&["remote", "add", "origin", origin_path.to_str().unwrap()])
        .unwrap();

    // --- Run the local CI command as CI would after a rebase merge ---
    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "merge",
            "--merge-commit-sha",
            new_sha2.as_str(),
            "--head-ref",
            "feature",
            "--head-sha",
            feature_sha2.as_str(),
            "--base-ref",
            "main",
            "--base-sha",
            base_sha.as_str(),
            "--skip-fetch-notes",
            "--skip-fetch-base",
        ])
        .expect("ci local merge should succeed");

    assert!(
        output.contains("authorship rewritten successfully"),
        "Expected authorship rewritten, got: {}",
        output
    );

    // --- Verify each rebased commit carries notes for its own file only ---
    let note1 = repo
        .read_authorship_note(&new_sha1)
        .expect("rebased commit 1 should have an authorship note");
    let note2 = repo
        .read_authorship_note(&new_sha2)
        .expect("rebased commit 2 should have an authorship note");

    let files1: Vec<String> = AuthorshipLog::deserialize_from_string(&note1)
        .unwrap()
        .attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect();
    let files2: Vec<String> = AuthorshipLog::deserialize_from_string(&note2)
        .unwrap()
        .attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect();

    assert!(
        files1.iter().any(|f| f.contains("file_a")),
        "rebased commit 1 should reference file_a.txt, got: {:?}",
        files1
    );
    assert!(
        !files1.iter().any(|f| f.contains("file_b")),
        "COMMIT ORDER BUG: rebased commit 1 references file_b (newest-first pairing). Got: {:?}",
        files1
    );
    assert!(
        files2.iter().any(|f| f.contains("file_b")),
        "rebased commit 2 should reference file_b.txt, got: {:?}",
        files2
    );
    assert!(
        !files2.iter().any(|f| f.contains("file_a")),
        "COMMIT ORDER BUG: rebased commit 2 references file_a (newest-first pairing). Got: {:?}",
        files2
    );
}

/// Three-commit variant of `test_ci_local_rebase_merge_two_commits`.
///
/// Each of the three original commits touches a distinct file (file_a / file_b /
/// file_c).  After rebasing onto an advanced main and running
/// `git-ai ci local merge`, every rebased commit must carry the note for its
/// own file and none of the others.  This catches both full inversions
/// (first↔last) and off-by-one shifts in the positional pairing.
#[test]
fn test_ci_local_rebase_merge_three_commits() {
    use git_ai::model::authorship_log_serialization::AuthorshipLog;

    let repo = direct_test_repo();

    // --- Initial commit on main ---
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();
    let base_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // --- Feature branch: three commits touching distinct files ---
    repo.git_og(&["checkout", "-b", "feature"]).unwrap();

    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["ai content in file_a".ai()]);
    let feature_sha1 = repo.stage_all_and_commit("Add file_a").unwrap().commit_sha;

    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["ai content in file_b".ai()]);
    let feature_sha2 = repo.stage_all_and_commit("Add file_b").unwrap().commit_sha;

    let mut file_c = repo.filename("file_c.txt");
    file_c.set_contents(crate::lines!["ai content in file_c".ai()]);
    let feature_sha3 = repo.stage_all_and_commit("Add file_c").unwrap().commit_sha;

    // --- Advance main so the rebase produces new commit SHAs ---
    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_file = repo.filename("main_only.txt");
    main_file.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Advance main"]).unwrap();

    // --- Rebase feature onto main, bypassing the local rebase hook ---
    repo.git_og(&["checkout", "feature"]).unwrap();
    repo.git_og(&["rebase", "main"]).unwrap();

    let new_sha3 = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let new_sha2 = repo
        .git_og(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    let new_sha1 = repo
        .git_og(&["rev-parse", "HEAD~2"])
        .unwrap()
        .trim()
        .to_string();

    assert_ne!(
        new_sha1, feature_sha1,
        "rebase must produce a new SHA for commit 1"
    );
    assert_ne!(
        new_sha2, feature_sha2,
        "rebase must produce a new SHA for commit 2"
    );
    assert_ne!(
        new_sha3, feature_sha3,
        "rebase must produce a new SHA for commit 3"
    );

    // --- Fast-forward main to the rebased feature HEAD ---
    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--ff-only", "feature"]).unwrap();

    // --- Bare clone so push_authorship("origin") inside CiContext can succeed ---
    let origin_dir = tempfile::tempdir().unwrap();
    let origin_path = origin_dir.path().join("origin.git");
    repo.git_og(&[
        "clone",
        "--bare",
        repo.path().to_str().unwrap(),
        origin_path.to_str().unwrap(),
    ])
    .unwrap();
    repo.git_og(&["remote", "add", "origin", origin_path.to_str().unwrap()])
        .unwrap();

    // --- Run the local CI command as CI would after a rebase merge ---
    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "merge",
            "--merge-commit-sha",
            new_sha3.as_str(),
            "--head-ref",
            "feature",
            "--head-sha",
            feature_sha3.as_str(),
            "--base-ref",
            "main",
            "--base-sha",
            base_sha.as_str(),
            "--skip-fetch-notes",
            "--skip-fetch-base",
        ])
        .expect("ci local merge should succeed");

    assert!(
        output.contains("authorship rewritten successfully"),
        "Expected authorship rewritten, got: {}",
        output
    );

    // --- Verify each rebased commit carries notes for its own file only ---
    let note1 = repo
        .read_authorship_note(&new_sha1)
        .expect("rebased commit 1 should have an authorship note");
    let note2 = repo
        .read_authorship_note(&new_sha2)
        .expect("rebased commit 2 should have an authorship note");
    let note3 = repo
        .read_authorship_note(&new_sha3)
        .expect("rebased commit 3 should have an authorship note");

    let files = |note: &str| -> Vec<String> {
        AuthorshipLog::deserialize_from_string(note)
            .unwrap()
            .attestations
            .iter()
            .map(|a| a.file_path.clone())
            .collect()
    };

    let files1 = files(&note1);
    let files2 = files(&note2);
    let files3 = files(&note3);

    // Commit 1 → file_a only
    assert!(
        files1.iter().any(|f| f.contains("file_a")),
        "rebased commit 1 should reference file_a.txt, got: {:?}",
        files1
    );
    assert!(
        !files1
            .iter()
            .any(|f| f.contains("file_b") || f.contains("file_c")),
        "COMMIT ORDER BUG: rebased commit 1 references wrong file. Got: {:?}",
        files1
    );

    // Commit 2 → file_b only
    assert!(
        files2.iter().any(|f| f.contains("file_b")),
        "rebased commit 2 should reference file_b.txt, got: {:?}",
        files2
    );
    assert!(
        !files2
            .iter()
            .any(|f| f.contains("file_a") || f.contains("file_c")),
        "COMMIT ORDER BUG: rebased commit 2 references wrong file. Got: {:?}",
        files2
    );

    // Commit 3 → file_c only
    assert!(
        files3.iter().any(|f| f.contains("file_c")),
        "rebased commit 3 should reference file_c.txt, got: {:?}",
        files3
    );
    assert!(
        !files3
            .iter()
            .any(|f| f.contains("file_a") || f.contains("file_b")),
        "COMMIT ORDER BUG: rebased commit 3 references wrong file. Got: {:?}",
        files3
    );
}

crate::reuse_tests_in_worktree!(
    test_ci_local_rebase_merge_with_abbreviated_merge_sha,
    test_ci_local_rebase_merge_two_commits,
    test_ci_local_rebase_merge_three_commits,
);
