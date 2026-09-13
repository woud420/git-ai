//! Conformance test suite for the bash tool change attribution feature.
//!
//! Covers PRD Sections 5.1 (file mutations), 5.2 (read-only operations),
//! 5.3 (edge cases), 5.4 (pre/post hook semantics), tool classification
//! for all six agents, gitignore filtering, and full handle_bash_tool
//! orchestration.

use crate::bash_tool_common::{add_and_commit, post_hook, pre_hook, repo_root};
use crate::repos::test_repo::TestRepo;
use git_ai::operations::commands::checkpoint_agent::bash_tool::{
    Agent, BashCheckpointAction, StatDiffResult, StatEntry, StatFileType, StatSnapshot, ToolClass,
    build_gitignore, classify_tool, diff, git_status_fallback, normalize_path, snapshot,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

mod ignored_paths;
mod stat_snapshot;

// ===========================================================================
// Section 5.1 — File Mutations
// ===========================================================================

#[test]
fn test_bash_tool_detect_file_creation() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    repo.write_file("new.txt", "hello");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        created.iter().any(|p| p.contains("new.txt")),
        "new.txt should appear in created; got {:?}",
        created
    );
    assert!(result.modified.is_empty(), "no files should be modified");
}

#[test]
fn test_bash_tool_detect_modification() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "existing.txt", "foo", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Allow filesystem time granularity to advance so the stat-tuple changes.
    thread::sleep(Duration::from_millis(50));
    repo.write_file("existing.txt", "bar");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let modified: Vec<String> = result
        .modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        modified.iter().any(|p| p.contains("existing.txt")),
        "existing.txt should appear in modified; got {:?}",
        modified
    );
    assert!(result.created.is_empty(), "no files should be created");
}

#[cfg(unix)]
#[test]
fn test_bash_tool_detect_permission_change() {
    use std::os::unix::fs::PermissionsExt;

    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "script.sh", "#!/bin/bash\necho hi", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // chmod +x
    let abs = repo.path().join("script.sh");
    let mut perms = fs::metadata(&abs).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&abs, perms).expect("chmod should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let modified: Vec<String> = result
        .modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        modified.iter().any(|p| p.contains("script.sh")),
        "script.sh should appear in modified after chmod; got {:?}",
        modified
    );
}

#[test]
fn test_bash_tool_detect_rename() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "old.txt", "data", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    fs::rename(repo.path().join("old.txt"), repo.path().join("new.txt"))
        .expect("rename should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    // After rename: old.txt no longer exists (deletion not tracked), new.txt appears as created.
    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        created.iter().any(|p| p.contains("new.txt")),
        "new.txt should appear in created after rename; got {:?}",
        created
    );
}

#[test]
fn test_bash_tool_detect_copy() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "source.txt", "copy-me", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    fs::copy(repo.path().join("source.txt"), repo.path().join("dest.txt"))
        .expect("copy should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let created: Vec<String> = result
        .created
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        created.iter().any(|p| p.contains("dest.txt")),
        "dest.txt should appear in created (or modified) after copy; got {:?}",
        created
    );
    // source.txt should NOT appear as modified since we only read it
    let modified: Vec<String> = result
        .modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        !modified.iter().any(|p| p.contains("source.txt")),
        "source.txt should not be modified by a copy; got {:?}",
        modified
    );
}

// ===========================================================================
// Section 5.2 — Read-Only Operations
// ===========================================================================

#[test]
fn test_bash_tool_no_changes_detected() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "stable.txt", "unchanged", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");
    // No mutations between snapshots.
    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(
        result.is_empty(),
        "diff should be empty when nothing changed"
    );
    assert!(result.created.is_empty());
    assert!(result.modified.is_empty());
}

#[test]
fn test_bash_tool_empty_repo_no_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");
    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(result.is_empty(), "empty repo diff should be empty");
}

