use super::should_ignore_file;

#[test]
fn test_should_ignore_file_with_patterns() {
    let lockfile_patterns = vec![
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
        "Cargo.lock".to_string(),
        "go.sum".to_string(),
    ];

    // Test that specified patterns are ignored
    assert!(should_ignore_file("package-lock.json", &lockfile_patterns));
    assert!(should_ignore_file("yarn.lock", &lockfile_patterns));
    assert!(should_ignore_file("Cargo.lock", &lockfile_patterns));
    assert!(should_ignore_file("go.sum", &lockfile_patterns));

    // Test with paths
    assert!(should_ignore_file(
        "src/package-lock.json",
        &lockfile_patterns
    ));
    assert!(should_ignore_file("backend/Cargo.lock", &lockfile_patterns));
    assert!(should_ignore_file("./yarn.lock", &lockfile_patterns));

    // Test that non-matching files are not ignored
    assert!(!should_ignore_file("package.json", &lockfile_patterns));
    assert!(!should_ignore_file("Cargo.toml", &lockfile_patterns));
    assert!(!should_ignore_file("src/main.rs", &lockfile_patterns));
    assert!(!should_ignore_file("pnpm-lock.yaml", &lockfile_patterns)); // Not in our pattern list

    // Test with empty patterns - nothing should be ignored
    let empty_patterns: Vec<String> = vec![];
    assert!(!should_ignore_file("package-lock.json", &empty_patterns));
    assert!(!should_ignore_file("Cargo.lock", &empty_patterns));
}

#[test]
fn test_should_ignore_file_with_glob_patterns() {
    // Test wildcard patterns
    let wildcard_patterns = vec!["*.lock".to_string()];

    // Should match any file ending in .lock
    assert!(should_ignore_file("Cargo.lock", &wildcard_patterns));
    assert!(should_ignore_file("package.lock", &wildcard_patterns));
    assert!(should_ignore_file("yarn.lock", &wildcard_patterns));
    assert!(should_ignore_file("src/Cargo.lock", &wildcard_patterns));
    assert!(should_ignore_file("backend/deps.lock", &wildcard_patterns));

    // Should not match files not ending in .lock
    assert!(!should_ignore_file("Cargo.toml", &wildcard_patterns));
    assert!(!should_ignore_file("lock.txt", &wildcard_patterns));
    assert!(!should_ignore_file("locked.rs", &wildcard_patterns));

    // Test multiple wildcards
    let multi_wildcard = vec!["*.lock".to_string(), "*.generated.*".to_string()];
    assert!(should_ignore_file("test.generated.js", &multi_wildcard));
    assert!(should_ignore_file("api.generated.ts", &multi_wildcard));
    assert!(should_ignore_file("schema.lock", &multi_wildcard));
    assert!(!should_ignore_file("manual.js", &multi_wildcard));
}

#[test]
fn test_should_ignore_file_with_path_glob_patterns() {
    // Test path-based patterns
    let path_patterns = vec!["**/target/**".to_string()];

    // Should match files in target directory at any depth
    assert!(should_ignore_file("target/debug/foo", &path_patterns));
    assert!(should_ignore_file(
        "backend/target/release/bar",
        &path_patterns
    ));
    assert!(should_ignore_file("project/target/file.rs", &path_patterns));

    // Should not match files outside target
    assert!(!should_ignore_file("src/target.rs", &path_patterns));
    assert!(!should_ignore_file("target.txt", &path_patterns));

    // Test specific directory patterns
    let dir_patterns = vec!["node_modules/**".to_string()];
    assert!(should_ignore_file(
        "node_modules/package/index.js",
        &dir_patterns
    ));
    assert!(should_ignore_file("node_modules/foo.js", &dir_patterns));
    assert!(!should_ignore_file("src/node_modules.rs", &dir_patterns));
}

#[test]
fn test_should_ignore_file_with_prefix_patterns() {
    // Test prefix patterns
    let prefix_patterns = vec!["generated-*".to_string()];

    assert!(should_ignore_file("generated-api.ts", &prefix_patterns));
    assert!(should_ignore_file("generated-schema.js", &prefix_patterns));
    assert!(should_ignore_file(
        "src/generated-types.d.ts",
        &prefix_patterns
    ));
    assert!(!should_ignore_file("api-generated.ts", &prefix_patterns));
    assert!(!should_ignore_file("manual.ts", &prefix_patterns));
}

#[test]
fn test_should_ignore_file_with_complex_glob_patterns() {
    // Test complex patterns (note: brace expansion like {js,ts} is not supported by glob crate)
    let complex_patterns = vec![
        "**/*.generated.js".to_string(),
        "**/*.generated.ts".to_string(),
        "*-lock.*".to_string(),
        "dist/**".to_string(),
    ];

    // Glob patterns with multiple wildcards
    assert!(should_ignore_file(
        "src/api.generated.js",
        &complex_patterns
    ));
    assert!(should_ignore_file("types.generated.ts", &complex_patterns));
    assert!(should_ignore_file("package-lock.json", &complex_patterns));
    assert!(should_ignore_file("yarn-lock.yaml", &complex_patterns));
    assert!(should_ignore_file("dist/bundle.js", &complex_patterns));
    assert!(should_ignore_file(
        "dist/nested/file.css",
        &complex_patterns
    ));

    assert!(!should_ignore_file("src/manual.js", &complex_patterns));
    assert!(!should_ignore_file("lock.txt", &complex_patterns));
}

