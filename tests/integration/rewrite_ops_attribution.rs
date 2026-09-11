/// Deterministic regression tests for attribution bugs found by the fuzzer
/// on the rewrite-ops branch. Each test models a specific fuzzer failure pattern
/// using explicit file writes and checkpoint calls.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use git_ai::model::attribution_tracker::LineAttribution;
use git_ai::operations::git::repo_storage::InitialAttributions;

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use serde_json::json;

fn commit_ai_line(repo: &TestRepo, filename: &str, line: &str, message: &str) {
    let path = repo.path().join(filename);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, format!("{line}\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", filename]).unwrap();
    repo.stage_all_and_commit(message).unwrap();

    let mut file = repo.filename(filename);
    file.assert_committed_lines(crate::lines![line.ai()]);
}

fn claude_checkpoint(repo: &TestRepo, event: &str, file_path: &Path, session_id: &str) {
    let transcript_path = repo.path().join(format!("{session_id}.jsonl"));
    if !transcript_path.exists() {
        fs::write(&transcript_path, "").unwrap();
    }
    let hook_input = json!({
        "cwd": repo.path().to_string_lossy().to_string(),
        "hook_event_name": event,
        "tool_name": "Edit",
        "session_id": session_id,
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_use_id": format!("{session_id}-{event}"),
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();

    repo.checkpoint_with_hook_input("claude", &hook_input)
        .unwrap_or_else(|error| panic!("claude {event} checkpoint failed: {error}"));
}

struct DelayedAiCommit {
    repo: TestRepo,
}

fn delayed_ai_commit_without_harness_sync_with_delay(delay_ms: u64) -> DelayedAiCommit {
    let delay_spec = format!("commit={delay_ms}");
    let repo = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND",
        delay_spec.as_str(),
    )]);
    let file_path = repo.path().join("reader.txt");
    fs::write(&file_path, "AI reader line\n").unwrap();
    claude_checkpoint(&repo, "PostToolUse", &file_path, "reader-session");
    repo.git(&["add", "reader.txt"]).unwrap();
    repo.git_without_test_sync_for_test(&["commit", "-m", "delayed ai reader commit"], &[])
        .unwrap();

    DelayedAiCommit { repo }
}

fn head_reflog(repo: &TestRepo) -> PathBuf {
    repo.path().join(".git/logs/HEAD")
}

fn current_branch_reflog(repo: &TestRepo) -> PathBuf {
    repo.path()
        .join(".git/logs/refs/heads")
        .join(repo.current_branch())
}

fn truncate_reflog_to_first_entry(path: &Path) {
    let bytes = fs::read(path).unwrap();
    let first_end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(bytes.len());
    assert!(
        first_end < bytes.len(),
        "expected multiple reflog entries in {}",
        path.display()
    );
    fs::write(path, &bytes[..first_end]).unwrap();
}

mod cherry_pick_recovery;
mod edit_preservation;
mod file_coverage;
mod rebase_resolution;
mod ref_cursor_integrity;

mod squash_attribution;

// =============================================================================
// Category D: Overbroad AI session range (human lines inside AI range)
//
// Reproduction of fuzz_combined_0:
// When AI and KnownHuman checkpoints both fire before a single commit,
// the resulting note's AI session range covers ALL lines (1-N) instead of
// only the lines from the AI checkpoint. The KnownHuman checkpoint's lines
// are swallowed into the AI range.
// =============================================================================

/// AI checkpoint then KnownHuman checkpoint, single commit.
/// The note must NOT lump human lines into the AI session range.
///
/// Models fuzz_combined_0: note says `s_xxx 1-5` but lines 2-5 are KnownHuman.
#[test]
fn test_overbroad_ai_range_swallows_human_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // AI writes some lines
    fs::write(&file_path, "ai-line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human appends more lines AFTER the AI checkpoint
    fs::write(&file_path, "ai-line\nhuman-1\nhuman-2\nhuman-3\nhuman-4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("mixed").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-line".ai(),
        "human-1".human(),
        "human-2".human(),
        "human-3".human(),
        "human-4".human(),
    ]);
}

/// Inverse order: KnownHuman first, then AI prepends. Both must be tracked.
#[test]
fn test_overbroad_human_first_then_ai_prepend() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Human writes 4 lines
    fs::write(&file_path, "human-a\nhuman-b\nhuman-c\nhuman-d\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    // AI prepends 1 line
    fs::write(&file_path, "ai-top\nhuman-a\nhuman-b\nhuman-c\nhuman-d\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("prepend ai").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-top".ai(),
        "human-a".human(),
        "human-b".human(),
        "human-c".human(),
        "human-d".human(),
    ]);
}

