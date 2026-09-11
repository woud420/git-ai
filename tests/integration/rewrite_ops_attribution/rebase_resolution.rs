use super::{ExpectedLineExt, TestRepo, claude_checkpoint, fs};

#[test]
fn test_named_branch_conflict_rebase_keeps_feature_side_ai_attribution() {
    let repo = TestRepo::new();
    let animals_path = repo.path().join("jokes-animals.csv");
    let dad_path = repo.path().join("jokes-dad.csv");

    let animals_base = "\
setup,punchline
What do you call a bear with no teeth?,A gummy bear
Why did the chicken go to the movie?,To see the hen-ema
What do you call an alligator in a vest?,An investigator
";
    let dad_base = "\
setup,punchline
Why don't scientists trust atoms?,Because they make up everything
What did the ocean say to the beach?,Nothing it just waved
";

    fs::write(&animals_path, animals_base).unwrap();
    fs::write(&dad_path, dad_base).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-animals.csv"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-dad.csv"])
        .unwrap();
    repo.stage_all_and_commit("base jokes").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "scenario-3-multi-file-conflict"])
        .unwrap();
    fs::write(
        &animals_path,
        format!("{animals_base}What do you call a sleeping bull?,A dozer\n"),
    )
    .unwrap();
    fs::write(
        &dad_path,
        format!("{dad_base}What do you call a bear in the rain?,A drizzly bear\n"),
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-animals.csv"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-dad.csv"])
        .unwrap();
    repo.stage_all_and_commit("Add bull and drizzly bear jokes")
        .unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(
        &animals_path,
        format!("{animals_base}What's a cat's favorite color?,Purr-ple\n"),
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-animals.csv"])
        .unwrap();
    repo.stage_all_and_commit("Add purple cat joke").unwrap();

    let rebase_result = repo.git(&["rebase", &main_branch, "scenario-3-multi-file-conflict"]);
    assert!(
        rebase_result.is_err(),
        "named-branch rebase should conflict on jokes-animals.csv"
    );

    fs::write(
        &animals_path,
        format!(
            "{animals_base}What's a cat's favorite color?,Purr-ple\nWhat do you call a sleeping bull?,A dozer\n"
        ),
    )
    .unwrap();
    repo.git(&["add", "jokes-animals.csv"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let rebased_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_ne!(
        rebased_head, feature_commit,
        "rebase should rewrite the feature commit"
    );
    let note = repo
        .read_authorship_note(&rebased_head)
        .expect("rebased feature commit should have an authorship note");
    assert!(
        note.contains("jokes-animals.csv"),
        "rebased note should retain conflict-file attribution: {note}"
    );

    let mut animals = repo.filename("jokes-animals.csv");
    animals.assert_committed_lines(crate::lines![
        "setup,punchline".ai(),
        "What do you call a bear with no teeth?,A gummy bear".ai(),
        "Why did the chicken go to the movie?,To see the hen-ema".ai(),
        "What do you call an alligator in a vest?,An investigator".ai(),
        "What's a cat's favorite color?,Purr-ple".ai(),
        "What do you call a sleeping bull?,A dozer".ai(),
    ]);
}

#[test]
fn test_named_branch_conflict_rebase_agent_keep_both_without_intermediate_sync() {
    let repo = TestRepo::new();
    let animals_path = repo.path().join("jokes-animals.csv");
    let dad_path = repo.path().join("jokes-dad.csv");

    let animals_base = "\
setup,punchline
What do you call a bear with no teeth?,A gummy bear
Why did the chicken go to the movie?,To see the hen-ema
What do you call an alligator in a vest?,An investigator
";
    let dad_base = "\
setup,punchline
Why don't scientists trust atoms?,Because they make up everything
What did the ocean say to the beach?,Nothing it just waved
";

    fs::write(&animals_path, animals_base).unwrap();
    fs::write(&dad_path, dad_base).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-animals.csv"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-dad.csv"])
        .unwrap();
    repo.stage_all_and_commit("base jokes").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "scenario-3-multi-file-conflict"])
        .unwrap();
    fs::write(
        &animals_path,
        format!("{animals_base}What do you call a sleeping bull?,A dozer\n"),
    )
    .unwrap();
    fs::write(
        &dad_path,
        format!("{dad_base}What do you call a bear in the rain?,A drizzly bear\n"),
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-animals.csv"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-dad.csv"])
        .unwrap();
    repo.stage_all_and_commit("Add bull and drizzly bear jokes")
        .unwrap();

    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(
        &animals_path,
        format!("{animals_base}What's a cat's favorite color?,Purr-ple\n"),
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-animals.csv"])
        .unwrap();
    repo.stage_all_and_commit("Add purple cat joke").unwrap();

    let rebase_result = repo.git(&["rebase", &main_branch, "scenario-3-multi-file-conflict"]);
    assert!(
        rebase_result.is_err(),
        "named-branch rebase should conflict on jokes-animals.csv"
    );

    // This mirrors a real agent edit: a pre-edit checkpoint first captures the
    // conflict-marker file, then the AI checkpoint captures the resolved file.
    repo.git_ai(&["checkpoint", "human", "jokes-animals.csv"])
        .unwrap();
    fs::write(
        &animals_path,
        format!(
            "{animals_base}What's a cat's favorite color?,Purr-ple\nWhat do you call a sleeping bull?,A dozer\n"
        ),
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-animals.csv"])
        .unwrap();
    repo.git(&["add", "jokes-animals.csv"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let mut animals = repo.filename("jokes-animals.csv");
    animals.assert_committed_lines(crate::lines![
        "setup,punchline".ai(),
        "What do you call a bear with no teeth?,A gummy bear".ai(),
        "Why did the chicken go to the movie?,To see the hen-ema".ai(),
        "What do you call an alligator in a vest?,An investigator".ai(),
        "What's a cat's favorite color?,Purr-ple".ai(),
        "What do you call a sleeping bull?,A dozer".ai(),
    ]);
}

#[test]
fn test_named_branch_conflict_rebase_real_claude_keep_both_without_intermediate_sync() {
    let repo = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND",
        "rebase=1500",
    )]);
    let animals_path = repo.path().join("jokes-animals.csv");
    let dad_path = repo.path().join("jokes-dad.csv");

    let animals_base = "\
setup,punchline
What do you call a bear with no teeth?,A gummy bear
Why did the chicken go to the movie?,To see the hen-ema
What do you call an alligator in a vest?,An investigator
";
    let dad_base = "\
setup,punchline
Why don't scientists trust atoms?,Because they make up everything
What did the ocean say to the beach?,Nothing it just waved
";

    fs::write(&animals_path, animals_base).unwrap();
    fs::write(&dad_path, dad_base).unwrap();
    claude_checkpoint(&repo, "PostToolUse", &animals_path, "setup-session");
    claude_checkpoint(&repo, "PostToolUse", &dad_path, "setup-session");
    repo.stage_all_and_commit("base jokes").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "scenario-3-multi-file-conflict"])
        .unwrap();
    fs::write(
        &animals_path,
        format!("{animals_base}What do you call a sleeping bull?,A dozer\n"),
    )
    .unwrap();
    fs::write(
        &dad_path,
        format!("{dad_base}What do you call a bear in the rain?,A drizzly bear\n"),
    )
    .unwrap();
    claude_checkpoint(&repo, "PostToolUse", &animals_path, "feature-session");
    claude_checkpoint(&repo, "PostToolUse", &dad_path, "feature-session");
    repo.stage_all_and_commit("Add bull and drizzly bear jokes")
        .unwrap();

    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(
        &animals_path,
        format!("{animals_base}What's a cat's favorite color?,Purr-ple\n"),
    )
    .unwrap();
    claude_checkpoint(&repo, "PostToolUse", &animals_path, "main-session");
    repo.stage_all_and_commit("Add purple cat joke").unwrap();

    let rebase_result = repo.git(&["rebase", &main_branch, "scenario-3-multi-file-conflict"]);
    assert!(
        rebase_result.is_err(),
        "named-branch rebase should conflict on jokes-animals.csv"
    );

    claude_checkpoint(&repo, "PreToolUse", &animals_path, "resolve-session");
    fs::write(
        &animals_path,
        format!(
            "{animals_base}What's a cat's favorite color?,Purr-ple\nWhat do you call a sleeping bull?,A dozer\n"
        ),
    )
    .unwrap();
    claude_checkpoint(&repo, "PostToolUse", &animals_path, "resolve-session");
    repo.git(&["add", "jokes-animals.csv"]).unwrap();
    repo.git_without_test_sync_for_test(&["rebase", "--continue"], &[("GIT_EDITOR", "true")])
        .unwrap();

    let mut animals = repo.filename("jokes-animals.csv");
    animals.assert_committed_lines(crate::lines![
        "setup,punchline".ai(),
        "What do you call a bear with no teeth?,A gummy bear".ai(),
        "Why did the chicken go to the movie?,To see the hen-ema".ai(),
        "What do you call an alligator in a vest?,An investigator".ai(),
        "What's a cat's favorite color?,Purr-ple".ai(),
        "What do you call a sleeping bull?,A dozer".ai(),
    ]);
}