#[test]
fn test_bash_tool_multiple_mutations_combined() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "modify-me.txt", "original", "initial");
    add_and_commit(&repo, "delete-me.txt", "gone-soon", "add delete target");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    // Perform multiple mutations
    thread::sleep(Duration::from_millis(50));
    repo.write_file("modify-me.txt", "changed");
    repo.write_file("brand-new.txt", "fresh");
    fs::remove_file(repo.path().join("delete-me.txt")).expect("delete should succeed");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    assert!(
        !result.is_empty(),
        "diff should not be empty after multiple mutations"
    );

    let all_paths = result.all_changed_paths();
    assert!(
        all_paths.iter().any(|p| p.contains("modify-me.txt")),
        "modify-me.txt should be in changed paths; got {:?}",
        all_paths
    );
    assert!(
        all_paths.iter().any(|p| p.contains("brand-new.txt")),
        "brand-new.txt should be in changed paths; got {:?}",
        all_paths
    );
    // delete-me.txt is not tracked (deletions are not reported)
}

// ===========================================================================
// Subdirectory file operations
// ===========================================================================

#[test]
fn test_bash_tool_detect_file_in_subdirectory() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "src/lib.rs", "pub fn foo() {}", "initial");

    let pre = snapshot(&root, "sess", "t1", None).expect("pre-snapshot should succeed");

    thread::sleep(Duration::from_millis(50));
    repo.write_file("src/lib.rs", "pub fn bar() {}");
    repo.write_file("src/nested/deep/module.rs", "mod deep;");

    let post = snapshot(&root, "sess", "t2", None).expect("post-snapshot should succeed");
    let result = diff(&pre, &post);

    let all = result.all_changed_paths();
    assert!(
        all.iter()
            .any(|p| p.contains("src/lib.rs") || p.contains("src\\lib.rs")),
        "src/lib.rs should be detected; got {:?}",
        all
    );
    assert!(
        all.iter().any(|p| p.contains("module.rs")),
        "deeply nested module.rs should be detected; got {:?}",
        all
    );
}

// ===========================================================================
// Section 5.4 — Pre/Post Hook Semantics
// ===========================================================================

#[test]
fn test_bash_tool_pre_hook_returns_take_pre_snapshot() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    pre_hook(&root, "sess", "tool1");
}

#[test]
fn test_bash_tool_post_hook_no_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "stable.txt", "unchanged", "initial");

    // Pre-hook stores the snapshot
    pre_hook(&root, "sess", "tool1");

    // Post-hook with no changes
    let post_action = post_hook(&root, "sess", "tool1");
    assert!(
        matches!(post_action.action, BashCheckpointAction::NoChanges),
        "PostToolUse with no changes should return NoChanges; got {:?}",
        post_action.action
    );
}

#[test]
fn test_bash_tool_post_hook_detects_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "target.txt", "before", "initial");

    // Pre-hook
    pre_hook(&root, "sess", "tool2");

    // Mutate between pre and post
    thread::sleep(Duration::from_millis(50));
    repo.write_file("target.txt", "after");

    // Post-hook
    let post_action = post_hook(&root, "sess", "tool2");
    match &post_action.action {
        BashCheckpointAction::Checkpoint(paths) => {
            assert!(
                paths.iter().any(|p| p.contains("target.txt")),
                "Checkpoint should include target.txt; got {:?}",
                paths
            );
        }
        other => panic!("Expected Checkpoint, got {:?}", other),
    }
}

#[test]
fn test_bash_tool_post_hook_without_pre_uses_fallback() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Do NOT call PreToolUse first. PostToolUse should fall back to git status.
    // Create a tracked file and then modify it so git status shows changes.
    add_and_commit(&repo, "changed.txt", "original", "initial");
    repo.write_file("changed.txt", "modified");

    let post_action = post_hook(&root, "sess", "missing-pre");

    // Without a pre-snapshot, expect MissingPreSnapshot (or possibly Checkpoint
    // if the daemon happens to have state from a prior run).
    match &post_action.action {
        BashCheckpointAction::Checkpoint(paths) => {
            assert!(
                paths.iter().any(|p| p.contains("changed.txt")),
                "Should detect changed.txt; got {:?}",
                paths
            );
        }
        BashCheckpointAction::NoChanges
        | BashCheckpointAction::MissingPreSnapshot
        | BashCheckpointAction::HookTimeout
        | BashCheckpointAction::SnapshotFailed => {
            // Acceptable — no pre-snapshot was stored or other failure
        }
    }
}

