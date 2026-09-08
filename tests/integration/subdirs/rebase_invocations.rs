use super::{ExpectedLineExt, fs};

crate::subdir_test_variants! {
    fn rebase_no_conflicts() {
        // Test that rebase works correctly
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src").join("lib");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main line 1", "main line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get the default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines![
        "// AI generated feature 1".ai(),
        "feature line 1".ai()
    ]);
    repo.stage_all_and_commit("AI feature 1").unwrap();

    // Second AI commit
    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines![
        "// AI generated feature 2".ai(),
        "feature line 2".ai()
    ]);
    repo.stage_all_and_commit("AI feature 2").unwrap();

    // Advance default branch (non-conflicting)
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other content"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch (hooks will handle authorship tracking)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git_from_working_dir(&working_dir, &["rebase", &default_branch])
        .expect("Rebase should succeed");

    // Verify authorship was preserved for both files after rebase
    feature1.assert_lines_and_blame(crate::lines![
        "// AI generated feature 1".ai(),
        "feature line 1".ai()
    ]);
    feature2.assert_lines_and_blame(crate::lines![
        "// AI generated feature 2".ai(),
        "feature line 2".ai()
    ]);
    }
}

crate::subdir_test_variants! {
    fn rebase_multiple_commits() {
        // Test rebase with multiple commits
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with multiple AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines!["// AI feature 1".ai()]);
    repo.stage_all_and_commit("AI feature 1").unwrap();

    // Second AI commit
    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines!["// AI feature 2".ai()]);
    repo.stage_all_and_commit("AI feature 2").unwrap();

    // Third AI commit
    let mut feature3 = repo.filename("feature3.txt");
    feature3.set_contents(crate::lines!["// AI feature 3".ai()]);
    repo.stage_all_and_commit("AI feature 3").unwrap();

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main2_file = repo.filename("main2.txt");
    main2_file.set_contents(crate::lines!["more main content"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch (hooks will handle authorship tracking)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git_from_working_dir(&working_dir, &["rebase", &default_branch])
        .expect("Rebase should succeed");

    // Verify all files have preserved AI authorship after rebase
    feature1.assert_lines_and_blame(crate::lines!["// AI feature 1".ai()]);
    feature2.assert_lines_and_blame(crate::lines!["// AI feature 2".ai()]);
    feature3.assert_lines_and_blame(crate::lines!["// AI feature 3".ai()]);
    }
}

crate::subdir_test_variants! {
    fn rebase_mixed_authorship() {
        // Test rebase where only some commits have authorship logs
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src").join("components");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Human commit (no AI authorship)
    let mut human_file = repo.filename("human.txt");
    human_file.set_contents(crate::lines!["human work"]);
    repo.stage_all_and_commit("Human work").unwrap();

    // AI commit
    let mut ai_file = repo.filename("ai.txt");
    ai_file.set_contents(crate::lines!["// AI work".ai()]);
    repo.stage_all_and_commit("AI work").unwrap();

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main2_file = repo.filename("main2.txt");
    main2_file.set_contents(crate::lines!["more main"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch (hooks will handle authorship tracking)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git_from_working_dir(&working_dir, &["rebase", &default_branch])
        .expect("Rebase should succeed");

    // Verify authorship was preserved correctly
    human_file.assert_lines_and_blame(crate::lines!["human work".human()]);
    ai_file.assert_lines_and_blame(crate::lines!["// AI work".ai()]);
    }
}

crate::subdir_test_variants! {
    fn rebase_with_different_trees() {
        // Test rebase where trees differ (parent changes result in different tree IDs)
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("lib");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with multiple AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines!["// AI added feature 1".ai()]);
    repo.stage_all_and_commit("AI changes 1").unwrap();

    // Second AI commit
    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines!["// AI added feature 2".ai()]);
    repo.stage_all_and_commit("AI changes 2").unwrap();

    // Go back to default branch and add a different file (non-conflicting)
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Main changes").unwrap();

    // Rebase feature onto default branch (no conflicts, but trees will differ)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git_from_working_dir(&working_dir, &["rebase", &default_branch])
        .expect("Rebase should succeed");

    // Verify authorship was preserved for both files after rebase
    feature1.assert_lines_and_blame(crate::lines!["// AI added feature 1".ai()]);
    feature2.assert_lines_and_blame(crate::lines!["// AI added feature 2".ai()]);
    }
}

crate::subdir_test_variants! {
    fn rebase_with_files_in_subdirs() {
        // Test rebase where feature branch has files in subdirectories
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src").join("lib");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with AI commits in subdirectories
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Create subdirectory file with AI content
    let subdir_path = repo.path().join("src").join("lib");
    fs::create_dir_all(&subdir_path).unwrap();
    let mut feature_file = repo.filename("src/lib/utils.rs");
    feature_file.set_contents(crate::lines![
        "// AI generated utils".ai(),
        "pub fn helper() {}".ai()
    ]);
    repo.stage_all_and_commit("AI feature in subdir").unwrap();

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other content"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git_from_working_dir(&working_dir, &["rebase", &default_branch])
        .expect("Rebase should succeed");

    // Verify authorship was preserved for file in subdirectory after rebase
    feature_file.assert_lines_and_blame(crate::lines![
        "// AI generated utils".ai(),
        "pub fn helper() {}".ai()
    ]);
    }
}