#[test]
fn test_named_branch_conflict_rebase_agent_rewrite_without_intermediate_sync() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("conflict.csv");

    let base = "\
setup,punchline
base joke,base punchline
";
    fs::write(&file_path, base).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.stage_all_and_commit("base jokes").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature-rewrite-conflict"])
        .unwrap();
    fs::write(
        &file_path,
        format!("{base}feature joke,feature punchline\n"),
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.stage_all_and_commit("feature joke").unwrap();

    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(&file_path, format!("{base}main joke,main punchline\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.stage_all_and_commit("main joke").unwrap();

    let rebase_result = repo.git(&["rebase", &main_branch, "feature-rewrite-conflict"]);
    assert!(rebase_result.is_err(), "rebase should stop for a conflict");

    repo.git_ai(&["checkpoint", "human", "conflict.csv"])
        .unwrap();
    fs::write(&file_path, format!("{base}rewritten by ai,new punchline\n")).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict.csv"])
        .unwrap();
    repo.git(&["add", "conflict.csv"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let mut file = repo.filename("conflict.csv");
    file.assert_committed_lines(crate::lines![
        "setup,punchline".ai(),
        "base joke,base punchline".ai(),
        "rewritten by ai,new punchline".ai(),
    ]);
}

/// Rebase with conflict resolved via `checkout --theirs` should preserve attribution.
/// In rebase context, `--theirs` means the branch being rebased (feature branch).
#[test]
fn test_rebase_conflict_theirs_preserves_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.txt");

    // Initial commit
    fs::write(&file_path, "line-1\nline-2\nline-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let main_branch = repo.current_branch();

    // Feature branch: AI prepend
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(&file_path, "ai-prepend\nline-1\nline-2\nline-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature: AI prepend").unwrap();

    // Main: conflicting prepend
    repo.git(&["checkout", &main_branch]).unwrap();
    fs::write(&file_path, "main-prepend\nline-1\nline-2\nline-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "main.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("main: human prepend").unwrap();

    // Rebase feature onto main (will conflict)
    repo.git(&["checkout", "feature"]).unwrap();
    let _ = repo.git(&["rebase", &main_branch]); // expect conflict

    // Resolve by taking theirs (feature branch's version in rebase context)
    repo.git(&["checkout", "--theirs", "--", "main.txt"])
        .unwrap();
    repo.git(&["add", "main.txt"]).unwrap();

    // Set GIT_EDITOR to avoid interactive editor during rebase --continue
    let result = repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None);
    assert!(result.is_ok(), "rebase --continue failed: {:?}", result);

    // After rebase --continue, the rebased commit should retain AI attribution
    // In rebase context, --theirs = feature branch version = ai-prepend + original lines
    let mut file = repo.filename("main.txt");
    file.assert_committed_lines(crate::lines![
        "ai-prepend".ai(),
        "line-1".human(),
        "line-2".human(),
        "line-3".human(),
    ]);
}
