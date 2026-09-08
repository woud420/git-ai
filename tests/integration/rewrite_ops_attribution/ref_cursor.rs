use super::{
    ExpectedLineExt, HashMap, InitialAttributions, LineAttribution, TestRepo, commit_ai_line,
    current_branch_reflog, delayed_ai_commit_without_harness_sync_with_delay, fs, head_reflog,
    truncate_reflog_to_first_entry,
};

// =============================================================================
// Category 0: Trace2 ref-cursor branch lifecycle
// =============================================================================

/// Deleting a branch removes its reflog file. Recreating the same branch name
/// starts a new reflog generation at byte 0, so the daemon cursor must clear any
/// offset it learned from the previous generation.
#[test]
fn test_branch_delete_recreate_resets_trace2_ref_cursor() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["base".ai()]);

    let main_branch = repo.current_branch();
    fs::write(&file_path, "base\nmain\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("main advance").unwrap();
    file.assert_committed_lines(crate::lines!["base".ai(), "main".ai()]);

    let initial = repo.git(&["rev-parse", "HEAD~1"]).unwrap();
    let initial = initial.trim().to_string();
    repo.git(&["checkout", "-b", "rebase-side", initial.as_str()])
        .unwrap();
    fs::write(&file_path, "base\nside\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("side advance").unwrap();
    file.assert_committed_lines(crate::lines!["base".ai(), "side".ai()]);

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["branch", "-D", "rebase-side"]).unwrap();

    repo.git(&["checkout", "-b", "rebase-side", initial.as_str()])
        .unwrap();
    file.assert_committed_lines(crate::lines!["base".ai()]);
}

/// If an out-of-band raw git commit moves HEAD without trace2/hook handling,
/// the next traced commit must not consume that stale HEAD reflog entry as its
/// own ref transition.
#[test]
fn test_raw_git_commit_before_traced_commit_does_not_poison_ref_cursor() {
    let repo = TestRepo::new();

    let base_path = repo.path().join("base.txt");
    fs::write(&base_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "base.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let raw_path = repo.path().join("raw.txt");
    fs::write(&raw_path, "raw human\n").unwrap();
    repo.git_og(&["add", "raw.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "raw human commit"]).unwrap();

    let ai_path = repo.path().join("ai.txt");
    fs::write(&ai_path, "ai tracked\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "ai.txt"]).unwrap();
    repo.stage_all_and_commit("ai tracked commit").unwrap();

    let mut ai_file = repo.filename("ai.txt");
    ai_file.assert_committed_lines(crate::lines!["ai tracked".ai()]);
}

#[test]
fn test_daemon_reports_post_commit_side_effect_error() {
    let repo = TestRepo::new();

    let base_path = repo.path().join("base.txt");
    fs::write(&base_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "base.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let working_log = repo.current_working_logs();
    let mut files = HashMap::new();
    files.insert(
        "broken.txt".to_string(),
        vec![LineAttribution {
            start_line: 1,
            end_line: 1,
            author_id: "missing-snapshot-ai".to_string(),
            overrode: None,
        }],
    );
    working_log
        .write_initial(InitialAttributions {
            files,
            ..InitialAttributions::default()
        })
        .unwrap();

    fs::write(repo.path().join("broken.txt"), "broken\n").unwrap();
    repo.git(&["add", "broken.txt"]).unwrap();
    repo.git(&["commit", "-m", "commit with broken initial"])
        .unwrap();

    let sync = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        repo.sync_daemon_force();
    }));
    assert!(
        sync.is_err(),
        "post-commit side-effect failure must be reported through daemon sync"
    );
}

#[test]
fn test_production_show_does_not_hidden_sync_pending_commit_authorship_side_effect() {
    let fixture = delayed_ai_commit_without_harness_sync_with_delay(5000);

    let output = fixture
        .repo
        .git_ai_without_pre_sync_for_test(&["show", "HEAD"])
        .expect("immediate show should succeed");

    assert!(
        !output.contains("\"tool\":\"claude\"") && !output.contains("\"tool\": \"claude\""),
        "production show performed a hidden daemon sync before rendering:\n{output}"
    );
}

#[test]
fn test_empty_head_reflog_does_not_break_trace2_ref_cursor() {
    let repo = TestRepo::new();

    commit_ai_line(&repo, "base.txt", "base", "initial");
    fs::write(head_reflog(&repo), "").unwrap();
    commit_ai_line(&repo, "next.txt", "next ai", "after empty head reflog");
}

#[test]
fn test_empty_branch_reflog_does_not_break_trace2_ref_cursor() {
    let repo = TestRepo::new();

    commit_ai_line(&repo, "base.txt", "base", "initial");
    fs::write(current_branch_reflog(&repo), "").unwrap();
    commit_ai_line(&repo, "next.txt", "next ai", "after empty branch reflog");
}

#[test]
fn test_partially_pruned_head_reflog_does_not_break_trace2_ref_cursor() {
    let repo = TestRepo::new();

    commit_ai_line(&repo, "base.txt", "base", "initial");
    commit_ai_line(&repo, "advance.txt", "advance", "advance cursor");
    truncate_reflog_to_first_entry(&head_reflog(&repo));
    commit_ai_line(
        &repo,
        "next.txt",
        "next ai",
        "after partially pruned head reflog",
    );
}

#[test]
fn test_partially_pruned_branch_reflog_does_not_break_trace2_ref_cursor() {
    let repo = TestRepo::new();

    commit_ai_line(&repo, "base.txt", "base", "initial");
    commit_ai_line(&repo, "advance.txt", "advance", "advance cursor");
    truncate_reflog_to_first_entry(&current_branch_reflog(&repo));
    commit_ai_line(
        &repo,
        "next.txt",
        "next ai",
        "after partially pruned branch reflog",
    );
}

#[test]
fn test_deleted_head_reflog_does_not_break_trace2_ref_cursor() {
    let repo = TestRepo::new();

    commit_ai_line(&repo, "base.txt", "base", "initial");
    fs::remove_file(head_reflog(&repo)).unwrap();
    commit_ai_line(&repo, "next.txt", "next ai", "after deleted head reflog");
}

#[test]
fn test_deleted_branch_reflog_does_not_break_trace2_ref_cursor() {
    let repo = TestRepo::new();

    commit_ai_line(&repo, "base.txt", "base", "initial");
    fs::remove_file(current_branch_reflog(&repo)).unwrap();
    commit_ai_line(&repo, "next.txt", "next ai", "after deleted branch reflog");
}
