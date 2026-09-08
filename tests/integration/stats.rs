use crate::repos::diff_hostility::{
    configure_hostile_diff_settings, configure_repo_external_diff_helper,
};
use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::{extract_json_object, raw_git};
use git_ai::operations::authorship::stats::CommitStats;
use insta::assert_debug_snapshot;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn stats_from_args(repo: &TestRepo, args: &[&str]) -> CommitStats {
    let raw = repo.git_ai(args).expect("git-ai stats should succeed");
    let json = extract_json_object(&raw);
    serde_json::from_str(&json).expect("valid stats json")
}

fn stats_while_restoring_authorship_note(
    repo: &TestRepo,
    commit_sha: &str,
    args: &[&str],
) -> String {
    let note = repo
        .read_authorship_note(commit_sha)
        .expect("commit should start with an authorship note");
    repo.git_og(&["notes", "--ref=ai", "remove", commit_sha])
        .expect("authorship note should be removable");

    let mut command =
        repo.git_ai_command_without_pre_sync_for_test(args, &[("GIT_AI_TEST_FORCE_TTY", "1")]);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stats should start");
    let mut waiting_message = String::new();
    BufReader::new(child.stderr.take().expect("stats stderr should be piped"))
        .read_line(&mut waiting_message)
        .expect("stats should write its waiting indicator");
    assert!(
        waiting_message.contains("Waiting for git-ai to process this commit"),
        "interactive stats should show a waiting indicator, got:\n{waiting_message}"
    );

    repo.git_og(&["notes", "--ref=ai", "add", "-f", "-m", &note, commit_sha])
        .expect("authorship note should be restorable");

    let output = child.wait_with_output().expect("stats should finish");
    assert!(
        output.status.success(),
        "stats failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        waiting_message
    )
}

#[test]
fn test_markdown_stats_deletion_only() {
    use git_ai::operations::authorship::stats::write_stats_to_markdown;
    use std::collections::BTreeMap;

    let stats = CommitStats {
        human_additions: 0,
        unknown_additions: 0,
        ai_additions: 0,
        ai_accepted: 0,

        git_diff_deleted_lines: 5,
        git_diff_added_lines: 0,
        tool_model_breakdown: BTreeMap::new(),
    };

    let markdown = write_stats_to_markdown(&stats);

    assert_debug_snapshot!(markdown);
}

#[test]
fn test_markdown_stats_all_human() {
    use git_ai::operations::authorship::stats::write_stats_to_markdown;
    use std::collections::BTreeMap;

    let stats = CommitStats {
        human_additions: 10,
        unknown_additions: 0,
        ai_additions: 0,
        ai_accepted: 0,

        git_diff_deleted_lines: 0,
        git_diff_added_lines: 10,
        tool_model_breakdown: BTreeMap::new(),
    };

    let markdown = write_stats_to_markdown(&stats);

    assert_debug_snapshot!(markdown);
}

#[test]
fn test_markdown_stats_all_ai() {
    use git_ai::operations::authorship::stats::write_stats_to_markdown;
    use std::collections::BTreeMap;

    let stats = CommitStats {
        human_additions: 0,
        unknown_additions: 0,
        ai_additions: 15,
        ai_accepted: 15,

        git_diff_deleted_lines: 0,
        git_diff_added_lines: 15,
        tool_model_breakdown: BTreeMap::new(),
    };

    let markdown = write_stats_to_markdown(&stats);

    assert_debug_snapshot!(markdown);
}

#[test]
fn test_markdown_stats_mixed() {
    use git_ai::operations::authorship::stats::write_stats_to_markdown;
    use std::collections::BTreeMap;

    let stats = CommitStats {
        human_additions: 10,
        unknown_additions: 0,
        ai_additions: 15,
        ai_accepted: 15,

        git_diff_deleted_lines: 5,
        git_diff_added_lines: 30,
        tool_model_breakdown: BTreeMap::new(),
    };

    let markdown = write_stats_to_markdown(&stats);

    assert_debug_snapshot!(markdown);
}

#[test]
fn test_markdown_stats_no_mixed() {
    use git_ai::operations::authorship::stats::write_stats_to_markdown;
    use std::collections::BTreeMap;

    let stats = CommitStats {
        human_additions: 8,
        unknown_additions: 0,
        ai_additions: 12,
        ai_accepted: 12,

        git_diff_deleted_lines: 0,
        git_diff_added_lines: 20,
        tool_model_breakdown: BTreeMap::new(),
    };

    let markdown = write_stats_to_markdown(&stats);

    assert_debug_snapshot!(markdown);
}

#[test]
fn test_markdown_stats_minimal_human() {
    use git_ai::operations::authorship::stats::write_stats_to_markdown;
    use std::collections::BTreeMap;

    // Test that humans get at least 2 visible blocks if they have more than 1 line
    let stats = CommitStats {
        human_additions: 2,
        unknown_additions: 0,
        ai_additions: 98,
        ai_accepted: 98,

        git_diff_deleted_lines: 0,
        git_diff_added_lines: 100,
        tool_model_breakdown: BTreeMap::new(),
    };

    let markdown = write_stats_to_markdown(&stats);

    assert_debug_snapshot!(markdown);
}

#[test]
fn test_markdown_stats_formatting() {
    use git_ai::operations::authorship::stats::{ToolModelHeadlineStats, write_stats_to_markdown};
    use std::collections::BTreeMap;

    let mut tool_model_breakdown = BTreeMap::new();
    tool_model_breakdown.insert(
        "cursor::claude-3.5-sonnet".to_string(),
        ToolModelHeadlineStats {
            ai_additions: 6,
            ai_accepted: 6,
        },
    );

    let stats = CommitStats {
        human_additions: 5,
        unknown_additions: 0,
        ai_additions: 6,
        ai_accepted: 6,
        git_diff_deleted_lines: 2,
        git_diff_added_lines: 13,
        tool_model_breakdown,
    };

    let markdown = write_stats_to_markdown(&stats);
    println!("{}", markdown);
    assert_debug_snapshot!(markdown);
}

mod authorship_sync;
mod commit_ranges;
mod ignored_files;

crate::reuse_tests_in_worktree!(
    test_markdown_stats_deletion_only,
    test_markdown_stats_all_human,
    test_markdown_stats_all_ai,
    test_markdown_stats_mixed,
    test_markdown_stats_no_mixed,
    test_markdown_stats_minimal_human,
    test_markdown_stats_formatting,
);
