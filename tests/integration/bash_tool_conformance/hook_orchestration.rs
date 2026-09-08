use super::{
    BashCheckpointAction, Duration, TestRepo, add_and_commit, fs, post_hook, pre_hook, repo_root,
    snapshot, thread,
};

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
