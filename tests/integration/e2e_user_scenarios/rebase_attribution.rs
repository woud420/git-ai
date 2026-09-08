use super::{TestRepo, assert_stats, assert_tool_model, fs, head_stats};

// ---------------------------------------------------------------------------
// Test 14: AI authorship is preserved after rebase
// ---------------------------------------------------------------------------
#[test]
fn test_rebase_preserves_ai_authorship() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("base.py");
    let default_branch = repo.current_branch();

    // Create initial state on main
    let base_content = "\
# Base module
def base_function():
    return \"base\"
";
    fs::write(&file_path, base_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();
    repo.git(&["add", "base.py"]).unwrap();
    repo.commit("Initial base file").unwrap();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature-branch"]).unwrap();

    // AI creates a file on feature branch
    let feature_path = repo.path().join("feature.py");
    let feature_code = "\
def ai_feature():
    print(\"AI generated feature\")
    return \"feature\"
class AIHelper:
    def __init__(self):
        self.name = \"AI Helper\"
    def help(self):
        return \"AI assistance\"
";
    fs::write(&feature_path, feature_code).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.py"])
        .unwrap();
    repo.git(&["add", "feature.py"]).unwrap();
    repo.commit("AI creates feature module").unwrap();

    let stats_before = head_stats(&repo);
    assert_stats(&stats_before, 0, 8, 8, 0, 8);
    assert_tool_model(&stats_before, "mock_ai::unknown", 8, 8);

    let blame_before = repo.git_ai(&["blame", "feature.py"]).unwrap();
    assert!(blame_before.contains("mock_ai"));

    // Switch back to main and create a new commit
    repo.git(&["checkout", &default_branch]).unwrap();
    fs::write(
        &file_path,
        "\
# Base module
def base_function():
    return \"base\"

def new_base_function():
    return \"new base\"
",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();
    repo.git(&["add", "base.py"]).unwrap();
    repo.commit("Add new function to base").unwrap();

    // Rebase feature branch onto updated main
    repo.git(&["checkout", "feature-branch"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify AI authorship is preserved after rebase
    let stats_after = head_stats(&repo);
    assert_stats(&stats_after, 0, 8, 8, 0, 8);
    assert_tool_model(&stats_after, "mock_ai::unknown", 8, 8);

    assert!(repo.path().join("feature.py").exists());
    let content = fs::read_to_string(repo.path().join("feature.py")).unwrap();
    assert!(content.contains("ai_feature"));
}

// ---------------------------------------------------------------------------
// Test 15: AI attribution preserved after fixing conflict during rebase
// ---------------------------------------------------------------------------
#[test]
fn test_rebase_conflict_resolution_preserves_authorship() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("shared.py");
    let default_branch = repo.current_branch();

    // Create initial file on main
    let initial = "\
def function_one():
    return 1
def function_two():
    return 2
";
    fs::write(&file_path, initial).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();
    repo.git(&["add", "shared.py"]).unwrap();
    repo.commit("Initial shared file").unwrap();

    // Feature branch: AI modifies function_two and adds ai_function
    repo.git(&["checkout", "-b", "feature-ai"]).unwrap();

    let ai_edit = "\
def function_one():
    return 1
def function_two():
    # AI enhanced this function
    result = 2 * 2
    return result
def ai_function():
    print(\"AI added this\")
    return \"ai_data\"
";
    fs::write(&file_path, ai_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "shared.py"])
        .unwrap();
    repo.git(&["add", "shared.py"]).unwrap();
    repo.commit("AI enhances function_two and adds ai_function")
        .unwrap();

    let ai_stats = head_stats(&repo);
    assert_stats(&ai_stats, 0, 6, 6, 1, 6);
    assert_tool_model(&ai_stats, "mock_ai::unknown", 6, 6);

    // Go back to main and make conflicting changes
    repo.git(&["checkout", &default_branch]).unwrap();

    let human_edit = "\
def function_one():
    return 1
def function_two():
    # Human modified this differently
    value = 2 + 2
    return value
def human_function():
    return \"human_data\"
";
    fs::write(&file_path, human_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();
    repo.git(&["add", "shared.py"]).unwrap();
    repo.commit("Human modifies function_two and adds human_function")
        .unwrap();

    let human_stats = head_stats(&repo);
    assert_stats(&human_stats, 5, 0, 0, 1, 5);
    assert!(human_stats.tool_model_breakdown.is_empty());

    // Rebase — will conflict
    repo.git(&["checkout", "feature-ai"]).unwrap();
    let rebase_result = repo.git(&["rebase", &default_branch]);
    assert!(rebase_result.is_err(), "Rebase should conflict");

    // Resolve the conflict
    let resolved = "\
def function_one():
    return 1
def function_two():
    # AI enhanced this function
    result = 2 * 2
    return result
def ai_function():
    print(\"AI added this\")
    return \"ai_data\"
def human_function():
    return \"human_data\"
";
    fs::write(&file_path, resolved).unwrap();
    repo.git(&["add", "shared.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    // Verify AI authorship preserved after conflict resolution
    let stats_after = head_stats(&repo);
    assert_stats(&stats_after, 0, 6, 6, 3, 6);
    assert_tool_model(&stats_after, "mock_ai::unknown", 6, 6);

    let blame_after = repo.git_ai(&["blame", "shared.py"]).unwrap();
    assert!(blame_after.contains("mock_ai"));
    assert!(blame_after.contains("Test User"));

    let content = fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("AI added this"));
    assert!(content.contains("human_data"));
}

// ---------------------------------------------------------------------------
// Test 18: rebase feature branch with mixed authorship onto diverged main
// ---------------------------------------------------------------------------
#[test]
fn test_rebase_mixed_authorship_diverged_main() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("app.py");
    let default_branch = repo.current_branch();

    // Create initial state
    let initial = "\
# Application Module
# This file contains the main application logic

def main():
    print(\"Application starting\")

# Utility functions section
# Add utility functions below

# Data processing section
# Add data processing functions below

# End of file
";
    fs::write(&file_path, initial).unwrap();
    repo.git(&["add", "app.py"]).unwrap();
    repo.commit("Initial application setup").unwrap();

    let common_ancestor = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Human adds function signature
    let human_edit = "\
# Application Module
# This file contains the main application logic

def main():
    print(\"Application starting\")

# Utility functions section
# Add utility functions below

# Data processing section
# Add data processing functions below

def process_data(input_data):
    # Validate input

# End of file
";
    fs::write(&file_path, human_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    // AI adds implementation
    let ai_edit = "\
# Application Module
# This file contains the main application logic

def main():
    print(\"Application starting\")

# Utility functions section
# Add utility functions below

# Data processing section
# Add data processing functions below

def process_data(input_data):
    # Validate input
    if not input_data:
        return None
    result = input_data.upper()
    return result

# End of file
";
    fs::write(&file_path, ai_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "app.py"]).unwrap();

    repo.git(&["add", "app.py"]).unwrap();
    repo.commit("Feature: Add data processing function")
        .unwrap();

    let stats_before = head_stats(&repo);
    assert_stats(&stats_before, 3, 4, 4, 0, 7);
    assert_tool_model(&stats_before, "mock_ai::unknown", 4, 4);

    let blame_before = repo.git_ai(&["blame", "app.py"]).unwrap();
    assert!(blame_before.contains("mock_ai"));
    assert!(blame_before.contains("Test User"));

    // Switch to main and create 3 diverging commits (modifying utility section)
    repo.git(&["checkout", &default_branch]).unwrap();

    // Main commit 1
    let main1 = "\
# Application Module
# This file contains the main application logic
import logging

def main():
    print(\"Application starting\")

# Utility functions section
# Add utility functions below

def get_config():
    return {\"debug\": True}

# Data processing section
# Add data processing functions below

# End of file
";
    fs::write(&file_path, main1).unwrap();
    repo.git(&["add", "app.py"]).unwrap();
    repo.commit("Main: Add logging and get_config utility")
        .unwrap();

    // Main commit 2
    let main2 = "\
# Application Module
# This file contains the main application logic
import logging

def main():
    print(\"Application starting\")

# Utility functions section
# Add utility functions below

def get_config():
    return {\"debug\": True}

def log_message(msg):
    logging.info(msg)

# Data processing section
# Add data processing functions below

# End of file
";
    fs::write(&file_path, main2).unwrap();
    repo.git(&["add", "app.py"]).unwrap();
    repo.commit("Main: Add log_message utility").unwrap();

    // Main commit 3
    let main3 = "\
# Application Module
# This file contains the main application logic
import logging

def main():
    print(\"Application starting\")

# Utility functions section
# Add utility functions below

def get_config():
    return {\"debug\": True}

def log_message(msg):
    logging.info(msg)

def handle_error(err):
    logging.error(f\"Error: {err}\")

# Data processing section
# Add data processing functions below

# End of file
";
    fs::write(&file_path, main3).unwrap();
    repo.git(&["add", "app.py"]).unwrap();
    repo.commit("Main: Add handle_error utility").unwrap();

    let main_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Verify 3 commits ahead
    let commits_ahead = repo
        .git(&[
            "rev-list",
            "--count",
            &format!("{common_ancestor}..{default_branch}"),
        ])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(commits_ahead, "3");

    // Rebase feature onto main
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify stats preserved after rebase
    let stats_after = head_stats(&repo);
    assert_stats(&stats_after, 3, 4, 4, 0, 7);
    assert_tool_model(&stats_after, "mock_ai::unknown", 4, 4);

    let blame_after = repo.git_ai(&["blame", "app.py"]).unwrap();
    assert!(blame_after.contains("mock_ai"));
    assert!(blame_after.contains("Test User"));

    // Verify properly rebased onto main
    let merge_base = repo
        .git(&["merge-base", &default_branch, "feature"])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(merge_base, main_head);

    let ahead = repo
        .git(&["rev-list", "--count", &format!("{default_branch}..feature")])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(ahead, "1");

    // Verify content from both branches present
    let content = fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("process_data"));
    assert!(content.contains("input_data.upper()"));
    assert!(content.contains("import logging"));
    assert!(content.contains("get_config"));
    assert!(content.contains("log_message"));
    assert!(content.contains("handle_error"));
}