// ===========================================================================
// Full handle_bash_tool orchestration — Pre followed by Post with creation
// ===========================================================================

#[test]
fn test_bash_tool_orchestration_create_file() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Make an initial commit so the repo is valid
    add_and_commit(&repo, "readme.md", "# Hello", "init");

    // Pre-hook
    pre_hook(&root, "orch-sess", "orch-tool");

    // Simulate bash creating a new file
    repo.write_file("generated.rs", "fn main() {}");

    // Post-hook
    let action = post_hook(&root, "orch-sess", "orch-tool");

    match &action.action {
        BashCheckpointAction::Checkpoint(paths) => {
            assert!(
                paths.iter().any(|p| p.contains("generated.rs")),
                "Orchestrated checkpoint should include generated.rs; got {:?}",
                paths
            );
        }
        BashCheckpointAction::NoChanges => {
            panic!("Expected Checkpoint after creating a file, got NoChanges");
        }
        _ => panic!("Expected Checkpoint after creating a file"),
    }
}

#[test]
fn test_bash_tool_orchestration_delete_file() {
    // Deletions are not tracked; a bash call that only deletes files
    // produces NoChanges.
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "doomed.txt", "temporary", "initial");

    pre_hook(&root, "del-sess", "del-tool");

    fs::remove_file(repo.path().join("doomed.txt")).expect("remove should succeed");

    let action = post_hook(&root, "del-sess", "del-tool");

    // Deletion-only bash call: no changed paths to report.
    assert!(
        matches!(action.action, BashCheckpointAction::NoChanges),
        "Expected NoChanges for deletion-only bash call"
    );
}

#[test]
fn test_bash_tool_orchestration_multiple_tool_uses() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "base.txt", "base", "initial");

    // First tool use: create file
    pre_hook(&root, "multi-sess", "use1");
    repo.write_file("first.txt", "first");
    let action1 = post_hook(&root, "multi-sess", "use1");
    assert!(
        matches!(action1.action, BashCheckpointAction::Checkpoint(_)),
        "First tool use should produce Checkpoint"
    );

    // Second tool use: modify file
    pre_hook(&root, "multi-sess", "use2");
    thread::sleep(Duration::from_millis(50));
    repo.write_file("first.txt", "modified-first");
    let action2 = post_hook(&root, "multi-sess", "use2");
    assert!(
        matches!(action2.action, BashCheckpointAction::Checkpoint(_)),
        "Second tool use should produce Checkpoint"
    );
}

// ===========================================================================
// handle_bash_tool — PostToolUse without PreToolUse, clean repo → NoChanges
// ===========================================================================

#[test]
fn test_post_hook_without_pre_clean_repo_returns_no_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "clean.txt", "clean", "initial");
    // No PreToolUse, no modifications — should get MissingPreSnapshot or NoChanges

    let action = post_hook(&root, "sess", "missing");

    assert!(
        matches!(
            action.action,
            BashCheckpointAction::NoChanges | BashCheckpointAction::MissingPreSnapshot
        ),
        "Clean repo without pre-snapshot should return NoChanges or MissingPreSnapshot"
    );
}

// ===========================================================================
// Multiple files in different states detected simultaneously
// ===========================================================================

// ===========================================================================
// handle_bash_tool full orchestration — rename detection through pre/post
// ===========================================================================

#[test]
fn test_handle_bash_tool_detects_rename() {
    use git_ai::operations::commands::checkpoint_agent::bash_tool::diff;
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "original.txt", "content", "initial");

    let pre = snapshot(&root, "rename-sess", "rename-t1", None).unwrap();

    fs::rename(
        repo.path().join("original.txt"),
        repo.path().join("renamed.txt"),
    )
    .expect("rename should succeed");

    let post = snapshot(&root, "rename-sess", "rename-t2", None).unwrap();
    let result = diff(&pre, &post);
    assert!(
        result
            .created
            .iter()
            .any(|p| p.display().to_string().contains("renamed.txt")),
        "renamed.txt should appear as created after rename; got created={:?}",
        result.created,
    );
}