crate::subdir_test_variants! {
    fn rebase_nested() {
        // Test rebase when run from a deeply nested subdirectory
    let repo = TestRepo::new();

    // Create deeply nested subdirectory structure
    let working_dir = repo.path().join("a").join("b").join("c");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch with AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines![
        "// AI feature 1".ai(),
        "function feature1() {}".ai()
    ]);
    repo.stage_all_and_commit("AI feature 1").unwrap();

    // Second AI commit
    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines![
        "// AI feature 2".ai(),
        "function feature2() {}".ai()
    ]);
    repo.stage_all_and_commit("AI feature 2").unwrap();

    // Advance default branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto default branch
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git_from_working_dir(&working_dir, &["rebase", &default_branch])
        .expect("Rebase should succeed");

    // Verify AI authorship is preserved after rebase
    feature1.assert_lines_and_blame(crate::lines![
        "// AI feature 1".ai(),
        "function feature1() {}".ai()
    ]);
    feature2.assert_lines_and_blame(crate::lines![
        "// AI feature 2".ai(),
        "function feature2() {}".ai()
    ]);
    }
}

crate::subdir_test_variants! {
    fn rebase_fast_forward() {
        // Test empty rebase (fast-forward)
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content"]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Get default branch name
    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Add commit on feature
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["// AI feature".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();

    // Rebase onto default branch (should be fast-forward, no changes)
    repo.git_from_working_dir(&working_dir, &["rebase", &default_branch])
        .expect("Fast-forward rebase should succeed");

    // Verify authorship is still correct after fast-forward rebase
    feature_file.assert_lines_and_blame(crate::lines!["// AI feature".ai()]);
    }
}

crate::subdir_test_variants! {
    fn rebase_with_conflicts() {
        // Test rebase --onto from a subdirectory; ensure authorship preserved
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit
        let mut base_file = repo.filename("base.txt");
        base_file.set_contents(crate::lines!["base content"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        let default_branch = repo.current_branch();

        // Create old_base branch and commit
        repo.git(&["checkout", "-b", "old_base"]).unwrap();
        let mut old_file = repo.filename("old.txt");
        old_file.set_contents(crate::lines!["old base"]);
        repo.stage_all_and_commit("Old base commit").unwrap();
        let old_base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Create feature branch from old_base with AI commit
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        let mut feature_file = repo.filename("feature.txt");
        feature_file.set_contents(crate::lines!["// AI feature".ai()]);
        repo.stage_all_and_commit("AI feature").unwrap();

        // Create new_base branch from default branch
        repo.git(&["checkout", &default_branch]).unwrap();
        repo.git(&["checkout", "-b", "new_base"]).unwrap();
        let mut new_file = repo.filename("new.txt");
        new_file.set_contents(crate::lines!["new base"]);
        repo.stage_all_and_commit("New base commit").unwrap();
        let new_base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Rebase feature --onto new_base old_base from the subdirectory
        repo.git(&["checkout", "feature"]).unwrap();
        repo.git_from_working_dir(
            &working_dir,
            &["rebase", "--onto", &new_base_sha, &old_base_sha]
        )
        .expect("Rebase --onto should succeed");

        // Verify authorship preserved after rebase
        feature_file.assert_lines_and_blame(crate::lines!["// AI feature".ai()]);
    }
}

crate::subdir_test_variants! {
    fn rebase_abort() {
        // Test rebase abort - ensures no authorship corruption
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut conflict_file = repo.filename("conflict.txt");
    conflict_file.set_contents(crate::lines!["line 1", "line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch with AI changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    conflict_file.replace_at(1, "AI CHANGE".ai());
    repo.stage_all_and_commit("AI changes").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Make conflicting change on main
    repo.git(&["checkout", &default_branch]).unwrap();
    conflict_file.replace_at(1, "MAIN CHANGE".human());
    repo.stage_all_and_commit("Main changes").unwrap();

    // Try to rebase - will conflict
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git_from_working_dir(&working_dir, &["rebase", &default_branch]);

    // Should conflict
    assert!(rebase_result.is_err(), "Rebase should conflict");

    // Abort the rebase
    repo.git_from_working_dir(&working_dir, &["rebase", "--abort"])
        .expect("Rebase abort should succeed");

    // Verify we're back to original commit
    let current_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_eq!(
        current_commit, feature_commit,
        "Should be back to original commit after abort"
    );

    // Verify original authorship is intact (by checking file blame)
    conflict_file.assert_lines_and_blame(crate::lines!["line 1".human(), "AI CHANGE".ai()]);
    }
}
