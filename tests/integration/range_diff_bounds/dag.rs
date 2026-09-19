use super::*;

#[test]
fn parallel_history_that_exceeds_the_suffix_budget_skips_range_diff() {
    let log_dir = tempfile::tempdir().unwrap();
    let log_path = log_dir.path().join("spawns.log");
    let repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_RANGE_DIFF_COMMIT_LIMIT", "3"),
        ("GIT_AI_SPAWN_LOG", log_path.to_str().unwrap()),
    ]);
    let feature_tip = prepare_branch_move(&repo, 0, 6);
    repo.git(&["checkout", "-b", "parallel", "old-stack~1"])
        .unwrap();
    let mut parallel = repo.filename("parallel.txt");
    for index in 0..4 {
        let lines = (0..=index)
            .map(|n| format!("Parallel {n}").human())
            .collect::<Vec<_>>();
        parallel.set_contents(lines.clone());
        repo.stage_all_and_commit(&format!("Parallel {index}"))
            .unwrap();
        parallel.assert_committed_lines(lines);
    }
    repo.git(&["merge", "--no-ff", "-m", "Merge upstream", &feature_tip])
        .unwrap();
    parallel.assert_committed_lines((0..4).map(|n| format!("Parallel {n}").human()).collect());
    repo.filename("feature.txt")
        .assert_committed_lines(crate::lines!["AI feature".unattributed_human()]);
    let new_tip = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let suffix = repo
        .git(&[
            "rev-list",
            "--topo-order",
            "--max-count=4",
            "old-stack~1..HEAD",
        ])
        .unwrap();
    let candidate = suffix.lines().nth(3).unwrap();
    let count = repo
        .git(&["rev-list", "--count", &format!("{candidate}..{new_tip}")])
        .unwrap()
        .trim()
        .parse::<usize>()
        .unwrap();
    assert!(
        count > 3,
        "fixture must exceed the budget across its parents"
    );

    repo.git(&["checkout", "old-stack"]).unwrap();
    repo.sync_daemon();
    fs::write(&log_path, "").unwrap();
    repo.git(&["reset", "--keep", &new_tip]).unwrap();
    repo.sync_daemon();
    let spawns = fs::read_to_string(&log_path).unwrap();
    assert!(
        !spawns.lines().any(|command| command == "range-diff"),
        "unbounded parallel history spawned range-diff: {spawns}"
    );
    repo.filename("feature.txt")
        .assert_committed_lines(crate::lines!["AI feature".unattributed_human()]);
}
