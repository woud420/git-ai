use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_checkpoint_then_stage_then_checkpoint_across_two_commits_preserves_ai_lines() {
    // Exact reproduction from bug report.
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.txt");

    fs::write(&file_path, "test\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();

    repo.git(&["add", "."]).unwrap();

    fs::write(&file_path, "test\ntest1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();

    let first_commit = repo.commit("test").unwrap();
    assert!(
        !first_commit.authorship_log.attestations.is_empty(),
        "first commit should include AI attribution for line 1"
    );

    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(lines!["test".ai()]);

    repo.git(&["add", "."]).unwrap();
    let second_commit = repo.commit("test1").unwrap();
    assert!(
        !second_commit.authorship_log.attestations.is_empty(),
        "second commit should include AI attribution for line 2"
    );

    file.assert_lines_and_blame(lines!["test".ai(), "test1".ai()]);
}

#[test]
fn test_checkpoint_stage_checkpoint_with_non_adjacent_hunks_preserves_second_hunk_ai() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.md");

    let initial = "\
# Notes
intro line

**Section Alpha**
alpha 1
alpha 2
alpha 3

middle context
another context
yet another context

**Section Omega**
omega 1
omega 2
omega 3
";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let first_ai_hunk_only = "\
# Notes
intro line

### Section Alpha
alpha 1
alpha 2
alpha 3

middle context
another context
yet another context

**Section Omega**
omega 1
omega 2
omega 3
";
    fs::write(&file_path, first_ai_hunk_only).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.md"])
        .unwrap();

    repo.git(&["add", "."]).unwrap();

    let both_ai_hunks = "\
# Notes
intro line

### Section Alpha
alpha 1
alpha 2
alpha 3

middle context
another context
yet another context

### Section Omega
omega 1
omega 2
omega 3
";
    fs::write(&file_path, both_ai_hunks).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.md"])
        .unwrap();

    let first_commit = repo.commit("Commit first staged hunk").unwrap();
    assert!(
        !first_commit.authorship_log.attestations.is_empty(),
        "first commit should include AI attribution for the first hunk"
    );

    let mut file = repo.filename("example.md");
    file.assert_committed_lines(lines![
        "# Notes".human(),
        "intro line".human(),
        "".human(),
        "### Section Alpha".ai(),
        "alpha 1".human(),
        "alpha 2".human(),
        "alpha 3".human(),
        "".human(),
        "middle context".human(),
        "another context".human(),
        "yet another context".human(),
        "".human(),
        "omega 1".human(),
        "omega 2".human(),
        "omega 3".human(),
    ]);

    repo.git(&["add", "."]).unwrap();
    let second_commit = repo.commit("Commit second unstaged hunk").unwrap();
    assert!(
        !second_commit.authorship_log.attestations.is_empty(),
        "second commit should include AI attribution for the second hunk"
    );

    file.assert_lines_and_blame(lines![
        "# Notes".human(),
        "intro line".human(),
        "".human(),
        "### Section Alpha".ai(),
        "alpha 1".human(),
        "alpha 2".human(),
        "alpha 3".human(),
        "".human(),
        "middle context".human(),
        "another context".human(),
        "yet another context".human(),
        "".human(),
        "### Section Omega".ai(),
        "omega 1".human(),
        "omega 2".human(),
        "omega 3".human(),
    ]);
}

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
        "Human line".human(),                  // known human
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
        "Human line".human(),                  // known human
        "AI line".ai(),                        // AI line
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
    // that by calling a 'legacy human' (untracked) checkpoint.
    repo.git_ai(&["checkpoint", "human", "example.md"]).unwrap();

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
        "Human line".human(),                  // known human
        "AI line".ai(),                        // AI line
        "Another untracked line".ai(),         // recovered edge line
        "Another AI line".ai(),                // AI line
    ]);
}

#[test]
fn test_ai_heading_checkpoint_then_human_top_commit_then_rest_preserves_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("aidanwashere.md");

    let initial = "\
> \"First, solve the problem. Then, write the code.\"
> \"It works on my machine.\"

*Verse 1:*
Aidan was here, left his mark on the page,
Writing code through the night, line by line, stage by stage.

*Chorus:*
Oh, Aidan was here, yeah, Aidan was here,
The git log will show it, the history's clear.

