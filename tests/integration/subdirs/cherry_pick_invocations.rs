use super::{ExpectedLineExt, fs};

crate::subdir_test_variants! {
    fn cherry_pick_single_commit() {
        // Test cherry-picking a single AI-authored commit
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit on default branch
        let mut file = repo.filename("file.txt");
        file.set_contents(crate::lines!["Initial content"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        // Get current branch name
        let main_branch = repo.current_branch();

        // Create feature branch with AI-authored changes
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        file.insert_at(1, crate::lines!["AI feature line".ai()]);
        repo.stage_all_and_commit("Add AI feature").unwrap();
        let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Switch back to main and cherry-pick the feature commit
        repo.git(&["checkout", &main_branch]).unwrap();
        repo.git_from_working_dir(&working_dir, &["cherry-pick", &feature_commit])
            .expect("Cherry-pick should succeed");

        // Verify final file state - hooks should have preserved AI authorship
        file.assert_lines_and_blame(crate::lines!["Initial content".ai(), "AI feature line".ai(),]);
    }
}

crate::subdir_test_variants! {
    fn cherry_pick_multiple_commits() {
        // Test cherry-picking multiple commits in sequence
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit on default branch
        let mut file = repo.filename("file.txt");
        file.set_contents(crate::lines!["Line 1", ""]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        let main_branch = repo.current_branch();

        // Create feature branch with multiple AI-authored commits
        repo.git(&["checkout", "-b", "feature"]).unwrap();

        // First AI commit
        file.insert_at(1, crate::lines!["AI line 2".ai()]);
        repo.stage_all_and_commit("AI commit 1").unwrap();
        let commit1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Second AI commit
        file.insert_at(2, crate::lines!["AI line 3".ai()]);
        repo.stage_all_and_commit("AI commit 2").unwrap();
        let commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Third AI commit
        file.insert_at(3, crate::lines!["AI line 4".ai()]);
        repo.stage_all_and_commit("AI commit 3").unwrap();
        let commit3 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Switch back to main and cherry-pick all three commits
        repo.git(&["checkout", &main_branch]).unwrap();
        repo.git_from_working_dir(&working_dir, &["cherry-pick", &commit1, &commit2, &commit3])
            .expect("Cherry-pick multiple commits should succeed");

        // Verify final file state - hooks should have preserved AI authorship
        file.assert_lines_and_blame(crate::lines![
            "Line 1".human(),
            "AI line 2".ai(),
            "AI line 3".ai(),
            "AI line 4".ai(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn cherry_pick_with_conflict() {
        // Test cherry-pick with conflicts and --continue
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit on default branch
        let mut file = repo.filename("file.txt");
        file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        let main_branch = repo.current_branch();

        // Create feature branch with AI changes
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        file.replace_at(1, "AI_FEATURE_VERSION".ai());
        repo.stage_all_and_commit("AI feature").unwrap();
        let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Switch back to main and make conflicting change
        repo.git(&["checkout", &main_branch]).unwrap();
        file.replace_at(1, "MAIN_BRANCH_VERSION".human());
        repo.stage_all_and_commit("Human change").unwrap();

        // Try to cherry-pick (should conflict)
        let cherry_pick_result = repo.git_from_working_dir(&working_dir, &["cherry-pick", &feature_commit]);
        assert!(cherry_pick_result.is_err(), "Should have conflict");

        // Resolve conflict by choosing the AI version
        fs::write(
            repo.path().join("file.txt"),
            "Line 1\nAI_FEATURE_VERSION\nLine 3",
        )
        .unwrap();
        repo.git(&["add", "file.txt"]).unwrap();

        // Continue cherry-pick (need GIT_EDITOR for commit message)
        repo.git_with_env(
            &["cherry-pick", "--continue"],
            &[("GIT_EDITOR", "true")],
            Some(&working_dir)
        )
        .expect("Cherry-pick continue should succeed");

        // Verify final file state - hooks should have preserved AI authorship
        file.assert_lines_and_blame(crate::lines![
            "Line 1".human(),
            "AI_FEATURE_VERSION".ai(),
            "Line 3".human(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn cherry_pick_abort() {
        // Test cherry-pick --abort
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit on default branch
        let mut file = repo.filename("file.txt");
        file.set_contents(crate::lines!["Line 1", "Line 2"]);
        repo.stage_all_and_commit("Initial commit").unwrap();
        let initial_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        let main_branch = repo.current_branch();

        // Create feature branch with AI changes (modify line 2)
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        file.replace_at(1, "AI modification of line 2".ai());
        repo.stage_all_and_commit("AI feature").unwrap();

        // Assert intermediary blame
        file.assert_lines_and_blame(crate::lines!["Line 1".human(), "AI modification of line 2".ai(),]);

        let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Switch back to main and make conflicting change (also modify line 2)
        repo.git(&["checkout", &main_branch]).unwrap();
        file.replace_at(1, "Human modification of line 2".human());
        repo.stage_all_and_commit("Human change").unwrap();

        // Assert intermediary blame
        file.assert_lines_and_blame(crate::lines![
            "Line 1".human(),
            "Human modification of line 2".human(),
        ]);

        // Try to cherry-pick (should conflict)
        let cherry_pick_result = repo.git_from_working_dir(&working_dir, &["cherry-pick", &feature_commit]);
        assert!(cherry_pick_result.is_err(), "Should have conflict");

        // Abort the cherry-pick
        repo.git_from_working_dir(&working_dir, &["cherry-pick", "--abort"])
            .expect("Cherry-pick abort should succeed");

        // Verify HEAD is back to before the cherry-pick
        let current_head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
        assert_ne!(current_head, initial_head); // Different because we made the "Human change" commit

        // Verify final file state (should have human's version)
        file.assert_lines_and_blame(crate::lines![
            "Line 1".human(),
            "Human modification of line 2".human(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn cherry_pick_no_ai_authorship() {
        // Test cherry-picking from branch without AI authorship
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit on default branch
        let mut file = repo.filename("file.txt");
        file.set_contents(crate::lines!["Line 1"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        let main_branch = repo.current_branch();
        // Create feature branch with human-only changes (no AI)
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        file.insert_at(1, crate::lines!["Human line 2".human()]);
        repo.stage_all_and_commit("Human feature").unwrap();
        let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Switch back to main and cherry-pick
        repo.git(&["checkout", &main_branch]).unwrap();
        repo.git_from_working_dir(&working_dir, &["cherry-pick", &feature_commit])
            .expect("Cherry-pick should succeed");

        // Verify final file state - should have no AI authorship
        file.assert_lines_and_blame(crate::lines!["Line 1".human(), "Human line 2".human(),]);
    }
}

crate::subdir_test_variants! {
    fn cherry_pick_multiple_ai_sessions() {
        // Test cherry-pick preserving multiple AI sessions from different commits
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit on default branch
        let mut file = repo.filename("main.rs");
        file.set_contents(crate::lines!["fn main() {}"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        let main_branch = repo.current_branch();

        // Create feature branch
        repo.git(&["checkout", "-b", "feature"]).unwrap();

        // First AI session adds logging
        file.replace_at(0, "fn main() {".human());
        file.insert_at(1, crate::lines!["    println!(\"Starting\");".ai()]);
        file.insert_at(2, crate::lines!["}".human()]);
        repo.stage_all_and_commit("Add logging").unwrap();
        let commit1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Second AI session adds error handling
        file.insert_at(2, crate::lines!["    // TODO: Add error handling".ai()]);
        repo.stage_all_and_commit("Add error handling").unwrap();
        let commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // Cherry-pick both to main
        repo.git(&["checkout", &main_branch]).unwrap();
        repo.git_from_working_dir(&working_dir, &["cherry-pick", &commit1, &commit2])
            .expect("Cherry-pick multiple AI sessions should succeed");

        // Verify final file state - hooks should have preserved AI authorship
        file.assert_lines_and_blame(crate::lines![
            "fn main() {".ai(),
            "    println!(\"Starting\");".ai(),
            "    // TODO: Add error handling".ai(),
            "}".human(),
        ]);
    }
}
