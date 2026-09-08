use super::{
    Duration, ExpectedLineExt, ParsedHookEvent, TestRepo, fs, json, parse_windsurf, thread,
};

// ============================================================================
// run_command (bash) hook tests
// ============================================================================

#[test]
fn test_windsurf_preset_pre_run_command_captures_bash_snapshot() {
    let repo = TestRepo::new();
    let repo_root = repo.canonical_path();

    let hook_input = json!({
        "trajectory_id": "traj-bash-pre",
        "execution_id": "exec-bash-1",
        "agent_action_name": "pre_run_command",
        "model_name": "GPT 4.1",
        "tool_info": {
            "command_line": "git status --short",
            "cwd": repo_root.to_string_lossy().to_string(),
        }
    })
    .to_string();

    let events = parse_windsurf(&hook_input).expect("pre_run_command should run");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "windsurf");
            assert_eq!(e.context.agent_id.id, "traj-bash-pre");
            assert_eq!(e.context.agent_id.model, "GPT 4.1");
            assert_eq!(e.tool_use_id, "exec-bash-1");
        }
        _ => panic!("Expected PreBashCall for pre_run_command"),
    }
}

#[test]
fn test_windsurf_preset_post_run_command_detects_changed_files() {
    let repo = TestRepo::new();
    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("src").join("main.rs");
    fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    fs::write(&file_path, "fn main() {}\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Pre-run command via CLI (need snapshot captured first)
    let pre_hook_input = json!({
        "trajectory_id": "traj-bash-post",
        "execution_id": "exec-bash-2",
        "agent_action_name": "pre_run_command",
        "tool_info": {
            "command_line": "echo changed >> src/main.rs",
            "cwd": repo_root.to_string_lossy().to_string(),
        }
    })
    .to_string();

    repo.checkpoint_with_hook_input("windsurf", &pre_hook_input)
        .unwrap();

    thread::sleep(Duration::from_millis(50));
    fs::write(&file_path, "fn main() { println!(\"hi\"); }\n").unwrap();

    let post_hook_input = json!({
        "trajectory_id": "traj-bash-post",
        "execution_id": "exec-bash-2",
        "agent_action_name": "post_run_command",
        "tool_info": {
            "command_line": "echo changed >> src/main.rs",
            "cwd": repo_root.to_string_lossy().to_string(),
        }
    })
    .to_string();

    // Post-run also via CLI since the bash tool state is in the repo
    repo.checkpoint_with_hook_input("windsurf", &post_hook_input)
        .unwrap();

    // Verify that files were changed (commit and check attribution)
    let commit = repo.stage_all_and_commit("Post run command edit").unwrap();
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "post_run_command should produce AI attestations"
    );
}

#[test]
fn test_windsurf_preset_post_run_command_without_snapshot_falls_back_gracefully() {
    let repo = TestRepo::new();
    let repo_root = repo.canonical_path();

    // No pre_run_command hook fired -- snapshot is missing.
    let hook_input = json!({
        "trajectory_id": "traj-orphan-post",
        "execution_id": "exec-orphan",
        "agent_action_name": "post_run_command",
        "tool_info": {
            "command_line": "pwd",
            "cwd": repo_root.to_string_lossy().to_string(),
        }
    })
    .to_string();

    // Use CLI to ensure it doesn't error
    let result = repo.checkpoint_with_hook_input("windsurf", &hook_input);
    assert!(
        result.is_ok(),
        "orphan post_run_command should not error: {:?}",
        result.err()
    );
}

#[test]
fn test_windsurf_e2e_run_command_attribution() {
    let repo = TestRepo::new();
    let repo_root = repo.canonical_path();

    let file_path = repo_root.join("index.ts");
    fs::write(&file_path, "const x = 1;\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let pre_hook = json!({
        "trajectory_id": "traj-e2e-bash",
        "execution_id": "exec-e2e-1",
        "agent_action_name": "pre_run_command",
        "tool_info": {
            "command_line": "sed -i '' 's/1;/2;/' index.ts",
            "cwd": repo_root.to_string_lossy().to_string(),
        }
    })
    .to_string();
    repo.checkpoint_with_hook_input("windsurf", &pre_hook)
        .unwrap();

    thread::sleep(Duration::from_millis(50));
    fs::write(&file_path, "const x = 2;\n").unwrap();

    let post_hook = json!({
        "trajectory_id": "traj-e2e-bash",
        "execution_id": "exec-e2e-1",
        "agent_action_name": "post_run_command",
        "tool_info": {
            "command_line": "sed -i '' 's/1;/2;/' index.ts",
            "cwd": repo_root.to_string_lossy().to_string(),
        }
    })
    .to_string();
    repo.checkpoint_with_hook_input("windsurf", &post_hook)
        .unwrap();

    let commit = repo.stage_all_and_commit("Windsurf bash edit").unwrap();

    let mut file = repo.filename("index.ts");
    file.assert_lines_and_blame(crate::lines!["const x = 2;".ai()]);

    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "run_command edits should produce AI attestations"
    );
}
