use super::{
    ExpectedLineExt, TestRepo, commit_tree_rewrite_current_branch, head_sha, open_repo, read_note,
    setup_initial_commit,
};

#[test]
fn test_commit_tree_update_ref_preserves_authorship_notes_on_reparent() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout feature should succeed");

    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(lines!["human line", "ai line".ai()]);
    let feature_commit = repo
        .stage_all_and_commit("feature commit")
        .expect("feature commit should succeed");

    let git_ai_repo = open_repo(&repo);
    assert!(
        read_note(&git_ai_repo, &feature_commit.commit_sha).is_some(),
        "expected initial feature commit to have an authorship note",
    );

    repo.git(&["checkout", "main"])
        .expect("checkout main should succeed");
    let mut trunk_file = repo.filename("trunk.txt");
    trunk_file.set_contents(lines!["trunk update"]);
    let main_commit = repo
        .stage_all_and_commit("main update")
        .expect("main update should succeed");

    repo.git(&["checkout", "feature"])
        .expect("checkout feature should succeed");
    let (old_head, new_head) = commit_tree_rewrite_current_branch(
        &repo,
        "feature",
        &main_commit.commit_sha,
        "feature commit",
    );

    repo.sync_daemon();

    let git_ai_repo = open_repo(&repo);
    assert!(
        read_note(&git_ai_repo, &new_head).is_some(),
        "expected rewritten commit {} to preserve authorship note from {}",
        new_head,
        old_head,
    );

    let mut rewritten_file = repo.filename("feature.txt");
    rewritten_file.assert_lines_and_blame(lines!["human line".human(), "ai line".ai()]);
}

#[test]
fn test_commit_tree_update_ref_moves_working_log_to_rewritten_head() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout feature should succeed");

    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(lines!["human line", "committed ai".ai()]);
    repo.stage_all_and_commit("feature commit")
        .expect("feature commit should succeed");

    repo.git(&["checkout", "main"])
        .expect("checkout main should succeed");
    let mut trunk_file = repo.filename("trunk.txt");
    trunk_file.set_contents(lines!["trunk update"]);
    let main_commit = repo
        .stage_all_and_commit("main update")
        .expect("main update should succeed");

    repo.git(&["checkout", "feature"])
        .expect("checkout feature should succeed");
    feature_file.set_contents_no_stage(lines![
        "human line",
        "committed ai".ai(),
        "pending ai".ai(),
    ]);

    repo.sync_daemon();

    let old_head = head_sha(&repo);
    let git_ai_repo = open_repo(&repo);
    assert!(
        git_ai_repo.storage.has_working_log(&old_head),
        "expected dirty branch to have a working log before rewrite",
    );

    let (_, new_head) = commit_tree_rewrite_current_branch(
        &repo,
        "feature",
        &main_commit.commit_sha,
        "feature commit",
    );

    repo.sync_daemon();

    let git_ai_repo = open_repo(&repo);
    assert!(
        git_ai_repo.storage.has_working_log(&new_head),
        "expected working log to follow rewritten HEAD from {} to {}",
        old_head,
        new_head,
    );
    assert!(
        !git_ai_repo.storage.has_working_log(&old_head),
        "expected working log for old HEAD {} to be renamed away",
        old_head,
    );

    repo.git(&["add", "-A"]).expect("git add should succeed");
    repo.commit("commit after plumbing rewrite")
        .expect("commit after plumbing rewrite should succeed");

    let mut rewritten_file = repo.filename("feature.txt");
    rewritten_file.assert_lines_and_blame(lines![
        "human line".human(),
        "committed ai".ai(),
        "pending ai".ai(),
    ]);
}

