# Checkpoint test patterns

Simple test pattern (using all standard helpers):
```rust
#[test]
fn test_using_test_repo() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");
    file.set_contents(lines!["Line 1", "AI line".ai()]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    file.assert_lines_and_blame(lines!["Line 1".human(), "AI line".ai()]);
}
```

For certain test cases, especially where you are focused on specific checkpoint or attribution behavior, do NOT use the `file.set_contents` helper. It uses a specific (and synthetic) known-human/AI checkpoint flow: first it sets file content to the expected known-human values with placeholders for AI lines and calls a known-human checkpoint; then it replaces the placeholders with the AI values and calls the AI checkpoint. That can hide ordering and boundary behavior. In those cases, write the file with standard Rust file utilities and invoke checkpoints explicitly: `mock_known_human` for evidence-backed human edits, the compatibility `human` preset for untracked boundaries, and `mock_ai` for AI edits. The following example uses explicit writes and checkpoints when exact flow and ordering matter:

```rust
#[test]
fn test_using_test_repo_with_custom_checkpoints() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.md");

    let initial = "\
Untracked line
";
    fs::write(&file_path, initial).unwrap();
    // Example of a completely untracked edit where we didn't fire a checkpoint call at all
    repo.stage_all_and_commit("Initial commit").unwrap();
    // Assert after every commit
    let mut file = repo.filename("example.md");
    // ALWAYS use the helper to assert the lines post-commit AND make sure to always assert line-level after EVERY commit for EVERY test you EVER right. This is CRUCIAL.
    file.assert_committed_lines(lines![
        "Untracked line".unattributed_human(), // 'untracked'
    ]);


    let second_edit = "\
Untracked line
Human line
";
    fs::write(&file_path, second_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "example.md"])
        .unwrap();

    // Explicit add call (very useful to test partial staging scenarios)
    repo.git(&["add", "."]).unwrap();
    // Explicit commit
    repo.commit("Second commit").unwrap();
    file.assert_committed_lines(lines![
        "Untracked line".unattributed_human(), // still 'untracked'
        "Human line".human(), // known human
    ]);

    let third_edit = "\
Untracked line
Human line
AI line
";
    fs::write(&file_path, third_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.md"])
        .unwrap();
    // Example of a completely untracked edit where we didn't fire a checkpoint call at all
    repo.stage_all_and_commit("Third commit").unwrap();
    file.assert_committed_lines(lines![
        "Untracked line".unattributed_human(), // 'untracked'
        "Human line".human(), // known human
        "AI line".ai(), // AI line
    ]);

    let fourth_edit = "\
Untracked line
Human line
AI line
Another untracked line
";
    fs::write(&file_path, fourth_edit).unwrap();
    // Mocking an AI agent preset's pre edit checkpoint, which all the AI agent presets do to exclude
    // changes made by something else (impossible to know what) before the AI makes its own edit. We mock
    // that with the compatibility `human` checkpoint, which records an untracked boundary.
    repo.git_ai(&["checkpoint", "human", "example.md"])
        .unwrap();

    let fifth_edit = "\
Untracked line
Human line
AI line
Another untracked line
Another AI line
";
    fs::write(&file_path, fifth_edit).unwrap();
    // Mocking an AI agent preset's post edit checkpoint, which all the AI agent presets do to capture the changes made by the AI.
    // We mock that by calling a 'mock_ai' checkpoint.
    repo.git_ai(&["checkpoint", "mock_ai", "example.md"])
        .unwrap();
    repo.stage_all_and_commit("Fourth commit").unwrap();
    file.assert_committed_lines(lines![
        "Untracked line".unattributed_human(), // 'untracked'
        "Human line".human(), // known human
        "AI line".ai(), // AI line
        "Another untracked line".unattributed_human(), // 'untracked'
        "Another AI line".ai(), // AI line
    ]);
}
```
