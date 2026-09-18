use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use std::fs;

mod dag;

fn prepare_branch_move(repo: &TestRepo, dropped: usize, landing: usize) -> String {
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base".human()]);
    let base_sha = repo.stage_all_and_commit("Base").unwrap().commit_sha;
    base.assert_committed_lines(crate::lines!["base".human()]);

    repo.git(&["checkout", "-b", "old-stack"]).unwrap();
    let mut feature = repo.filename("feature.txt");
    feature.set_contents(crate::lines!["AI feature".ai()]);
    let feature_sha = repo.stage_all_and_commit("Feature").unwrap().commit_sha;
    feature.assert_committed_lines(crate::lines!["AI feature".ai()]);
    let mut tail = repo.filename("tail.txt");
    for index in 0..dropped {
        let lines = (0..=index)
            .map(|n| format!("AI tail {n}").ai())
            .collect::<Vec<_>>();
        tail.set_contents(lines.clone());
        repo.stage_all_and_commit(&format!("Tail {index}")).unwrap();
        tail.assert_committed_lines(lines);
    }

    repo.git(&["checkout", "-b", "new-stack", &base_sha])
        .unwrap();
    let mut upstream = repo.filename("upstream.txt");
    for index in 0..landing {
        let lines = (0..=index)
            .map(|n| format!("Upstream {n}").human())
            .collect::<Vec<_>>();
        upstream.set_contents(lines.clone());
        repo.stage_all_and_commit(&format!("Upstream {index}"))
            .unwrap();
        upstream.assert_committed_lines(lines);
    }
    repo.git(&["cherry-pick", &feature_sha]).unwrap();
    feature.assert_committed_lines(crate::lines!["AI feature".ai()]);
    if landing == 0 {
        repo.git(&["commit", "--amend", "-m", "Rewritten feature"])
            .unwrap();
        feature.assert_committed_lines(crate::lines!["AI feature".ai()]);
    }
    let new_tip = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    // Remove the cherry-pick's note so the tested ref move must recover the
    // feature's authorship; an already annotated target would mask a no-op.
    repo.git(&["notes", "--ref=ai", "remove", &new_tip])
        .unwrap();
    feature.assert_committed_lines(crate::lines!["AI feature".unattributed_human()]);
    repo.git(&["checkout", "old-stack"]).unwrap();
    repo.sync_daemon();
    new_tip
}

#[test]
fn branch_move_discards_oversized_post_match_squash_candidates() {
    let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_DEBUG", "1")]);
    let new_tip = prepare_branch_move(&repo, 65, 0);
    let range_diff = repo
        .git(&[
            "range-diff",
            "-s",
            "--creation-factor=100",
            "old-stack~66..old-stack",
            "old-stack~66..new-stack",
        ])
        .unwrap();
    let statuses = range_diff
        .lines()
        .filter_map(|line| line.split_whitespace().nth(2))
        .collect::<Vec<_>>();
    assert_eq!(
        statuses.len(),
        66,
        "unexpected fixture matching: {range_diff}"
    );
    assert!(matches!(statuses[0], "=" | "!"));
    assert!(
        statuses[1..].iter().all(|status| *status == "<"),
        "fixture must have 65 consecutive drops: {range_diff}"
    );
    let previous_log = repo.daemon_diagnostics().1.len();
    repo.git(&["reset", "--keep", &new_tip]).unwrap();
    repo.sync_daemon();
    repo.filename("feature.txt")
        .assert_committed_lines(crate::lines!["AI feature".ai()]);
    assert!(!repo.path().join("tail.txt").exists());
    let log = repo.daemon_diagnostics().1;
    assert!(
        log[previous_log..].contains("shift_authorship_notes: 1 mappings"),
        "unbounded dropped mappings were retained: {}",
        &log[previous_log..]
    );
}

#[test]
fn oversized_old_range_skips_range_diff_before_spawning_it() {
    let log_dir = tempfile::tempdir().unwrap();
    let log_path = log_dir.path().join("spawns.log");
    let repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_RANGE_DIFF_COMMIT_LIMIT", "4"),
        ("GIT_AI_SPAWN_LOG", log_path.to_str().unwrap()),
    ]);
    let new_tip = prepare_branch_move(&repo, 5, 1);
    fs::write(&log_path, "").unwrap();
    repo.git(&["reset", "--keep", &new_tip]).unwrap();
    repo.sync_daemon();
    let spawns = fs::read_to_string(&log_path).unwrap();
    assert!(
        !spawns.lines().any(|command| command == "range-diff"),
        "oversized range spawned range-diff: {spawns}"
    );
    repo.filename("feature.txt")
        .assert_committed_lines(crate::lines!["AI feature".unattributed_human()]);
}

#[test]
fn bounded_new_suffix_preserves_a_small_stack_on_long_upstream_history() {
    let log_dir = tempfile::tempdir().unwrap();
    let log_path = log_dir.path().join("spawns.log");
    let repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_RANGE_DIFF_COMMIT_LIMIT", "3"),
        ("GIT_AI_SPAWN_LOG", log_path.to_str().unwrap()),
    ]);
    let new_tip = prepare_branch_move(&repo, 0, 6);
    fs::write(&log_path, "").unwrap();
    repo.git(&["reset", "--keep", &new_tip]).unwrap();
    repo.sync_daemon();
    repo.filename("feature.txt")
        .assert_committed_lines(crate::lines!["AI feature".ai()]);
    let spawns = fs::read_to_string(&log_path).unwrap();
    assert_eq!(
        spawns
            .lines()
            .filter(|command| *command == "range-diff")
            .count(),
        1
    );
}
