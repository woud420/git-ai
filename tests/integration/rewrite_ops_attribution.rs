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

mod checkpoint_ranges;
mod cherry_pick;
mod edit_preservation;
mod file_coverage;
mod rebase_resolution;
mod ref_cursor;
mod reset_checkpoints;
mod revert_attribution;
mod spawn_bounds;
mod squash_attribution;
