use super::{
    ExpectedLineExt, TestRepo,
    delayed_checkout_switch_merge_trace_replay_does_not_attribute_later_uncheckpointed_edit, fs,
    raw_git_trace_to_file, replay_trace_file_to_daemon, setup_initial_commit,
};

#[test]
fn test_delayed_stash_apply_trace_replay_preserves_named_stash_attribution() {
    let repo = TestRepo::new();
    let mut readme = repo.filename("README.md");
    readme.set_contents(lines!["base"]);
    repo.stage_all_and_commit("base").unwrap();

    let mut first = repo.filename("first.txt");
    first.set_contents(lines!["first stash ai".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai", "first.txt"])
        .unwrap();
    repo.git(&["stash", "push", "-m", "first"]).unwrap();

    let mut second = repo.filename("second.txt");
    second.set_contents(lines!["second stash ai".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai", "second.txt"])
        .unwrap();
    repo.git(&["stash", "push", "-m", "second"]).unwrap();

    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let apply_trace = trace_dir.path().join("stash-apply.trace2");

    raw_git_trace_to_file(&repo, &["stash", "apply", "stash@{1}"], &apply_trace);
    repo.git_og(&["stash", "drop", "stash@{1}"])
        .expect("drop applied stash after raw apply");

    replay_trace_file_to_daemon(&repo, &apply_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    repo.stage_all_and_commit("apply first stash").unwrap();
    first.assert_committed_lines(lines!["first stash ai".ai()]);
}

#[test]
fn test_delayed_stash_pop_trace_replay_preserves_popped_stash_attribution() {
    let repo = TestRepo::new();
    let mut readme = repo.filename("README.md");
    readme.set_contents(lines!["base"]);
    repo.stage_all_and_commit("base").unwrap();

    let mut first = repo.filename("first.txt");
    first.set_contents(lines!["first stash ai".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai", "first.txt"])
        .unwrap();
    repo.git(&["stash", "push", "-m", "first"]).unwrap();

    let mut second = repo.filename("second.txt");
    second.set_contents(lines!["second stash ai".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai", "second.txt"])
        .unwrap();
    repo.git(&["stash", "push", "-m", "second"]).unwrap();

    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let pop_trace = trace_dir.path().join("stash-pop.trace2");

    raw_git_trace_to_file(&repo, &["stash", "pop"], &pop_trace);
    repo.git_og(&["stash", "drop", "stash@{0}"])
        .expect("drop remaining stash after raw pop");

    replay_trace_file_to_daemon(&repo, &pop_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    repo.stage_all_and_commit("apply second stash").unwrap();
    second.assert_committed_lines(lines!["second stash ai".ai()]);
}

#[test]
fn test_delayed_switch_merge_trace_replay_does_not_attribute_later_uncheckpointed_edit() {
    delayed_checkout_switch_merge_trace_replay_does_not_attribute_later_uncheckpointed_edit(&[
        "switch", "--merge", "feature",
    ]);
}

#[test]
fn test_delayed_checkout_merge_trace_replay_does_not_attribute_later_uncheckpointed_edit() {
    delayed_checkout_switch_merge_trace_replay_does_not_attribute_later_uncheckpointed_edit(&[
        "checkout", "--merge", "feature",
    ]);
}

#[test]
fn test_delayed_switch_trace_replay_renames_working_log_for_uncommitted_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(repo.path().join("feature-only.txt"), "feature only\n").unwrap();
    repo.stage_all_and_commit("feature only").unwrap();
    repo.git(&["checkout", &default_branch]).unwrap();

    let mut file = repo.filename("plain-switch.txt");
    file.set_contents(lines!["plain switch ai".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai", "plain-switch.txt"])
        .unwrap();
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let switch_trace = trace_dir.path().join("switch.trace2");

    raw_git_trace_to_file(&repo, &["switch", "feature"], &switch_trace);
    replay_trace_file_to_daemon(&repo, &switch_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    repo.stage_all_and_commit("commit after plain switch")
        .unwrap();
    file.assert_committed_lines(lines!["plain switch ai".ai()]);
}
