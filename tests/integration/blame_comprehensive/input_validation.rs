use super::{ExpectedLineExt, TestRepo};

// =============================================================================
// Error Handling Tests - Invalid inputs, missing files, git errors
// =============================================================================

#[test]
fn test_blame_error_missing_file() {
    // Error case: Blame on non-existent file
    let repo = TestRepo::new();

    let result = repo.git_ai(&["blame", "nonexistent.txt"]);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("File not found")
            || err.contains("does not exist")
            || err.contains("No such file")
            || err.contains("pathspec")
            || err.contains("did not match")
            || err.contains("cannot find the file")
            || err.contains("canonicalize file path"),
        "Expected error about missing file, got: {}",
        err
    );
}

#[test]
fn test_blame_error_invalid_line_range_start_zero() {
    // Error case: Line range starting at 0 (lines are 1-indexed)
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let result = repo.git_ai(&["blame", "-L", "0,1", "test.txt"]);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Invalid line range"));
}

#[test]
fn test_blame_error_invalid_line_range_end_zero() {
    // Error case: Line range ending at 0
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let result = repo.git_ai(&["blame", "-L", "1,0", "test.txt"]);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Invalid line range"));
}

#[test]
fn test_blame_error_invalid_line_range_start_greater_than_end() {
    // Error case: Start line > end line
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let result = repo.git_ai(&["blame", "-L", "3,1", "test.txt"]);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Invalid line range"));
}

#[test]
fn test_blame_error_invalid_line_range_beyond_file() {
    // Error case: Line range exceeds file length
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let result = repo.git_ai(&["blame", "-L", "1,100", "test.txt"]);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Invalid line range") && err.contains("File has 2 lines"));
}

#[test]
fn test_blame_error_invalid_commit_ref() {
    // Error case: Invalid commit SHA
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let result = repo.git_ai(&["blame", "invalid_sha_123", "test.txt"]);

    assert!(result.is_err());
}

#[test]
fn test_blame_error_file_outside_repo() {
    // Error case: Attempt to blame a file outside the repository
    let repo = TestRepo::new();

    // Use a unique temp dir per test instance to avoid races when the
    // worktree variant of this test runs concurrently in the same process.
    let outside_dir = tempfile::tempdir().expect("failed to create temp dir");
    let outside_file = outside_dir.path().join("outside.txt");
    std::fs::write(&outside_file, "outside content").unwrap();

    let result = repo.git_ai(&["blame", outside_file.to_str().unwrap()]);

    assert!(
        result.is_err(),
        "blaming a file outside the repo should fail"
    );
    // On Windows in worktree mode, both the worktree and the outside file reside
    // under the same temp directory.  UNC-path canonicalization (`\\?\…`) can
    // cause `strip_prefix` to behave differently, producing an error message that
    // does not contain the usual "not within repository root" text.  The important
    // invariant is that the command errors out; we only assert the specific message
    // on platforms where it is stable.
    #[cfg(not(target_os = "windows"))]
    {
        let err = result.unwrap_err();
        assert!(
            err.contains("not within repository root"),
            "unexpected error message: {err}"
        );
    }
}

#[test]
fn test_blame_error_directory_instead_of_file() {
    // Error case: Attempt to blame a directory
    let repo = TestRepo::new();

    let subdir = repo.path().join("src");
    std::fs::create_dir_all(&subdir).unwrap();

    let result = repo.git_ai(&["blame", "src"]);

    assert!(result.is_err());
}

// =============================================================================
// Path Normalization Tests - Absolute vs relative paths
// =============================================================================

#[test]
fn test_blame_path_normalization_absolute() {
    // Path normalization: Absolute path should be converted to relative
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Content".ai()]);
    repo.stage_all_and_commit("Test").unwrap();

    let abs_path = repo.path().join("test.txt");
    let output = repo.git_ai(&["blame", abs_path.to_str().unwrap()]).unwrap();

    assert!(output.contains("Content"));
}

#[test]
fn test_blame_path_normalization_relative() {
    // Path normalization: Relative path should work
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Content".ai()]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo.git_ai(&["blame", "test.txt"]).unwrap();

    assert!(output.contains("Content"));
}

#[test]
fn test_blame_path_normalization_subdirectory() {
    // Path normalization: File in subdirectory
    let repo = TestRepo::new();

    let subdir = repo.path().join("src");
    std::fs::create_dir_all(&subdir).unwrap();

    let mut file = repo.filename("src/code.rs");
    file.set_contents(crate::lines!["fn main() {}".ai()]);
    repo.stage_all_and_commit("Add code").unwrap();

    let output = repo.git_ai(&["blame", "src/code.rs"]).unwrap();

    assert!(output.contains("fn main()"));
}

crate::reuse_tests_in_worktree!(
    test_blame_error_missing_file,
    test_blame_error_invalid_line_range_start_zero,
    test_blame_error_invalid_line_range_end_zero,
    test_blame_error_invalid_line_range_start_greater_than_end,
    test_blame_error_invalid_line_range_beyond_file,
    test_blame_error_invalid_commit_ref,
    test_blame_error_file_outside_repo,
    test_blame_error_directory_instead_of_file,
    test_blame_path_normalization_absolute,
    test_blame_path_normalization_relative,
    test_blame_path_normalization_subdirectory,
);