#[test]
fn test_should_ignore_file_mixed_exact_and_glob() {
    // Test mixing exact matches and glob patterns
    let mixed_patterns = vec![
        "Cargo.lock".to_string(),        // Exact match
        "*.generated.js".to_string(),    // Glob pattern
        "package-lock.json".to_string(), // Exact match
        "**/target/**".to_string(),      // Path glob
    ];

    // Exact matches
    assert!(should_ignore_file("Cargo.lock", &mixed_patterns));
    assert!(should_ignore_file("package-lock.json", &mixed_patterns));

    // Glob matches
    assert!(should_ignore_file("api.generated.js", &mixed_patterns));
    assert!(should_ignore_file("target/debug/foo", &mixed_patterns));

    // Non-matches
    assert!(!should_ignore_file("Cargo.toml", &mixed_patterns));
    assert!(!should_ignore_file("manual.js", &mixed_patterns));
}

#[test]
fn test_should_ignore_file_case_sensitivity() {
    // Test that pattern matching is case-sensitive
    let patterns = vec!["Cargo.lock".to_string(), "*.LOG".to_string()];

    // Exact case matches
    assert!(should_ignore_file("Cargo.lock", &patterns));
    assert!(should_ignore_file("file.LOG", &patterns));
    assert!(should_ignore_file("debug.LOG", &patterns));

    // Different case should NOT match (case-sensitive)
    assert!(!should_ignore_file("cargo.lock", &patterns));
    assert!(!should_ignore_file("CARGO.LOCK", &patterns));
    assert!(!should_ignore_file("file.log", &patterns));
    assert!(!should_ignore_file("file.Log", &patterns));
}

#[test]
fn test_should_ignore_file_special_characters() {
    // Test filenames with special characters
    let patterns = vec![
        "file with spaces.txt".to_string(),
        "*.lock".to_string(),
        "file-with-dashes.js".to_string(),
        "file_with_underscores.rs".to_string(),
    ];

    // Files with spaces
    assert!(should_ignore_file("file with spaces.txt", &patterns));
    assert!(should_ignore_file(
        "path/to/file with spaces.txt",
        &patterns
    ));

    // Files with dashes and underscores
    assert!(should_ignore_file("file-with-dashes.js", &patterns));
    assert!(should_ignore_file("file_with_underscores.rs", &patterns));

    // Glob should still work with special chars in other files
    assert!(should_ignore_file("my-package.lock", &patterns));
    assert!(should_ignore_file("test_file.lock", &patterns));

    // Non-matches
    assert!(!should_ignore_file("file with spaces.js", &patterns));
    assert!(!should_ignore_file("different-file.txt", &patterns));
}

#[test]
fn test_should_ignore_file_hidden_files() {
    // Test hidden files (starting with .)
    let patterns = vec![".env".to_string(), ".*.swp".to_string(), ".*rc".to_string()];

    // Hidden files
    assert!(should_ignore_file(".env", &patterns));
    assert!(should_ignore_file("config/.env", &patterns));

    // Vim swap files
    assert!(should_ignore_file(".file.swp", &patterns));
    assert!(should_ignore_file(".main.rs.swp", &patterns));

    // RC files
    assert!(should_ignore_file(".bashrc", &patterns));
    assert!(should_ignore_file(".vimrc", &patterns));
    assert!(should_ignore_file("home/.npmrc", &patterns));

    // Non-matches
    assert!(!should_ignore_file("env", &patterns));
    assert!(!should_ignore_file("file.swp", &patterns));
    assert!(!should_ignore_file("bashrc", &patterns));
}

#[test]
fn test_should_ignore_file_multiple_extensions() {
    // Test files with multiple extensions
    let patterns = vec![
        "*.tar.gz".to_string(),
        "*.min.js".to_string(),
        "*.d.ts".to_string(),
    ];

    // Multiple extensions
    assert!(should_ignore_file("archive.tar.gz", &patterns));
    assert!(should_ignore_file("bundle.min.js", &patterns));
    assert!(should_ignore_file("types.d.ts", &patterns));
    assert!(should_ignore_file("build/dist/app.min.js", &patterns));

    // Partial matches should not match
    assert!(!should_ignore_file("file.tar", &patterns));
    assert!(!should_ignore_file("file.gz", &patterns));
    assert!(!should_ignore_file("file.js", &patterns));
    assert!(!should_ignore_file("types.ts", &patterns));
}

