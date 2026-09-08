use crate::repos::test_file::ExpectedLineExt;

use crate::test_utils::{fixture_path, raw_git, run_git_stdout};
use git_ai::model::attribution_tracker::LineAttribution;
use git_ai::model::authorship_log::PromptRecord;
use git_ai::model::working_log::{AgentId, CheckpointKind};
use git_ai::operations::authorship::stats::CommitStats;
use git_ai::operations::git::repository as GitAiRepository;
use insta::assert_debug_snapshot;
use rand::RngExt;
use regex::Regex;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

fn stats_from_args(repo: &crate::repos::test_repo::TestRepo, args: &[&str]) -> CommitStats {
    let raw = repo.git_ai(args).expect("git-ai stats should succeed");
    let start = raw.find('{').unwrap_or(0);
    let end = raw.rfind('}').unwrap_or(raw.len().saturating_sub(1));
    serde_json::from_str(&raw[start..=end]).expect("valid stats json")
}

fn expected_worktree_storage_prefix(main_repo_root: &Path) -> PathBuf {
    let git_common_dir = PathBuf::from(run_git_stdout(
        main_repo_root,
        &["rev-parse", "--git-common-dir"],
    ));
    let git_common_dir = if git_common_dir.is_relative() {
        main_repo_root.join(git_common_dir)
    } else {
        git_common_dir
    };
    git_common_dir.join("ai").join("worktrees")
}

fn canonicalize_for_assert(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn normalize_blame_output(blame_output: &str) -> String {
    let re_sha = Regex::new(r"[0-9a-f]{40}|[0-9a-f]{7,}").expect("valid sha regex");
    let result = re_sha.replace_all(blame_output, "COMMIT_SHA");
    let re_timestamp = Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} [\+\-]\d{4}")
        .expect("valid timestamp regex");
    let result = re_timestamp.replace_all(&result, "TIMESTAMP");
    let re_author = Regex::new(r"\(([^)]+?)\s+TIMESTAMP").expect("valid author regex");
    re_author
        .replace_all(&result, "(AUTHOR TIMESTAMP")
        .to_string()
}

fn normalize_blame_for_format_parity(blame_output: &str) -> String {
    blame_output
        .lines()
        .map(|line| {
            if let Some(start_paren) = line.find('(')
                && let Some(end_paren) = line.rfind(')')
            {
                let prefix = &line[..start_paren];
                let suffix = &line[end_paren + 1..];
                return format!("{prefix}(META){suffix}");
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unique_worktree_path() -> PathBuf {
    let mut rng = rand::rng();
    let n: u64 = rng.random_range(0..10_000_000_000);
    std::env::temp_dir().join(format!("git-ai-worktree-{}", n))
}

crate::worktree_test_wrappers! {
    fn worktree_initial_attributions_snapshot() {
        let repo = TestRepo::new();

        let mut readme = repo.filename("README.md");
        readme.set_contents(crate::lines!["# Test Repo"]);
        repo.stage_all_and_commit("initial commit").unwrap();

        let working_log = repo.current_working_logs();
        let mut initial_attributions = HashMap::new();
        initial_attributions.insert(
            "initial.txt".to_string(),
            vec![LineAttribution {
                start_line: 1,
                end_line: 2,
                author_id: "initial-ai-1".to_string(),
                overrode: None,
            }],
        );
        let mut prompts = HashMap::new();
        prompts.insert(
            "initial-ai-1".to_string(),
            PromptRecord {
                agent_id: AgentId {
                    tool: "test-tool".to_string(),
                    id: "session-1".to_string(),
                    model: "test-model".to_string(),
                },
                human_author: None,
                total_additions: 0,
                total_deletions: 0,
                accepted_lines: 0,
                overriden_lines: 0,
                custom_attributes: None,
            messages_url: None,
            },
        );
        let file_content = "a\nb\n";
        let mut initial_contents = HashMap::new();
        initial_contents.insert("initial.txt".to_string(), file_content.to_string());
        working_log
            .write_initial_attributions_with_contents(
                initial_attributions,
                prompts,
                BTreeMap::new(),
                initial_contents,
                BTreeMap::new(),
            )
            .expect("write initial attributions");

        fs::write(repo.path().join("initial.txt"), file_content).expect("write file");
        repo.git_ai(&["checkpoint"]).unwrap();
        repo.stage_all_and_commit("commit initial attribution")
            .unwrap();

        let blame_output = repo.git_ai(&["blame", "initial.txt"]).unwrap();
        let normalized = normalize_blame_output(&blame_output);
        assert_debug_snapshot!(normalized);
    }
}

crate::worktree_test_wrappers! {
    fn worktree_stats_snapshot() {
        let repo = TestRepo::new();
        let mut file = repo.filename("stats.txt");
        file.set_contents(crate::lines!["one".human(), "two".ai(), "three".ai()]);
        repo.stage_all_and_commit("stats seed").unwrap();

        let stats = repo.stats().expect("stats should succeed");
        assert_eq!(stats.unknown_additions, 0);
        assert_eq!(stats.human_additions + stats.ai_additions, 3);
        assert_eq!(stats.git_diff_added_lines, 3);
        assert_eq!(stats.git_diff_deleted_lines, 0);
    }
}

// ── Linked-worktree checkpoint routing ──────────────────────────────────────
//
// These tests reproduce the bug where an agent whose CWD is the *main* repo
// writes a file into a *linked* worktree (created with `git worktree add`).
// Before the fix, git-ai would fail to store any checkpoint because:
//   1. It opened the main repo (via CWD), and
//   2. `git status <file>` from the main repo returns nothing for files that
//      live inside a linked worktree's working tree.
// After the fix, git-ai detects that the edited file is outside the main
// repo's boundary and falls back to per-file repository discovery, routing
// the checkpoint to the linked worktree's isolated storage.

/// Helper: simulate a PostToolUse (AiAgent) Claude Code hook call where the
/// session CWD is `session_cwd` but the file being written lives in a
/// *different* directory (`file_path`).  Returns the git-ai stdout+stderr.
fn simulate_claude_post_tool_use(
    repo: &crate::repos::test_repo::TestRepo,
    session_cwd: &Path,
    file_path: &Path,
) -> Result<String, String> {
    let transcript = fixture_path("example-claude-code.jsonl");
    let hook_input = json!({
        "cwd": session_cwd.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        },
        "transcript_path": transcript.to_string_lossy().to_string()
    })
    .to_string();

    repo.git_ai_with_stdin(
        &["checkpoint", "claude", "--hook-input", "stdin"],
        hook_input.as_bytes(),
    )
}

/// Helper: simulate a PreToolUse (Human) Claude Code hook call.
fn simulate_claude_pre_tool_use(
    repo: &crate::repos::test_repo::TestRepo,
    session_cwd: &Path,
    file_path: &Path,
) -> Result<String, String> {
    let transcript = fixture_path("example-claude-code.jsonl");
    let hook_input = json!({
        "cwd": session_cwd.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        },
        "transcript_path": transcript.to_string_lossy().to_string()
    })
    .to_string();

    repo.git_ai_with_stdin(
        &["checkpoint", "claude", "--hook-input", "stdin"],
        hook_input.as_bytes(),
    )
}

mod checkpoint_routing;
mod storage_and_commands;