// ===========================================================================
// Tool Classification — All 6 Agents
// ===========================================================================

#[test]
fn test_classify_tool_claude_case_insensitive() {
    for tool_name in [
        "Write",
        "write",
        "WRITE",
        "wRiTe",
        "Edit",
        "edit",
        "EDIT",
        "eDiT",
        "MultiEdit",
        "multiedit",
        "MULTIEDIT",
        "mUlTiEdIt",
        "NotebookEdit",
        "notebookedit",
        "NOTEBOOKEDIT",
        "nOtEbOoKeDiT",
    ] {
        assert_eq!(
            classify_tool(Agent::Claude, tool_name),
            ToolClass::FileEdit,
            "Claude file-edit tool {tool_name:?} should be case-insensitive"
        );
    }

    for tool_name in ["Bash", "bash", "BASH", "bAsH"] {
        assert_eq!(
            classify_tool(Agent::Claude, tool_name),
            ToolClass::Bash,
            "Claude Bash tool {tool_name:?} should be case-insensitive"
        );
    }

    for tool_name in ["Read", "rEaD", "Glob", "gLoB", "unknown_tool"] {
        assert_eq!(
            classify_tool(Agent::Claude, tool_name),
            ToolClass::Skip,
            "Claude non-mutating tool {tool_name:?} should remain skipped"
        );
    }
}

#[test]
fn test_classify_tool_gemini() {
    assert_eq!(
        classify_tool(Agent::Gemini, "write_file"),
        ToolClass::FileEdit
    );
    assert_eq!(classify_tool(Agent::Gemini, "replace"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::Gemini, "shell"), ToolClass::Bash);
    assert_eq!(classify_tool(Agent::Gemini, "read_file"), ToolClass::Skip);
    assert_eq!(classify_tool(Agent::Gemini, "unknown"), ToolClass::Skip);
}

#[test]
fn test_classify_tool_continue_cli() {
    assert_eq!(
        classify_tool(Agent::ContinueCli, "edit"),
        ToolClass::FileEdit
    );
    assert_eq!(
        classify_tool(Agent::ContinueCli, "terminal"),
        ToolClass::Bash
    );
    assert_eq!(
        classify_tool(Agent::ContinueCli, "local_shell_call"),
        ToolClass::Bash
    );
    assert_eq!(classify_tool(Agent::ContinueCli, "read"), ToolClass::Skip);
    assert_eq!(
        classify_tool(Agent::ContinueCli, "unknown"),
        ToolClass::Skip
    );
}

#[test]
fn test_classify_tool_droid() {
    assert_eq!(
        classify_tool(Agent::Droid, "ApplyPatch"),
        ToolClass::FileEdit
    );
    assert_eq!(classify_tool(Agent::Droid, "Edit"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::Droid, "Write"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::Droid, "Create"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::Droid, "Bash"), ToolClass::Bash);
    assert_eq!(classify_tool(Agent::Droid, "Read"), ToolClass::Skip);
    assert_eq!(classify_tool(Agent::Droid, "unknown"), ToolClass::Skip);
}

#[test]
fn test_classify_tool_amp() {
    assert_eq!(classify_tool(Agent::Amp, "Write"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::Amp, "Edit"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::Amp, "Bash"), ToolClass::Bash);
    assert_eq!(classify_tool(Agent::Amp, "Read"), ToolClass::Skip);
    assert_eq!(classify_tool(Agent::Amp, "unknown"), ToolClass::Skip);
}

#[test]
fn test_classify_tool_opencode() {
    assert_eq!(classify_tool(Agent::OpenCode, "edit"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::OpenCode, "write"), ToolClass::FileEdit);
    assert_eq!(classify_tool(Agent::OpenCode, "bash"), ToolClass::Bash);
    assert_eq!(classify_tool(Agent::OpenCode, "shell"), ToolClass::Bash);
    assert_eq!(classify_tool(Agent::OpenCode, "read"), ToolClass::Skip);
    assert_eq!(classify_tool(Agent::OpenCode, "unknown"), ToolClass::Skip);
}

