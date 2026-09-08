use super::{Agent, ToolClass, classify_tool};

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
