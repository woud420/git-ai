use super::{TestRepo, assert_stats, assert_tool_model, fs, head_stats};

// ---------------------------------------------------------------------------
// Test 1: basic workflow — user creates file, AI adds code, user adds more
// ---------------------------------------------------------------------------
#[test]
fn test_basic_workflow_mixed_authorship() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.py");

    // User creates a file with 1 line
    fs::write(&file_path, "def hello():\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    // AI adds 2 lines
    fs::write(
        &file_path,
        "def hello():\n    print(\"Hello from AI\")\n    return \"AI generated\"\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.py"])
        .unwrap();

    // User adds 2 more lines
    fs::write(
        &file_path,
        "def hello():\n    print(\"Hello from AI\")\n    return \"AI generated\"\n\ndef goodbye():\n    print(\"Goodbye from user\")\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    repo.stage_all_and_commit("Add example.py with mixed authorship")
        .unwrap();

    let blame_output = repo.git_ai(&["blame", "example.py"]).unwrap();
    assert!(
        blame_output.contains("mock_ai"),
        "blame should contain 'mock_ai'"
    );
    assert!(
        blame_output.contains("Test User"),
        "blame should contain 'Test User'"
    );
}

// ---------------------------------------------------------------------------
// Test 2: checkpoint exits successfully
// ---------------------------------------------------------------------------
#[test]
fn test_checkpoint_exits_successfully() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");
    fs::write(&file_path, "# Test file\n").unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
}

// ---------------------------------------------------------------------------
// Test 3: checkpoint mock_ai with file path
// ---------------------------------------------------------------------------
#[test]
fn test_checkpoint_mock_ai_with_file_path() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("ai_file.txt");
    fs::write(&file_path, "AI generated content\n").unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "ai_file.txt"])
        .unwrap();
}

// ---------------------------------------------------------------------------
// Test 4: blame shows correct attribution after commit
// ---------------------------------------------------------------------------
#[test]
fn test_blame_shows_correct_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    fs::write(&file_path, "line1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    fs::write(&file_path, "line1\nline2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    repo.git(&["add", "test.txt"]).unwrap();
    repo.commit("Test commit").unwrap();

    let blame_output = repo.git_ai(&["blame", "test.txt"]).unwrap();
    assert!(blame_output.contains("line1"), "blame should contain line1");
    assert!(blame_output.contains("line2"), "blame should contain line2");
}

// ---------------------------------------------------------------------------
// Test 5: multiple checkpoints in sequence
// ---------------------------------------------------------------------------
#[test]
fn test_multiple_checkpoints_in_sequence() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("multi.txt");

    fs::write(&file_path, "step1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    fs::write(&file_path, "step1\nstep2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "multi.txt"])
        .unwrap();

    fs::write(&file_path, "step1\nstep2\nstep3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    fs::write(&file_path, "step1\nstep2\nstep3\nstep4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "multi.txt"])
        .unwrap();

    repo.git(&["add", "multi.txt"]).unwrap();
    repo.commit("Test multiple checkpoints in sequence")
        .unwrap();

    let stats = head_stats(&repo);
    assert_stats(&stats, 2, 2, 2, 0, 4);
    assert_tool_model(&stats, "mock_ai::unknown", 2, 2);
}

// ---------------------------------------------------------------------------
// Test 6: stats shows AI contribution after commit
// ---------------------------------------------------------------------------
#[test]
fn test_stats_shows_ai_contribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("stats_test.txt");

    fs::write(&file_path, "user line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    fs::write(&file_path, "user line\nAI line 1\nAI line 2\nAI line 3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "stats_test.txt"])
        .unwrap();

    repo.git(&["add", "stats_test.txt"]).unwrap();
    repo.commit("Test stats").unwrap();

    let stats = head_stats(&repo);
    assert_stats(&stats, 1, 3, 3, 0, 4);
    assert_tool_model(&stats, "mock_ai::unknown", 3, 3);
}

// ---------------------------------------------------------------------------
// Test 7: AI deletes lines from file
// ---------------------------------------------------------------------------
#[test]
fn test_ai_deletes_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("code.py");

    let initial = "\
def function1():
    print(\"Keep this\")
    return 1

def function2():
    print(\"AI will delete this\")
    return 2

def function3():
    print(\"Keep this too\")
    return 3
";
    fs::write(&file_path, initial).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    let after_delete = "\
def function1():
    print(\"Keep this\")
    return 1

def function3():
    print(\"Keep this too\")
    return 3
";
    fs::write(&file_path, after_delete).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "code.py"]).unwrap();

    repo.git(&["add", "code.py"]).unwrap();
    repo.commit("AI deleted function2").unwrap();

    let stats = head_stats(&repo);
    assert_stats(&stats, 7, 0, 0, 0, 7);
    // AI only deleted lines — no additions, so tool_model_breakdown may be empty or have 0s
    if let Some(entry) = stats.tool_model_breakdown.get("mock_ai::unknown") {
        assert_eq!(entry.ai_additions, 0);
        assert_eq!(entry.ai_accepted, 0);
    }
}

// ---------------------------------------------------------------------------
// Test 8: human deletes lines from AI-generated code
// ---------------------------------------------------------------------------
#[test]
fn test_human_deletes_ai_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("calculator.py");

    let ai_code = "\
def add(a, b):
    return a + b
def subtract(a, b):
    return a - b
def multiply(a, b):
    return a * b
";
    fs::write(&file_path, ai_code).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "calculator.py"])
        .unwrap();

    let after_human_delete = "\
def add(a, b):
    return a + b
def multiply(a, b):
    return a * b
";
    fs::write(&file_path, after_human_delete).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    repo.git(&["add", "calculator.py"]).unwrap();
    repo.commit("AI added functions, human removed one")
        .unwrap();

    let stats = head_stats(&repo);
    assert_stats(&stats, 0, 4, 4, 0, 4);
    assert_tool_model(&stats, "mock_ai::unknown", 4, 4);
}

// ---------------------------------------------------------------------------
// Test 9: AI generates code with empty lines in between
// ---------------------------------------------------------------------------
#[test]
fn test_ai_code_with_empty_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("app.py");

    fs::write(&file_path, "# My Application\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    let ai_code = "\
# My Application

import os
import sys

def setup():
    print(\"Setting up\")

def main():
    setup()
    print(\"Running main\")

def cleanup():
    print(\"Cleaning up\")

if __name__ == \"__main__\":
    main()
";
    fs::write(&file_path, ai_code).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "app.py"]).unwrap();

    repo.git(&["add", "app.py"]).unwrap();
    repo.commit("AI added code with empty lines").unwrap();

    let stats = head_stats(&repo);
    assert_stats(&stats, 1, 16, 16, 0, 17);
    assert_tool_model(&stats, "mock_ai::unknown", 16, 16);
}

// ---------------------------------------------------------------------------
// Test 10: AI creates a new file from scratch
// ---------------------------------------------------------------------------
#[test]
fn test_ai_creates_new_file() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("new_module.py");

    let ai_code = "\
class DataProcessor:
    def __init__(self):
        self.data = []
    def process(self, item):
        self.data.append(item)
        return item
    def get_results(self):
        return self.data
";
    fs::write(&file_path, ai_code).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "new_module.py"])
        .unwrap();

    repo.git(&["add", "new_module.py"]).unwrap();
    repo.commit("AI created new module").unwrap();

    let stats = head_stats(&repo);
    assert_stats(&stats, 0, 8, 8, 0, 8);
    assert_tool_model(&stats, "mock_ai::unknown", 8, 8);
}
