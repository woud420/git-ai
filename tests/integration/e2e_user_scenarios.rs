use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
#[cfg(not(target_os = "windows"))]
use crate::repos::write_executable_script;
use crate::test_utils::extract_json_object;
use git_ai::operations::authorship::stats::CommitStats;
use std::fs;

fn commit_stats(repo: &TestRepo, args: &[&str]) -> CommitStats {
    let raw = repo.git_ai(args).expect("git-ai stats should succeed");
    let json = extract_json_object(&raw);
    serde_json::from_str(&json).expect("valid stats json")
}

fn head_stats(repo: &TestRepo) -> CommitStats {
    commit_stats(repo, &["stats", "HEAD", "--json"])
}

fn assert_stats(
    stats: &CommitStats,
    human: u32,
    ai: u32,
    ai_accepted: u32,
    deleted: u32,
    added: u32,
) {
    assert_eq!(
        stats.human_additions, human,
        "human_additions: expected {human}, got {}",
        stats.human_additions
    );
    assert_eq!(
        stats.ai_additions, ai,
        "ai_additions: expected {ai}, got {}",
        stats.ai_additions
    );
    assert_eq!(
        stats.ai_accepted, ai_accepted,
        "ai_accepted: expected {ai_accepted}, got {}",
        stats.ai_accepted
    );
    assert_eq!(
        stats.git_diff_deleted_lines, deleted,
        "git_diff_deleted_lines: expected {deleted}, got {}",
        stats.git_diff_deleted_lines
    );
    assert_eq!(
        stats.git_diff_added_lines, added,
        "git_diff_added_lines: expected {added}, got {}",
        stats.git_diff_added_lines
    );
}

fn assert_tool_model(stats: &CommitStats, key: &str, ai_additions: u32, ai_accepted: u32) {
    let entry = stats
        .tool_model_breakdown
        .get(key)
        .unwrap_or_else(|| panic!("tool_model_breakdown missing key '{key}'"));
    assert_eq!(
        entry.ai_additions, ai_additions,
        "tool_model_breakdown[{key}].ai_additions: expected {ai_additions}, got {}",
        entry.ai_additions
    );
    assert_eq!(
        entry.ai_accepted, ai_accepted,
        "tool_model_breakdown[{key}].ai_accepted: expected {ai_accepted}, got {}",
        entry.ai_accepted
    );
}

mod basic_workflow;
mod rebase_attribution;
mod squash_authorship;
mod stats_and_reformat;