#[test]
fn test_should_ignore_file_no_extension() {
    // Test files without extensions
    let patterns = vec![
        "Makefile".to_string(),
        "Dockerfile".to_string(),
        "LICENSE".to_string(),
        "README".to_string(),
    ];

    // Files without extensions
    assert!(should_ignore_file("Makefile", &patterns));
    assert!(should_ignore_file("Dockerfile", &patterns));
    assert!(should_ignore_file("LICENSE", &patterns));
    assert!(should_ignore_file("README", &patterns));

    // In subdirectories
    assert!(should_ignore_file("project/Makefile", &patterns));
    assert!(should_ignore_file("docker/Dockerfile", &patterns));

    // Similar names should not match
    assert!(!should_ignore_file("Makefile.old", &patterns));
    assert!(!should_ignore_file("README.md", &patterns));
    assert!(!should_ignore_file("LICENSE.txt", &patterns));
}

#[test]
fn test_should_ignore_file_deeply_nested_paths() {
    // Test patterns at various nesting depths
    let patterns = vec![
        "**/node_modules/**".to_string(),
        "**/build/**".to_string(),
        "**/.git/**".to_string(),
    ];

    // Deep nesting
    assert!(should_ignore_file(
        "node_modules/package/index.js",
        &patterns
    ));
    assert!(should_ignore_file("a/b/c/node_modules/d/e/f.js", &patterns));
    assert!(should_ignore_file(
        "project/build/output/bundle.js",
        &patterns
    ));
    assert!(should_ignore_file(".git/objects/ab/cdef123", &patterns));
    assert!(should_ignore_file("repo/.git/hooks/pre-commit", &patterns));

    // Should not match similar names outside pattern
    assert!(!should_ignore_file("src/node_modules.js", &patterns));
    assert!(!should_ignore_file("build.sh", &patterns));
    assert!(!should_ignore_file("git.txt", &patterns));
}

#[test]
fn test_should_ignore_file_partial_matches() {
    // Test that partial matches don't incorrectly match
    let patterns = vec!["lock".to_string(), "*.lock".to_string()];

    // Should match
    assert!(should_ignore_file("lock", &patterns));
    assert!(should_ignore_file("file.lock", &patterns));
    assert!(should_ignore_file("package.lock", &patterns));

    // Should NOT match (lock is substring but not filename or extension)
    assert!(!should_ignore_file("locked.txt", &patterns));
    assert!(!should_ignore_file("unlock.sh", &patterns));
    assert!(!should_ignore_file("locksmith.rs", &patterns));
}

#[test]
fn test_should_ignore_file_with_wildcards_in_middle() {
    // Test patterns with wildcards in the middle
    let patterns = vec!["test-*-output.log".to_string(), "backup-*.sql".to_string()];

    // Should match
    assert!(should_ignore_file("test-123-output.log", &patterns));
    assert!(should_ignore_file("test-foo-output.log", &patterns));
    assert!(should_ignore_file("backup-daily.sql", &patterns));
    assert!(should_ignore_file("backup-2024-01-01.sql", &patterns));
    assert!(should_ignore_file("logs/test-debug-output.log", &patterns));

    // Should not match
    assert!(!should_ignore_file("test-output.log", &patterns));
    assert!(!should_ignore_file("test-123-result.log", &patterns));
    assert!(!should_ignore_file("backup.sql", &patterns));
}

#[test]
fn test_should_ignore_file_empty_pattern() {
    // Test with empty pattern string - empty pattern is technically valid glob
    // that matches empty string, but we test that non-empty files don't match
    let patterns = vec!["".to_string(), "*.lock".to_string()];

    // Regular files should not match the empty pattern
    assert!(!should_ignore_file("file.txt", &patterns));
    assert!(!should_ignore_file("src/main.rs", &patterns));

    // But valid patterns should still work
    assert!(should_ignore_file("file.lock", &patterns));
    assert!(should_ignore_file("package.lock", &patterns));
}

#[test]
fn test_should_ignore_file_directory_traversal() {
    // Test patterns with ../ or ./ in paths
    let patterns = vec!["*.lock".to_string()];

    // Should match regardless of ./ prefix
    assert!(should_ignore_file("./file.lock", &patterns));
    assert!(should_ignore_file("./path/to/file.lock", &patterns));

    // Complex paths
    assert!(should_ignore_file("src/../lib/file.lock", &patterns));
}

#[test]
fn test_should_ignore_file_numeric_filenames() {
    // Test numeric filenames
    let patterns = vec!["[0-9]*".to_string(), "*.123".to_string()];

    // Filenames starting with numbers
    assert!(should_ignore_file("123.txt", &patterns));
    assert!(should_ignore_file("456file.log", &patterns));
    assert!(should_ignore_file("7890.rs", &patterns));

    // Files ending with .123
    assert!(should_ignore_file("backup.123", &patterns));
    assert!(should_ignore_file("data.123", &patterns));

    // Should not match
    assert!(!should_ignore_file("file123.txt", &patterns));
    assert!(!should_ignore_file("test.456", &patterns));
}
