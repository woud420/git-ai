use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::operations::commands::checkpoint_agent::presets::resolve_preset;
use git_ai::operations::mdm::agents::get_all_installers;
use std::fs;

#[test]
fn retired_integrations_leave_native_agents_available() {
    let installers = get_all_installers();
    let ids: Vec<_> = installers.iter().map(|installer| installer.id()).collect();
    for retired in ["vscode", "jetbrains", "visual-studio"] {
        assert!(!ids.contains(&retired), "retired installer: {retired}");
    }
    for native in [
        "claude-code",
        "codex",
        "cline",
        "cursor",
        "github-copilot",
        "amp",
        "opencode",
        "pi",
        "gemini",
        "droid",
        "firebender",
        "windsurf",
    ] {
        assert!(ids.contains(&native), "missing native installer: {native}");
    }
    assert!(resolve_preset("ai_tab").is_err());
    for native in [
        "claude",
        "codex",
        "cline",
        "cursor",
        "github-copilot",
        "amp",
        "opencode",
        "pi",
        "gemini",
        "droid",
        "firebender",
        "windsurf",
        "continue-cli",
        "agent-v1",
        "cursor-background",
    ] {
        assert!(
            resolve_preset(native).is_ok(),
            "missing native preset: {native}"
        );
    }
}

#[test]
fn native_checkpoints_preserve_line_evidence_through_amend() {
    let repo = TestRepo::new();
    let path = repo.path().join("evidence.txt");
    fs::write(&path, "base\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    fs::write(&path, "base\nknown human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "evidence.txt"])
        .unwrap();
    repo.stage_all_and_commit("known human evidence").unwrap();
    let mut file = repo.filename("evidence.txt");
    file.assert_committed_lines(lines!["base".unattributed_human(), "known human".human()]);
    fs::write(&path, "base\nknown human\nuntracked\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "evidence.txt"])
        .unwrap();
    repo.stage_all_and_commit("untracked boundary").unwrap();
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "known human".human(),
        "untracked".unattributed_human()
    ]);
    fs::write(&path, "base\nknown human\nuntracked\nAI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "evidence.txt"])
        .unwrap();
    repo.stage_all_and_commit("mixed evidence").unwrap();
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "known human".human(),
        "untracked".unattributed_human(),
        "AI".ai()
    ]);
    repo.git(&["commit", "--amend", "-m", "rewritten evidence"])
        .unwrap();
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "known human".human(),
        "untracked".unattributed_human(),
        "AI".ai()
    ]);
}
