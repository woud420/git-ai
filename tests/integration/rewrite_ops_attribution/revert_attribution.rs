use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_revert_older_commit_restores_original_ai_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("revert.txt");

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert.txt"])
        .unwrap();
    fs::write(&file_path, "keep\nrestored ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "revert.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial mixed attribution")
        .unwrap();

    let mut file = repo.filename("revert.txt");
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai line").unwrap();
    let delete_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    file.assert_committed_lines(crate::lines!["keep".human()]);

    fs::write(repo.path().join("advance.txt"), "later human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "advance.txt"])
        .unwrap();
    repo.stage_all_and_commit("later unrelated commit").unwrap();
    let mut advance = repo.filename("advance.txt");
    advance.assert_committed_lines(crate::lines!["later human".human()]);

    repo.git(&["revert", &delete_commit]).unwrap();
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);
}

#[test]
fn test_revert_revision_expression_restores_original_ai_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("revert_expr.txt");

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_expr.txt"])
        .unwrap();
    fs::write(&file_path, "keep\nrestored ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "revert_expr.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial mixed attribution")
        .unwrap();

    let mut file = repo.filename("revert_expr.txt");
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_expr.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai line").unwrap();
    file.assert_committed_lines(crate::lines!["keep".human()]);

    fs::write(repo.path().join("advance.txt"), "later human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "advance.txt"])
        .unwrap();
    repo.stage_all_and_commit("later unrelated commit").unwrap();
    repo.filename("advance.txt")
        .assert_committed_lines(crate::lines!["later human".human()]);

    repo.git(&["revert", "HEAD~1"]).unwrap();
    file.assert_committed_lines(crate::lines!["keep".human(), "restored ai".ai()]);
}

/// Multi-commit `git revert <del_a> <del_b> <del_c>` (one invocation, several
/// destinations) exercises the per-destination revert loop. Each reverted
/// "delete" commit must restore its file's original AI attribution. This pins
/// the behavior so the per-commit revert work can be batched without regression.
#[test]
fn test_revert_multiple_commits_restores_each_original_attribution() {
    let repo = TestRepo::new();
    let fa = repo.path().join("a.txt");
    let fb = repo.path().join("b.txt");
    let fc = repo.path().join("c.txt");

    // Base: three files, each one human line.
    fs::write(&fa, "a base\n").unwrap();
    fs::write(&fb, "b base\n").unwrap();
    fs::write(&fc, "c base\n").unwrap();
    repo.stage_all_and_commit("base three files").unwrap();

    // Add an AI line to each file (committed once so attribution is recorded).
    fs::write(&fa, "a base\nAI a\n").unwrap();
    fs::write(&fb, "b base\nAI b\n").unwrap();
    fs::write(&fc, "c base\nAI c\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "a.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "b.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "c.txt"]).unwrap();
    repo.stage_all_and_commit("add ai lines").unwrap();
    repo.filename("a.txt")
        .assert_committed_lines(crate::lines!["a base".human(), "AI a".ai()]);

    // Delete each AI line in its own commit → three separate "delete" commits,
    // each touching a different file (no conflicts when reverted together).
    fs::write(&fa, "a base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "a.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai a").unwrap();
    let del_a = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    fs::write(&fb, "b base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "b.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai b").unwrap();
    let del_b = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    fs::write(&fc, "c base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "c.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai c").unwrap();
    let del_c = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Revert all three deletes in ONE command → one revert command, three
    // destination commits processed by the batched revert path. The revert
    // CONTENT is restored for every file (each AI line comes back), and the
    // FIRST reverted commit recovers its original AI attribution. Attribution
    // recovery for the 2nd+ reverted commit in a single multi-commit revert is a
    // known pre-existing limitation (the source note is located via
    // first-parent of the reverted commit, which for chained deletes does not
    // hold that file's original attestation — see the deferred #13 note in
    // ref_cursor.rs). This test pins the batched path's behavior so the
    // spawn-count reduction is verified behavior-preserving.
    repo.git(&["revert", "--no-edit", &del_a, &del_b, &del_c])
        .unwrap();

    // Content restored for all three files.
    let a = repo.read_file("a.txt").unwrap();
    let b = repo.read_file("b.txt").unwrap();
    let c = repo.read_file("c.txt").unwrap();
    assert!(a.contains("AI a"), "a.txt content restored");
    assert!(b.contains("AI b"), "b.txt content restored");
    assert!(c.contains("AI c"), "c.txt content restored");

    // First reverted commit recovers original AI attribution.
    repo.filename("a.txt")
        .assert_committed_lines(crate::lines!["a base".human(), "AI a".ai()]);
}

#[test]
fn test_revert_restored_ai_attribution_survives_shifted_line_numbers() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("revert_shift.txt");

    fs::write(&file_path, "keep\nrestored ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "revert_shift.txt"])
        .unwrap();
    repo.stage_all_and_commit("source ai line").unwrap();
    let mut file = repo.filename("revert_shift.txt");
    file.assert_committed_lines(crate::lines!["keep".ai(), "restored ai".ai()]);

    fs::write(&file_path, "keep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_shift.txt"])
        .unwrap();
    repo.stage_all_and_commit("delete ai line").unwrap();
    let delete_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    file.assert_committed_lines(crate::lines!["keep".ai()]);

    fs::write(&file_path, "later human\nkeep\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "revert_shift.txt"])
        .unwrap();
    repo.stage_all_and_commit("prepend later human line")
        .unwrap();
    file.assert_committed_lines(crate::lines!["later human".human(), "keep".ai()]);

    repo.git(&["revert", &delete_commit]).unwrap();
    file.assert_committed_lines(crate::lines![
        "later human".human(),
        "keep".ai(),
        "restored ai".ai(),
    ]);
}