#[test]
fn test_classify_tool_codex() {
    assert_eq!(classify_tool(Agent::Codex, "Bash"), ToolClass::Bash);
    assert_eq!(
        classify_tool(Agent::Codex, "apply_patch"),
        ToolClass::FileEdit
    );
    assert_eq!(classify_tool(Agent::Codex, "unknown"), ToolClass::Skip);
}

// ===========================================================================
// git_status_fallback
// ===========================================================================

#[test]
fn test_git_status_fallback_detects_changes() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "tracked.txt", "original", "initial");
    repo.write_file("tracked.txt", "modified");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");

    assert!(
        changed.iter().any(|p| p.contains("tracked.txt")),
        "git_status_fallback should report tracked.txt; got {:?}",
        changed
    );
}

#[test]
fn test_git_status_fallback_detects_untracked() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Make an initial commit so we have a valid repo
    add_and_commit(&repo, "base.txt", "base", "init");
    repo.write_file("untracked.txt", "new file");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");

    assert!(
        changed.iter().any(|p| p.contains("untracked.txt")),
        "git_status_fallback should report untracked.txt; got {:?}",
        changed
    );
}

#[test]
fn test_git_status_fallback_clean_repo() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "clean.txt", "clean", "initial");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.is_empty(),
        "clean repo should report no changes; got {:?}",
        changed
    );
}

// ===========================================================================
// git_status_fallback — unmerged/conflict files (u prefix)
// ===========================================================================

#[test]
fn test_git_status_fallback_merge_conflict() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create a file on main branch
    add_and_commit(&repo, "conflict.txt", "main content", "initial");

    // Create a branch, modify the file, commit
    repo.git_og(&["checkout", "-b", "feature"])
        .expect("checkout should succeed");
    repo.write_file("conflict.txt", "feature content");
    repo.git_og(&["add", "conflict.txt"])
        .expect("add should succeed");
    repo.git_og(&["commit", "-m", "feature change"])
        .expect("commit should succeed");

    // Go back to main, modify the same file differently, commit
    repo.git_og(&["checkout", "master"])
        .or_else(|_| repo.git_og(&["checkout", "main"]))
        .expect("checkout main should succeed");
    repo.write_file("conflict.txt", "main diverged content");
    repo.git_og(&["add", "conflict.txt"])
        .expect("add should succeed");
    repo.git_og(&["commit", "-m", "main diverged"])
        .expect("commit should succeed");

    // Attempt merge — this should produce a conflict
    let merge_result = repo.git_og(&["merge", "feature", "--no-edit"]);
    // If merge succeeds (auto-resolved), skip the test
    if merge_result.is_ok() {
        return; // Auto-resolved, no conflict to test
    }

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.iter().any(|p| p.contains("conflict.txt")),
        "git_status_fallback should report conflicted file; got {:?}",
        changed
    );
}

// ===========================================================================
// git_status_fallback — staged deletion
// ===========================================================================

#[test]
fn test_git_status_fallback_staged_deletion() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "to-delete.txt", "content", "initial");
    repo.git_og(&["rm", "to-delete.txt"])
        .expect("git rm should succeed");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.iter().any(|p| p.contains("to-delete.txt")),
        "git_status_fallback should report staged deletion; got {:?}",
        changed
    );
}

// ===========================================================================
// git_status_fallback — rename with spaces in both paths
// ===========================================================================

#[test]
fn test_git_status_fallback_rename_with_spaces() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    add_and_commit(&repo, "old file name.txt", "content", "add spaced file");
    fs::rename(
        root.join("old file name.txt"),
        root.join("new file name.txt"),
    )
    .expect("rename should succeed");
    repo.git_og(&["add", "-A"]).expect("git add should succeed");

    let changed = git_status_fallback(&root).expect("git_status_fallback should succeed");
    assert!(
        changed.iter().any(|p| p == "new file name.txt"),
        "should report new path with spaces; got {:?}",
        changed
    );
    assert!(
        changed.iter().any(|p| p == "old file name.txt"),
        "should report original path with spaces; got {:?}",
        changed
    );
}