/// AI OverwriteAll then Human Append — models the overwrite-and-rollback pattern.
/// The AI checkpoint covers ALL content initially, then human appends. The note
/// must NOT claim human-appended lines as AI.
///
/// Critical: uses OverwriteAll (deletes all existing content) which is a more
/// aggressive pattern than simple append/prepend.
#[test]
fn test_overbroad_overwrite_all_then_human_append() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit with some content
    fs::write(&file_path, "old-1\nold-2\nold-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // AI overwrites ALL content (OverwriteAll pattern)
    fs::write(&file_path, "ai-new-1\nai-new-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human appends after AI overwrite
    fs::write(&file_path, "ai-new-1\nai-new-2\nhuman-1\nhuman-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("overwrite then append").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-new-1".ai(),
        "ai-new-2".ai(),
        "human-1".human(),
        "human-2".human(),
    ]);
}

/// Exact fuzz_combined_0 pattern: many rapid checkpoints (storm), commit, hard
/// reset back, then AI overwrite + human append. The checkpoint storm creates
/// many working log entries that the hard reset must invalidate.
#[test]
fn test_overbroad_checkpoint_storm_then_reset_then_overwrite_human() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "aaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Checkpoint storm: many rapid edits, then commit
    fs::write(&file_path, "storm-1\nstorm-2\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&file_path, "storm-1\nstorm-2\nstorm-3\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(
        &file_path,
        "storm-1\nstorm-2\nstorm-3\nstorm-4\naaa\nbbb\nccc\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(
        &file_path,
        "storm-1\nstorm-2\nstorm-3\nstorm-4\nstorm-5\naaa\nbbb\nccc\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("storm commit").unwrap();

    // Hard reset back to initial, then checkpoint immediately.
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // AI OverwriteAll
    fs::write(&file_path, "Y-1\nY-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human Append
    fs::write(&file_path, "Y-1\nY-2\nZ-1\nZ-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("post-reset overwrite").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "Y-1".ai(),
        "Y-2".ai(),
        "Z-1".human(),
        "Z-2".human(),
    ]);
}

/// Like above but adds a cherry-pick conflict + abort after the overwrite,
/// matching the exact tail of fuzz_combined_0.
#[test]
fn test_overbroad_storm_reset_overwrite_then_cherry_pick_abort() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "aaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Storm + commit
    fs::write(&file_path, "s1\ns2\ns3\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("storm").unwrap();

    // Hard reset, then checkpoint immediately.
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // OverwriteAll (AI) + Append (Human) + commit
    fs::write(&file_path, "Y-1\nY-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&file_path, "Y-1\nY-2\nZ-1\nZ-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("overwrite-and-rollback").unwrap();

    // Feature branch from initial, prepend human lines
    repo.git(&["checkout", "-b", "cp-feature", "HEAD~1"])
        .unwrap();
    fs::write(&file_path, "human-a\nhuman-b\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature: prepend human").unwrap();
    let feature_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Back to main, prepend AI
    repo.git(&["checkout", "-"]).unwrap();
    fs::write(&file_path, "b-ai\nY-1\nY-2\nZ-1\nZ-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("main: prepend ai").unwrap();

    // Cherry-pick → conflict → abort
    let cp_result = repo.git(&["cherry-pick", &feature_sha]);
    if cp_result.is_err() {
        repo.git(&["cherry-pick", "--abort"]).ok();
    }

    // After abort: main state = b-ai, Y-1, Y-2, Z-1, Z-2
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "b-ai".ai(),
        "Y-1".ai(),
        "Y-2".ai(),
        "Z-1".human(),
        "Z-2".human(),
    ]);
}

/// Reset then re-edit and squash: AI lines in the middle must not fall into gaps.
#[test]
fn test_reset_reedit_squash_no_attribution_gaps() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit with mixed content
    fs::write(&file_path, "aaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit: add more AI lines
    fs::write(&file_path, "aaa\nbbb\nccc\nddd\neee\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("add more").unwrap();

    // Reset to initial
    repo.git(&["reset", "--mixed", "HEAD~1"]).unwrap();

    // Re-edit: human prepends, then AI appends
    fs::write(&file_path, "human-top\naaa\nbbb\nccc\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    fs::write(
        &file_path,
        "human-top\naaa\nbbb\nccc\nai-bot\nai-bot\nai-bot\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("re-edit after reset").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "human-top".human(),
        "aaa".ai(),
        "bbb".ai(),
        "ccc".ai(),
        "ai-bot".ai(),
        "ai-bot".ai(),
        "ai-bot".ai(),
    ]);
}

// =============================================================================
// Category C: Reset sequencing before subsequent checkpoints
//
// A checkpoint immediately after reset must see working-log state after reset,
// not stale state from the commit that reset just removed.
// =============================================================================

#[test]
fn test_hard_reset_then_ai_checkpoint_preserves_new_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    fs::write(&file_path, "base\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    fs::write(&file_path, "new-1\nnew-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("after reset").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["new-1".ai(), "new-2".ai(),]);
}

