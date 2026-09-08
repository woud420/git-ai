use super::{ExpectedLineExt, fs};

crate::subdir_test_variants! {
    fn commit() {
        // Test that git commit works correctly when run from within a subdirectory
        let repo = TestRepo::new();

        // Create a subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial file in root
        let mut root_file = repo.filename("README.md");
        root_file.set_contents(crate::lines!["# Project".human()]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        // Create a file in the subdirectory
        let subdir_file_path = working_dir.join("utils.rs");
        fs::write(&subdir_file_path, "pub fn helper() {\n    println!(\"hello\");\n}\n").unwrap();

        // Stage the file
        repo.git(&["add", "src/lib/utils.rs"]).unwrap();

        // Create AI checkpoint for the file in subdirectory
        repo.git_ai(&["checkpoint", "mock_ai", "src/lib/utils.rs"]).unwrap();

        // Now commit from within the subdirectory (not using -C flag)
        // This simulates running "git commit" from within the subdirectory
        // git-ai should automatically find the repository root
        repo.git_from_working_dir(&working_dir, &["commit", "-m", "Add utils from subdirectory"])
            .expect("Failed to commit from subdirectory");

        // Verify that the file was committed and has AI attribution
        let mut file = repo.filename("src/lib/utils.rs");
        file.assert_lines_and_blame(crate::lines![
            "pub fn helper() {".ai(),
            "    println!(\"hello\");".ai(),
            "}".ai(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn commit_with_mixed_files() {
        // Test committing files from both root and subdirectory
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut root_file = repo.filename("README.md");
    root_file.set_contents(crate::lines!["# Project".human()]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create file in subdirectory (AI-authored)
    let subdir_file_path = working_dir.join("main.rs");
    fs::write(&subdir_file_path, "fn main() {\n    println!(\"Hello, world!\");\n}\n").unwrap();

    // Create AI checkpoint for the file in subdirectory
    repo.git_ai(&["checkpoint", "mock_ai", "src/main.rs"]).unwrap();

    // Create file in root (human-authored)
    let root_file_path = repo.path().join("LICENSE");
    fs::write(&root_file_path, "MIT License\n").unwrap();

    // Stage both files
    repo.git(&["add", "src/main.rs", "LICENSE"]).unwrap();

    // Create human checkpoint
    repo.git_ai(&["checkpoint"]).unwrap(); // Human checkpoint for LICENSE

    // Commit (not using -C flag)
    // git-ai should automatically find the repository root
    repo.git_from_working_dir(&working_dir, &["commit", "-m", "Add files"])
        .expect("Failed to commit");

    // Verify AI attribution for subdirectory file
    let mut subdir_file = repo.filename("src/main.rs");
    subdir_file.assert_lines_and_blame(crate::lines![
        "fn main() {".ai(),
        "    println!(\"Hello, world!\");".ai(),
        "}".ai(),
    ]);

    // Verify human attribution for root file
    let mut license_file = repo.filename("LICENSE");
    license_file.assert_lines_and_blame(crate::lines![
        "MIT License".human(),
    ]);
    }
}

crate::subdir_test_variants! {
    fn commit_nested() {
        // Test committing from a deeply nested subdirectory
    let repo = TestRepo::new();

    // Create deeply nested subdirectory structure
    let working_dir = repo.path().join("a").join("b").join("c");
    fs::create_dir_all(&working_dir).unwrap();

    // Create initial commit
    let mut root_file = repo.filename("README.md");
    root_file.set_contents(crate::lines!["# Project".human()]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create file in nested subdirectory
    let nested_file_path = working_dir.join("deep.rs");
    fs::write(&nested_file_path, "pub mod deep {\n    pub fn func() {}\n}\n").unwrap();

    // Stage the file
    repo.git(&["add", "a/b/c/deep.rs"]).unwrap();

    // Create AI checkpoint
    repo.git_ai(&["checkpoint", "mock_ai", "a/b/c/deep.rs"]).unwrap();

    // Commit (not using -C flag)
    // git-ai should automatically find the repository root
    repo.git_from_working_dir(&working_dir, &["commit", "-m", "Add deep file"])
        .expect("Failed to commit");

    // Verify attribution
    let mut file = repo.filename("a/b/c/deep.rs");
    file.assert_lines_and_blame(crate::lines![
        "pub mod deep {".ai(),
        "    pub fn func() {}".ai(),
        "}".ai(),
    ]);
    }
}

crate::subdir_test_variants! {
    fn amend_add_lines() {
        // Test amending a commit by adding AI-authored lines
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("test.txt");

        // Initial file with human content
        file.set_contents(crate::lines!["line 1", "line 2", "line 3", "line 4", "line 5"]);

        repo.git(&["add", "-A"]).unwrap();

        repo.commit("Initial commit").unwrap();

        // AI adds lines at the top
        file.insert_at(
            0,
            crate::lines!["// AI added line 1".ai(), "// AI added line 2".ai()],
        );

        // Amend the commit (WITHOUT staging the AI lines)
        repo.git_from_working_dir(&working_dir, &["commit", "--amend", "-m", "Initial commit (amended)"])
            .expect("Amend should succeed");

        // Now stage and commit the AI lines
        repo.stage_all_and_commit("Add AI lines").unwrap();

        // Verify AI authorship is preserved after the second commit
        file.assert_lines_and_blame(crate::lines![
            "// AI added line 1".ai(),
            "// AI added line 2".ai(),
            "line 1".human(),
            "line 2".human(),
            "line 3".human(),
            "line 4".human(),
            "line 5".human()
        ]);
    }
}

crate::subdir_test_variants! {
    fn amend_add_lines_in_middle() {
        // Test amending a commit by adding AI-authored lines in the middle
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("test.txt");

        // Initial file with human content
        file.set_contents(crate::lines!["line 1", "line 2", "line 3", "line 4", "line 5"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        // AI adds lines in the middle
        file.insert_at(
            2,
            crate::lines!["// AI inserted line 1".ai(), "// AI inserted line 2".ai()],
        );

        // Amend the commit
        repo.git(&["add", "-A"]).unwrap();
        repo.git_from_working_dir(&working_dir, &["commit", "--amend", "-m", "Initial commit (amended)"])
            .expect("Amend should succeed");

        // Verify AI authorship is preserved
        file.assert_lines_and_blame(crate::lines![
            "line 1".human(),
            "line 2".human(),
            "// AI inserted line 1".ai(),
            "// AI inserted line 2".ai(),
            "line 3".human(),
            "line 4".human(),
            "line 5".human()
        ]);
    }
}

crate::subdir_test_variants! {
    fn amend_multiple_changes() {
        // Test amending with multiple AI changes
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("code.js");

        // Initial file with AI content
        file.set_contents(crate::lines![
            "function example() {".ai(),
            "  return 42;".ai(),
            "}".ai()
        ]);
        repo.stage_all_and_commit("Add example function").unwrap();

        // AI adds header comment
        file.insert_at(0, crate::lines!["// Header comment".ai()]);
        // After inserting at 0, the file now has 4 lines

        // AI adds documentation in middle (after line 2: "function example() {")
        file.insert_at(2, crate::lines!["  // Added documentation".ai()]);
        // After inserting at 2, the file now has 5 lines

        // AI adds footer at bottom (at the end after "}")
        file.insert_at(5, crate::lines!["// Footer".ai()]);

        // Amend the commit
        repo.git(&["add", "-A"]).unwrap();
        repo.git_from_working_dir(&working_dir, &["commit", "--amend", "-m", "Add example function (amended)"])
            .expect("Amend should succeed");

        // Verify all AI authorship is preserved
        file.assert_lines_and_blame(crate::lines![
            "// Header comment".ai(),
            "function example() {".ai(),
            "  // Added documentation".ai(),
            "  return 42;".ai(),
            "}".ai(),
            "// Footer".ai()
        ]);
    }
}

crate::subdir_test_variants! {
    fn amend_with_unstaged_ai_code() {
        // Test amending with unstaged AI code in other file
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit with fileA
        let mut file_a = repo.filename("fileA.txt");
        file_a.set_contents(crate::lines!["fileA line 1", "fileA line 2"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        // Create fileB with AI code but DON'T stage it yet
        let mut file_b = repo.filename("fileB.txt");
        file_b.set_contents_no_stage(crate::lines![
            "// AI code in fileB".ai(),
            "function foo() {".ai(),
            "  return 'bar';".ai(),
            "}".ai()
        ]);

        // Modify fileA and amend the previous commit (fileB stays unstaged in working tree)
        file_a.insert_at(2, crate::lines!["fileA line 3"]);
        repo.git(&["add", "fileA.txt"]).unwrap();
        repo.git_from_working_dir(&working_dir, &["commit", "--amend", "-m", "Initial commit (amended)"])
            .expect("Amend should succeed");

        // Now stage and commit fileB in a new commit
        repo.stage_all_and_commit("Add fileB").unwrap();

        // Verify fileB has AI authorship
        file_b.assert_lines_and_blame(crate::lines![
            "// AI code in fileB".ai(),
            "function foo() {".ai(),
            "  return 'bar';".ai(),
            "}".ai()
        ]);
    }
}

crate::subdir_test_variants! {
    fn amend_nested() {
        // Test amending a commit when run from a deeply nested subdirectory
        let repo = TestRepo::new();

        // Create deeply nested subdirectory structure
        let working_dir = repo.path().join("a").join("b").join("c");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("test.txt");

        // Initial file with human content
        file.set_contents(crate::lines!["line 1", "line 2", "line 3"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        // AI adds lines at the bottom
        file.insert_at(
            3,
            crate::lines!["// AI appended line 1".ai(), "// AI appended line 2".ai()],
        );

        // Amend the commit
        repo.git(&["add", "-A"]).unwrap();
        repo.git_from_working_dir(&working_dir, &["commit", "--amend", "-m", "Initial commit (amended)"])
            .expect("Amend should succeed");

        // Verify AI authorship is preserved
        file.assert_lines_and_blame(crate::lines![
            "line 1".human(),
            "line 2".human(),
            "line 3".ai(),
            "// AI appended line 1".ai(),
            "// AI appended line 2".ai()
        ]);
    }
}

crate::subdir_test_variants! {
    fn amend_preserves_unstaged_ai_attribution() {
        // Test that unstaged AI code in the tree is attributed after amending HEAD
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("components");
        fs::create_dir_all(&working_dir).unwrap();

        // Create initial commit with fileA
        let mut file_a = repo.filename("fileA.txt");
        file_a.set_contents(crate::lines!["original content"]);
        repo.stage_all_and_commit("Initial commit").unwrap();

        // Stage changes to fileA
        file_a.insert_at(1, crate::lines!["staged addition"]);
        repo.git(&["add", "fileA.txt"]).unwrap();

        // Create fileB with unstaged AI code
        let mut file_b = repo.filename("fileB.txt");
        file_b.set_contents_no_stage(crate::lines![
            "// Unstaged AI line 1".ai(),
            "// Unstaged AI line 2".ai(),
            "// Unstaged AI line 3".ai()
        ]);

        // Amend HEAD with fileA (fileB remains unstaged)
        repo.git_from_working_dir(&working_dir, &["commit", "--amend", "-m", "Amended commit"])
            .expect("Amend should succeed");

        // Verify that fileB's AI attribution was saved in INITIAL attributions
        let initial = repo.current_working_logs().read_initial_attributions();
        assert!(
            initial.files.contains_key("fileB.txt"),
            "fileB.txt should be in initial attributions"
        );
        let file_b_attrs = &initial.files["fileB.txt"];
        assert_eq!(
            file_b_attrs.len(),
            1,
            "fileB should have 1 attribution range"
        );
        assert_eq!(file_b_attrs[0].start_line, 1);
        assert_eq!(file_b_attrs[0].end_line, 3);

        // Now stage and commit fileB
        repo.stage_all_and_commit("Add fileB").unwrap();

        // Verify fileB retains AI authorship
        file_b.assert_lines_and_blame(crate::lines![
            "// Unstaged AI line 1".ai(),
            "// Unstaged AI line 2".ai(),
            "// Unstaged AI line 3".ai()
        ]);
    }
}
