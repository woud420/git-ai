use super::{ExpectedLineExt, TestRepo, fs, json};

// ============================================================================
// Checkpoint race condition tests
// ============================================================================

/// Simulates the Windsurf race condition where the IDE fires a KnownHuman
/// checkpoint between the pre-edit (WillEdit) and post-edit (Edited) AI
/// checkpoints. The KnownHuman should be suppressed because the file has
/// a pending AI edit in-flight.
#[test]
fn test_windsurf_known_human_suppressed_during_pending_ai_edit() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("race.txt");

    fs::write(&file_path, "original line\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Step 1: Pre-edit checkpoint via Windsurf preset (pretooluse).
    // This has agent_id + WillEdit path_role, registering pending AI edit state.
    let pre_hook = json!({
        "trajectory_id": "traj-race-001",
        "agent_action_name": "pre_write_code",
        "tool_info": {
            "file_path": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();
    repo.checkpoint_with_hook_input("windsurf", &pre_hook)
        .unwrap();

    // Step 2: Windsurf edits the file.
    fs::write(&file_path, "original line\nAI added line\n").unwrap();

    // Step 3: VS Code extension fires KnownHuman (spurious IDE save event).
    // This should be suppressed because race.txt has a pending AI edit.
    repo.git_ai(&["checkpoint", "mock_known_human", "race.txt"])
        .unwrap();

    // Step 4: Windsurf fires posttooluse (PostFileEdit / AI checkpoint).
    let post_hook = json!({
        "trajectory_id": "traj-race-001",
        "agent_action_name": "post_write_code",
        "tool_info": {
            "file_path": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();
    repo.checkpoint_with_hook_input("windsurf", &post_hook)
        .unwrap();

    // Commit and verify attribution.
    repo.stage_all_and_commit("Windsurf race edit").unwrap();
    let mut file = repo.filename("race.txt");
    file.assert_committed_lines(crate::lines![
        "original line".unattributed_human(),
        "AI added line".ai(),
    ]);
}

/// Verifies that KnownHuman checkpoints still work when there is NO pending
/// AI edit. This ensures the suppression logic doesn't over-suppress.
#[test]
fn test_known_human_not_suppressed_without_pending_ai_edit() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("human_edit.txt");

    fs::write(&file_path, "first line\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Genuine human edit — no pre-edit AI checkpoint was fired.
    fs::write(&file_path, "first line\nhuman added line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "human_edit.txt"])
        .unwrap();

    repo.stage_all_and_commit("Human edit").unwrap();
    let mut file = repo.filename("human_edit.txt");
    file.assert_committed_lines(crate::lines![
        "first line".unattributed_human(),
        "human added line".human(),
    ]);
}

/// Verifies that after the AI post-edit checkpoint completes (clearing pending
/// state), subsequent KnownHuman checkpoints on the same file are no longer
/// suppressed.
#[test]
fn test_known_human_works_after_ai_edit_completes() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("mixed.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Full AI edit cycle via Windsurf preset: pre-edit → edit → post-edit.
    let pre_hook = json!({
        "trajectory_id": "traj-mixed-001",
        "agent_action_name": "pre_write_code",
        "tool_info": {
            "file_path": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();
    repo.checkpoint_with_hook_input("windsurf", &pre_hook)
        .unwrap();
    fs::write(&file_path, "base\nai line\n").unwrap();
    let post_hook = json!({
        "trajectory_id": "traj-mixed-001",
        "agent_action_name": "post_write_code",
        "tool_info": {
            "file_path": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();
    repo.checkpoint_with_hook_input("windsurf", &post_hook)
        .unwrap();

    // Now a genuine human edit after the AI edit completed.
    fs::write(&file_path, "base\nai line\nhuman line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "mixed.txt"])
        .unwrap();

    repo.stage_all_and_commit("Mixed edit").unwrap();
    let mut file = repo.filename("mixed.txt");
    file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "ai line".ai(),
        "human line".human(),
    ]);
}