/// Replay multiple feature commits via commit-tree, then move the branch with
/// one update-ref from old tip to new tip. Git AI must detect the N-commit
/// rewrite and remap every authorship note.
#[test]
fn test_multi_commit_plumbing_rewrite_single_update_ref() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);
    let default_branch = repo.current_branch();

    // Create feature branch with 3 AI commits
    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout feature");

    let mut file_a = repo.filename("a.txt");
    file_a.set_contents(lines!["a1 ai".ai(), "a2 human"]);
    repo.stage_all_and_commit("feat: add file a")
        .expect("feat 1");

    let mut file_b = repo.filename("b.txt");
    file_b.set_contents(lines!["b1 ai".ai(), "b2 ai".ai()]);
    repo.stage_all_and_commit("feat: add file b")
        .expect("feat 2");

    file_a.set_contents(lines!["a1 ai".ai(), "a2 human", "a3 ai".ai()]);
    repo.stage_all_and_commit("feat: extend file a")
        .expect("feat 3");

    // Collect feature commits (oldest to newest)
    let feature_commits_str = repo
        .git(&[
            "rev-list",
            "--reverse",
            &format!("{}..HEAD", default_branch),
        ])
        .expect("rev-list");
    let feature_commits: Vec<&str> = feature_commits_str
        .trim()
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(feature_commits.len(), 3, "expected 3 feature commits");

    // Verify all 3 have authorship notes pre-rebase
    let git_ai_repo = open_repo(&repo);
    for &sha in &feature_commits {
        assert!(
            read_note(&git_ai_repo, sha).is_some(),
            "pre-rebase: commit {} should have authorship note",
            sha
        );
    }

    // Advance main so rebase has new base
    repo.git(&["checkout", &default_branch])
        .expect("checkout main");
    let mut trunk = repo.filename("trunk.txt");
    trunk.set_contents(lines!["trunk line 1"]);
    repo.stage_all_and_commit("main advance 1").expect("main 1");
    trunk.set_contents(lines!["trunk line 1", "trunk line 2"]);
    repo.stage_all_and_commit("main advance 2").expect("main 2");
    let main_tip = head_sha(&repo);

    // Switch back to feature for the replay
    repo.git(&["checkout", "feature"])
        .expect("checkout feature");
    let old_tip = head_sha(&repo);

    // Replay all commits via commit-tree (no update-ref yet)
    let mut new_parent = main_tip.clone();
    for &feature_sha in &feature_commits {
        let old_parent = repo
            .git(&["rev-parse", &format!("{}^", feature_sha)])
            .expect("rev-parse parent")
            .trim()
            .to_string();

        let merged_tree_output = repo
            .git(&[
                "merge-tree",
                "--write-tree",
                "--merge-base",
                &old_parent,
                &new_parent,
                feature_sha,
            ])
            .expect("merge-tree");
        let merged_tree = merged_tree_output
            .trim()
            .lines()
            .next()
            .unwrap()
            .to_string();

        let message = repo
            .git(&["log", "-1", "--format=%s", feature_sha])
            .expect("log message")
            .trim()
            .to_string();

        let new_commit = repo
            .git(&[
                "commit-tree",
                &merged_tree,
                "-p",
                &new_parent,
                "-m",
                &message,
            ])
            .expect("commit-tree")
            .trim()
            .to_string();

        new_parent = new_commit;
    }

    // Move the branch once after creating every replacement commit.
    let new_tip = new_parent;
    repo.git(&["update-ref", "refs/heads/feature", &new_tip, &old_tip])
        .expect("update-ref");
    repo.git(&["reset", "--hard", &new_tip]).expect("reset");

    repo.sync_daemon();

    // Verify all 3 rebased commits have authorship notes
    let rebased_commits_str = repo
        .git(&["rev-list", "--reverse", &format!("{}..HEAD", main_tip)])
        .expect("rev-list rebased");
    let rebased_commits: Vec<&str> = rebased_commits_str
        .trim()
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(rebased_commits.len(), 3, "expected 3 rebased commits");

    let git_ai_repo = open_repo(&repo);
    for (idx, &sha) in rebased_commits.iter().enumerate() {
        assert!(
            read_note(&git_ai_repo, sha).is_some(),
            "post-rebase: rebased commit {} (index {}) should have authorship note",
            sha,
            idx
        );
    }

    // Verify attribution on file_b (single-commit, straightforward)
    file_b.assert_lines_and_blame(lines!["b1 ai".ai(), "b2 ai".ai()]);
}