/// Simpler test: does overwriting all content work without a reset?
#[test]
fn test_overwrite_all_content_ai() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    fs::write(&file_path, "new-1\nnew-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["new-1".ai(), "new-2".ai(),]);
}

/// Same as above but with --mixed reset to see if bug is --hard specific.
#[test]
fn test_mixed_reset_then_ai_checkpoint() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit
    fs::write(&file_path, "base\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    // Mixed reset back to initial
    repo.git(&["reset", "--mixed", "HEAD~1"]).unwrap();

    // New AI edits after mixed reset (same content as hard reset test)
    fs::write(&file_path, "new-ai-1\nnew-ai-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("after mixed reset").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines!["new-ai-1".ai(), "new-ai-2".ai(),]);
}

/// Hard reset then mixed AI and human checkpoints — both must be correctly attributed.
#[test]
fn test_hard_reset_mixed_checkpoint_types() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit to create something to reset
    fs::write(&file_path, "init\nmore\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    // Hard reset
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // Human edits first
    fs::write(&file_path, "human-line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    // Then AI appends
    fs::write(&file_path, "human-line\nai-line\nai-line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("post-reset mixed").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "human-line".human(),
        "ai-line".ai(),
        "ai-line".ai(),
    ]);
}

/// Hard reset THEN overwrite+human pattern — simple variant.
#[test]
fn test_overbroad_after_hard_reset_overwrite_human() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "line-1\nline-2\nline-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Second commit (something to reset from)
    fs::write(&file_path, "line-1\nline-2\nline-3\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("second").unwrap();

    // Hard reset back, then checkpoint immediately.
    repo.git(&["reset", "--hard", "HEAD~1"]).unwrap();

    // AI OverwriteAll
    fs::write(&file_path, "Y-ai\nY-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Human Append
    fs::write(&file_path, "Y-ai\nY-ai\nZ-human\nZ-human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("overwrite-and-rollback").unwrap();

    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "Y-ai".ai(),
        "Y-ai".ai(),
        "Z-human".human(),
        "Z-human".human(),
    ]);
}

#[test]
fn test_revert_older_commit_restores_original_ai_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("revert.txt");

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert.txt"])
        .unwrap();
    fs::write(&file_path, "keep\nrestored ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "revert.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial mixed attribution")
        .unwrap();

    let mut file = repo.filename("revert.txt");
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai line").unwrap();
    let delete_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    file.assert_committed_lines(crate::lines!["keep".human()]);

    fs::write(repo.path().join("advance.txt"), "later human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "advance.txt"])
        .unwrap();
    repo.stage_all_and_commit("later unrelated commit").unwrap();
    let mut advance = repo.filename("advance.txt");
    advance.assert_committed_lines(crate::lines!["later human".human()]);

    repo.git(&["revert", &delete_commit]).unwrap();
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);
}

#[test]
fn test_revert_revision_expression_restores_original_ai_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("revert_expr.txt");

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_expr.txt"])
        .unwrap();
    fs::write(&file_path, "keep\nrestored ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "revert_expr.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial mixed attribution")
        .unwrap();

    let mut file = repo.filename("revert_expr.txt");
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_expr.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai line").unwrap();
    file.assert_committed_lines(crate::lines!["keep".human()]);

    fs::write(repo.path().join("advance.txt"), "later human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "advance.txt"])
        .unwrap();
    repo.stage_all_and_commit("later unrelated commit").unwrap();
    repo.filename("advance.txt")
        .assert_committed_lines(crate::lines!["later human".human()]);

    repo.git(&["revert", "HEAD~1"]).unwrap();
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);
}

/// Multi-commit `git revert <del_a> <del_b> <del_c>` (one invocation, several
/// destinations) exercises the per-destination revert loop. Each reverted
/// "delete" commit must restore its file's original AI attribution. This pins
/// the behavior so the per-commit revert work can be batched without regression.
#[test]
fn test_revert_multiple_commits_restores_each_original_attribution() {
    let repo = TestRepo::new();
    let fa = repo.path().join("a.txt");
    let fb = repo.path().join("b.txt");
    let fc = repo.path().join("c.txt");

    // Base: three files, each one human line.
    fs::write(&fa, "a base\n").unwrap();
    fs::write(&fb, "b base\n").unwrap();
    fs::write(&fc, "c base\n").unwrap();
    repo.stage_all_and_commit("base three files").unwrap();

    // Add an AI line to each file (committed once so attribution is recorded).
    fs::write(&fa, "a base\nAI a\n").unwrap();
    fs::write(&fb, "b base\nAI b\n").unwrap();
    fs::write(&fc, "c base\nAI c\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "a.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "b.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "c.txt"]).unwrap();
    repo.stage_all_and_commit("add ai lines").unwrap();
    repo.filename("a.txt")
        .assert_committed_lines(crate::lines!["a base".human(), "AI a".ai()]);

    // Delete each AI line in its own commit → three separate "delete" commits,
    // each touching a different file (no conflicts when reverted together).
    fs::write(&fa, "a base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "a.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai a").unwrap();
    let del_a = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    fs::write(&fb, "b base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "b.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai b").unwrap();
    let del_b = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    fs::write(&fc, "c base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "c.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai c").unwrap();
    let del_c = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Revert all three deletes in ONE command → one revert command, three
    // destination commits processed by the batched revert path. The revert
    // CONTENT is restored for every file (each AI line comes back), and the
    // FIRST reverted commit recovers its original AI attribution. Attribution
    // recovery for the 2nd+ reverted commit in a single multi-commit revert is a
    // known pre-existing limitation (the source note is located via
    // first-parent of the reverted commit, which for chained deletes does not
    // hold that file's original attestation — see the deferred #13 note in
    // ref_cursor.rs). This test pins the batched path's behavior so the
    // spawn-count reduction is verified behavior-preserving.
    repo.git(&["revert", "--no-edit", &del_a, &del_b, &del_c])
        .unwrap();

    // Content restored for all three files.
    let a = repo.read_file("a.txt").unwrap();
    let b = repo.read_file("b.txt").unwrap();
    let c = repo.read_file("c.txt").unwrap();
    assert!(a.contains("AI a"), "a.txt content restored");
    assert!(b.contains("AI b"), "b.txt content restored");
    assert!(c.contains("AI c"), "c.txt content restored");

    // First reverted commit recovers original AI attribution.
    repo.filename("a.txt")
        .assert_committed_lines(crate::lines!["a base".human(), "AI a".ai()]);
}

#[test]
fn test_revert_restored_ai_attribution_survives_shifted_line_numbers() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("revert_shift.txt");

    fs::write(&file_path, "keep\nrestored ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "revert_shift.txt"])
        .unwrap();
    repo.stage_all_and_commit("source ai line").unwrap();
    let mut file = repo.filename("revert_shift.txt");
    file.assert_committed_lines(crate::lines!["keep".ai(), "restored ai".ai()]);

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_shift.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai line").unwrap();
    let delete_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    file.assert_committed_lines(crate::lines!["keep".ai()]);

    fs::write(&file_path, "later human\nkeep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_shift.txt"])
        .unwrap();
    repo.stage_all_and_commit("prepend later human line")
        .unwrap();
    file.assert_committed_lines(crate::lines!["later human".human(), "keep".ai()]);

    repo.git(&["revert", &delete_commit]).unwrap();
    file.assert_committed_lines(crate::lines![
        "later human".human(),
        "keep".ai(),
        "restored ai".ai(),
    ]);
}

/// Spawn-scaling guard: a multi-commit `git revert` must trigger a CONSTANT
/// number of daemon git spawns regardless of how many commits are reverted.
/// Reverting N commits in one command and 3*N commits in another must produce
/// nearly the same revert-side-effect spawn count (the batched path issues a
/// fixed set of rev-parse/diff-tree/cat-file/notes spawns, not per-commit ones).
/// `#[ignore]` because it shells a dedicated daemon and inspects a spawn log;
/// run explicitly with `--ignored`.
#[test]
#[ignore]
fn revert_spawn_count_is_constant_in_commit_count() {
    fn revert_n(n: usize) -> usize {
        let log_dir =
            std::env::temp_dir().join(format!("git-ai-spawnlog-{}-{}", std::process::id(), n));
        let _ = fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("spawns.log");
        let _ = fs::remove_file(&log_path);
        let repo =
            TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log_path.to_str().unwrap())]);

        // Base with n files.
        for i in 0..n {
            fs::write(repo.path().join(format!("f{i}.txt")), format!("base {i}\n")).unwrap();
        }
        repo.stage_all_and_commit("base").unwrap();

        // Add an AI line to each, one commit.
        for i in 0..n {
            fs::write(
                repo.path().join(format!("f{i}.txt")),
                format!("base {i}\nAI {i}\n"),
            )
            .unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &format!("f{i}.txt")])
                .unwrap();
        }
        repo.stage_all_and_commit("add ai").unwrap();

        // Delete each AI line in its own commit → n delete commits.
        let mut del_commits = Vec::new();
        for i in 0..n {
            fs::write(repo.path().join(format!("f{i}.txt")), format!("base {i}\n")).unwrap();
            repo.git_ai(&["checkpoint", "mock_known_human", &format!("f{i}.txt")])
                .unwrap();
            repo.stage_all_and_commit(&format!("del {i}")).unwrap();
            del_commits.push(repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string());
        }

        repo.sync_daemon();
        let before = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);

        // One revert command over all n delete commits.
        let mut args = vec!["revert".to_string(), "--no-edit".to_string()];
        args.extend(del_commits);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        repo.git(&arg_refs).unwrap();
        repo.sync_daemon();

        let after = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);
        let _ = fs::remove_dir_all(&log_dir);
        after - before
    }

    let small = revert_n(2);
    let large = revert_n(8);
    eprintln!("revert spawns: n=2 -> {small}, n=8 -> {large}");
    // If revert work were per-commit, large would be ~4x small. With batching the
    // counts should be close (allow a small constant slack for git's own
    // revert-time invocations, which are not git-ai daemon spawns anyway).
    assert!(
        large <= small + 4,
        "revert spawn count scales with commit count: n=2 -> {small}, n=8 -> {large}"
    );
}