*Verse 2:*
From branches to merges, through conflicts and fear,
One thing is certain - Aidan was here.
";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("Initial markdown").unwrap();

    let ai_rewrites = "\
> \"First, solve the problem. Then, write the code.\"
> \"It works on my machine.\"

### Verse 1
Aidan was here, left his mark on the page,
Writing code through the night, line by line, stage by stage.

### Chorus
Oh, Aidan was here, yeah, Aidan was here,
The git log will show it, the history's clear.

### Verse 2
From branches to merges, through conflicts and fear,
One thing is certain - Aidan was here.
";
    fs::write(&file_path, ai_rewrites).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "aidanwashere.md"])
        .unwrap();

    let with_human_top = "\
Human preface 1
Human preface 2

> \"First, solve the problem. Then, write the code.\"
> \"It works on my machine.\"

### Verse 1
Aidan was here, left his mark on the page,
Writing code through the night, line by line, stage by stage.

### Chorus
Oh, Aidan was here, yeah, Aidan was here,
The git log will show it, the history's clear.

### Verse 2
From branches to merges, through conflicts and fear,
One thing is certain - Aidan was here.
";
    fs::write(&file_path, with_human_top).unwrap();
    // Intentionally no checkpoint for this human top edit.

    let patch_path = repo.path().join(".git").join("stage_human_top_only.patch");
    let top_hunk_patch = "\
diff --git a/aidanwashere.md b/aidanwashere.md
--- a/aidanwashere.md
+++ b/aidanwashere.md
@@ -0,0 +1,3 @@
+Human preface 1
+Human preface 2
+
";
    fs::write(&patch_path, top_hunk_patch).unwrap();
    repo.git(&[
        "apply",
        "--cached",
        "--unidiff-zero",
        patch_path.to_str().unwrap(),
    ])
    .unwrap();

    let first_commit = repo.commit("Commit human top section").unwrap();
    assert_eq!(
        first_commit.authorship_log.attestations.len(),
        0,
        "first commit should only contain human top insertion"
    );

    repo.git(&["add", "."]).unwrap();
    let second_commit = repo.commit("Commit remaining heading rewrites").unwrap();
    assert!(
        !second_commit.authorship_log.attestations.is_empty(),
        "second commit should contain AI heading rewrite attributions"
    );

    let mut file = repo.filename("aidanwashere.md");
    file.assert_lines_and_blame(lines![
        "Human preface 1".human(),
        "Human preface 2".human(),
        "".human(),
        "> \"First, solve the problem. Then, write the code.\"".human(),
        "> \"It works on my machine.\"".human(),
        "".human(),
        "### Verse 1".ai(),
        "Aidan was here, left his mark on the page,".human(),
        "Writing code through the night, line by line, stage by stage.".human(),
        "".human(),
        "### Chorus".ai(),
        "Oh, Aidan was here, yeah, Aidan was here,".human(),
        "The git log will show it, the history's clear.".human(),
        "".human(),
        "### Verse 2".ai(),
        "From branches to merges, through conflicts and fear,".human(),
        "One thing is certain - Aidan was here.".human(),
    ]);
}

#[test]
fn test_mock_ai_with_pathspecs() {
    let repo = TestRepo::new();
    let mut file1 = repo.filename("file1.txt");
    let mut file2 = repo.filename("file2.txt");

    // Create initial state
    file1.set_contents(crate::lines!["File1 Line 1", "File1 Line 2"]);
    file2.set_contents(crate::lines!["File2 Line 1", "File2 Line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Make changes to both files
    file1.insert_at(2, crate::lines!["File1 AI Line".ai()]);
    file2.insert_at(2, crate::lines!["File2 Human Line"]);

    // Use mock_ai with pathspec to only checkpoint file1.txt
    repo.git_ai(&["checkpoint", "mock_ai", "file1.txt"])
        .unwrap();

    // Commit the changes
    repo.stage_all_and_commit("Second commit").unwrap();

    // file1 should have AI attribution for the new line
    file1.assert_lines_and_blame(crate::lines![
        "File1 Line 1".human(),
        "File1 Line 2".ai(),
        "File1 AI Line".ai(),
    ]);

    // file2 should be all human since we didn't checkpoint it with mock_ai
    file2.assert_lines_and_blame(crate::lines![
        "File2 Line 1".human(),
        "File2 Line 2".human(),
        "File2 Human Line".human(),
    ]);
}
