use super::{ExpectedLineExt, fs};

crate::subdir_test_variants! {
    fn merge_with_ai_contributions() {
        // Test merge with AI contributions
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("test.txt");

        // Create base file and initial commit
        file.set_contents(crate::lines!["Base line 1", "Base line 2", "Base line 3"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        // Save the default branch name before creating feature branch
        let default_branch = repo.current_branch();

        // Create a feature branch
        repo.git(&["checkout", "-b", "feature"]).unwrap();

        // Make AI changes on feature branch (insert after line 3)
        file.insert_at(3, crate::lines!["FEATURE LINE 1".ai(), "FEATURE LINE 2".ai()]);
        repo.stage_all_and_commit("feature branch changes").unwrap();

        // Switch back to default branch and make human changes
        repo.git(&["checkout", &default_branch]).unwrap();
        file = repo.filename("test.txt"); // Reload file from default branch
        // Insert at beginning to avoid conflict with feature branch
        file.insert_at(0, crate::lines!["MAIN LINE 1", "MAIN LINE 2"]);
        repo.stage_all_and_commit("main branch changes").unwrap();

        // Merge feature branch into default branch (should not conflict)
        repo.git_from_working_dir(&working_dir, &["merge", "feature", "-m", "merge feature into main"])
            .expect("Merge should succeed");

        // Test blame after merge - should have both AI and human contributions
        file = repo.filename("test.txt");
        file.assert_lines_and_blame(crate::lines![
            "MAIN LINE 1".human(),
            "MAIN LINE 2".human(),
            "Base line 1".human(),
            "Base line 2".human(),
            "Base line 3".ai(),
            "FEATURE LINE 1".ai(),
            "FEATURE LINE 2".ai(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn merge_with_conflicts() {
        // Test merge with conflicts
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("conflict.txt");

        // Create base file and initial commit
        file.set_contents(crate::lines!["line 1", "line 2", "line 3"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        let default_branch = repo.current_branch();

        // Create feature branch with AI changes
        repo.git(&["checkout", "-b", "feature"]).unwrap();
        file.replace_at(1, "AI_FEATURE_VERSION".ai());
        repo.stage_all_and_commit("AI feature").unwrap();

        // Switch back to default branch and make conflicting change
        repo.git(&["checkout", &default_branch]).unwrap();
        file.replace_at(1, "MAIN_BRANCH_VERSION".human());
        repo.stage_all_and_commit("Human change").unwrap();

        // Try to merge (should conflict)
        let merge_result = repo.git_from_working_dir(&working_dir, &["merge", "feature", "-m", "Merge feature"]);
        assert!(merge_result.is_err(), "Should have conflict");

        // Resolve conflict by choosing the AI version
        fs::write(
            repo.path().join("conflict.txt"),
            "line 1\nAI_FEATURE_VERSION\nline 3",
        )
        .unwrap();
        repo.git(&["add", "conflict.txt"]).unwrap();

        // Continue merge (need GIT_EDITOR for commit message)
        repo.git_with_env(
            &["commit", "--no-edit"],
            &[("GIT_EDITOR", "true")],
            Some(&working_dir)
        )
        .expect("Merge continue should succeed");

        // Verify final file state - hooks should have preserved AI authorship
        file.assert_lines_and_blame(crate::lines![
            "line 1".human(),
            "AI_FEATURE_VERSION".ai(),
            "line 3".human(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn squash_merge() {
        // Test merge --squash with AI contributions
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("main.txt");

        // Create master branch with initial content
        file.set_contents(crate::lines!["line 1", "line 2", "line 3", ""]);
        repo.stage_all_and_commit("Initial commit on master")
            .unwrap();

        let default_branch = repo.current_branch();

        // Create feature branch
        repo.git(&["checkout", "-b", "feature"]).unwrap();

        // Add AI changes on feature branch
        file.insert_at(3, crate::lines!["// AI added feature".ai()]);
        repo.stage_all_and_commit("Add AI feature").unwrap();

        // Add human changes on feature branch
        file.insert_at(4, crate::lines!["// Human refinement"]);
        repo.stage_all_and_commit("Human refinement").unwrap();

        // Go back to master and squash merge
        repo.git(&["checkout", &default_branch]).unwrap();
        repo.git_from_working_dir(&working_dir, &["merge", "--squash", "feature"])
            .expect("Squash merge should succeed");
        repo.commit("Squashed feature").unwrap();

        // Verify AI attribution is preserved
        file.assert_lines_and_blame(crate::lines![
            "line 1".human(),
            "line 2".human(),
            "line 3".human(),
            "// AI added feature".ai(),
            "// Human refinement".human()
        ]);
    }
}

crate::subdir_test_variants! {
    fn stash_pop_with_ai_attribution() {
        // Test stash pop with AI attribution
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit
        let mut readme = repo.filename("README.md");
        readme.set_contents(vec!["# Test Repo".to_string()]);
        repo.stage_all_and_commit("initial commit")
            .expect("commit should succeed");

        // Create a file with AI attribution
        let mut example = repo.filename("example.txt");
        example.set_contents(vec!["line 1".ai(), "line 2".ai(), "line 3".ai()]);

        // Run checkpoint to track AI attribution
        repo.git_ai(&["checkpoint", "mock_ai"])
            .expect("checkpoint should succeed");

        // Stash the changes
        repo.git_from_working_dir(&working_dir, &["stash", "push", "-m", "test stash"])
            .expect("stash should succeed");

        // Verify file is gone
        assert!(repo.read_file("example.txt").is_none());

        // Pop the stash
        repo.git_from_working_dir(&working_dir, &["stash", "pop"])
            .expect("stash pop should succeed");

        // Verify file is back
        assert!(repo.read_file("example.txt").is_some());

        // Commit the changes
        let commit = repo
            .stage_all_and_commit("apply stashed changes")
            .expect("commit should succeed");

        // Verify AI attribution is preserved
        example.assert_lines_and_blame(vec!["line 1".ai(), "line 2".ai(), "line 3".ai()]);

        // Check authorship log has AI prompts
        assert!(
            !commit.authorship_log.metadata.sessions.is_empty(),
            "Expected sessions in authorship log"
        );
    }
}

crate::subdir_test_variants! {
    fn stash_apply_with_ai_attribution() {
        // Test stash apply with AI attribution
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit
        let mut readme = repo.filename("README.md");
        readme.set_contents(vec!["# Test Repo".to_string()]);
        repo.stage_all_and_commit("initial commit")
            .expect("commit should succeed");

        // Create a file with AI attribution
        let mut example = repo.filename("example.txt");
        example.set_contents(vec!["line 1".ai(), "line 2".ai()]);

        // Run checkpoint to track AI attribution
        repo.git_ai(&["checkpoint", "mock_ai"])
            .expect("checkpoint should succeed");

        // Stash the changes
        repo.git_from_working_dir(&working_dir, &["stash"])
            .expect("stash should succeed");

        // Apply (not pop) the stash
        repo.git_from_working_dir(&working_dir, &["stash", "apply"])
            .expect("stash apply should succeed");

        // Commit the changes
        let commit = repo
            .stage_all_and_commit("apply stashed changes")
            .expect("commit should succeed");

        // Verify AI attribution is preserved
        example.assert_lines_and_blame(vec!["line 1".ai(), "line 2".ai()]);

        // Check authorship log has AI prompts
        assert!(
            !commit.authorship_log.metadata.sessions.is_empty(),
            "Expected sessions in authorship log"
        );
    }
}

crate::subdir_test_variants! {
    fn stash_nested() {
        // Test stash operations when run from a deeply nested subdirectory
        let repo = TestRepo::new();

        // Create deeply nested subdirectory structure
        let working_dir = repo.path().join("a").join("b").join("c");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit
        let mut readme = repo.filename("README.md");
        readme.set_contents(vec!["# Test Repo".to_string()]);
        repo.stage_all_and_commit("initial commit")
            .expect("commit should succeed");

        // Create a file with AI attribution
        let mut example = repo.filename("example.txt");
        example.set_contents(vec!["line 1".ai(), "line 2".ai()]);

        // Run checkpoint to track AI attribution
        repo.git_ai(&["checkpoint", "mock_ai"])
            .expect("checkpoint should succeed");

        // Stash the changes
        repo.git_from_working_dir(&working_dir, &["stash", "push", "-m", "test stash"])
            .expect("stash should succeed");

        // Verify file is gone
        assert!(repo.read_file("example.txt").is_none());

        // Pop the stash
        repo.git_from_working_dir(&working_dir, &["stash", "pop"])
            .expect("stash pop should succeed");

        // Verify file is back
        assert!(repo.read_file("example.txt").is_some());

        // Commit the changes
        let commit = repo
            .stage_all_and_commit("apply stashed changes")
            .expect("commit should succeed");

        // Verify AI attribution is preserved
        example.assert_lines_and_blame(vec!["line 1".ai(), "line 2".ai()]);

        // Check authorship log has AI prompts
        assert!(
            !commit.authorship_log.metadata.sessions.is_empty(),
            "Expected sessions in authorship log"
        );
    }
}