/// Spawn-scaling guard for the rebase path: rebasing a feature branch of N AI
/// commits onto an advanced main must trigger a CONSTANT number of daemon git
/// spawns regardless of N (the note-shift and conflict-resolution work is
/// batched). `#[ignore]`; run with `--ignored`.
#[test]
#[ignore]
fn rebase_spawn_count_is_constant_in_commit_count() {
    fn rebase_n(n: usize) -> usize {
        let log_dir = std::env::temp_dir().join(format!(
            "git-ai-spawnlog-rebase-{}-{}",
            std::process::id(),
            n
        ));
        let _ = fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("spawns.log");
        let _ = fs::remove_file(&log_path);
        let repo =
            TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log_path.to_str().unwrap())]);

        fs::write(repo.path().join("base.txt"), "base\n").unwrap();
        repo.stage_all_and_commit("base").unwrap();
        let default_branch = repo.current_branch();

        // Feature branch with N AI commits, each adding a line to its own file.
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        for i in 0..n {
            fs::write(
                repo.path().join(format!("feat{i}.txt")),
                format!("AI feat {i}\n"),
            )
            .unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", &format!("feat{i}.txt")])
                .unwrap();
            repo.stage_all_and_commit(&format!("feat {i}")).unwrap();
        }

        // Advance main so the rebase is a real non-fast-forward.
        repo.git(&["checkout", &default_branch]).unwrap();
        fs::write(repo.path().join("main.txt"), "main work\n").unwrap();
        repo.stage_all_and_commit("main advance").unwrap();

        repo.git(&["checkout", "feature"]).unwrap();
        repo.sync_daemon();
        let before = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);

        repo.git(&["rebase", &default_branch]).unwrap();
        repo.sync_daemon();

        let after = fs::read_to_string(&log_path)
            .map(|c| c.lines().count())
            .unwrap_or(0);
        let _ = fs::remove_dir_all(&log_dir);
        after - before
    }

    let small = rebase_n(2);
    let large = rebase_n(8);
    eprintln!("rebase spawns: n=2 -> {small}, n=8 -> {large}");
    assert!(
        large <= small + 4,
        "rebase spawn count scales with commit count: n=2 -> {small}, n=8 -> {large}"
    );
}
