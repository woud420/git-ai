//! Unit tests for `GIT_AI_ALLOWED_REPOSITORIES` parsing and its union with
//! the config file / test patch in `build_config()`.
//!
//! Kept in a dedicated file rather than the shared `config::tests` module:
//! that module is at its file-length ceiling (see `.file-length-baseline.txt`),
//! with no headroom for new tests.

use glob::Pattern;

use super::build_config;
use super::patterns::{apply_env_allowed_repositories, parse_env_allowed_repositories};

#[test]
fn parse_env_allowed_repositories_splits_trims_and_drops_empty_segments() {
    let result =
        parse_env_allowed_repositories(" https://github.com/org/repo , , /nonexistent/path ,  ");
    assert_eq!(
        result,
        vec![
            "https://github.com/org/repo".to_string(),
            "/nonexistent/path".to_string(),
        ]
    );
}

#[test]
fn parse_env_allowed_repositories_returns_empty_for_blank_value() {
    assert!(parse_env_allowed_repositories("").is_empty());
    assert!(parse_env_allowed_repositories("   ,  ,\t").is_empty());
}

#[test]
fn parse_env_allowed_repositories_leaves_url_and_glob_patterns_untouched() {
    let result =
        parse_env_allowed_repositories("*,https://github.com/org/*,git@github.com:org/repo.git");
    assert_eq!(
        result,
        vec![
            "*".to_string(),
            "https://github.com/org/*".to_string(),
            "git@github.com:org/repo.git".to_string(),
        ]
    );
}

#[test]
fn parse_env_allowed_repositories_canonicalizes_existing_path_entries() {
    // `std::env::temp_dir()` is exactly the canonicalization trap this
    // resolves: on macOS it returns a symlinked path (e.g.
    // /var/folders/... -> /private/var/folders/...), and repo roots are
    // always matched against their canonicalized form
    // (`repo_root_matches_patterns`).
    let dir = std::env::temp_dir();
    let Ok(canonical) = dir.canonicalize() else {
        // Not every sandbox guarantees a canonicalizable temp dir; skip if so.
        return;
    };
    let raw = dir.to_string_lossy().to_string();
    let result = parse_env_allowed_repositories(&raw);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0],
        crate::utils::normalize_to_posix(&canonical.to_string_lossy())
    );
}

#[test]
#[serial_test::serial]
fn apply_env_allowed_repositories_is_noop_without_the_env_var() {
    let previous = std::env::var("GIT_AI_ALLOWED_REPOSITORIES").ok();
    unsafe { std::env::remove_var("GIT_AI_ALLOWED_REPOSITORIES") };

    let mut current: Vec<Pattern> = vec![Pattern::new("/already/allowed").unwrap()];
    apply_env_allowed_repositories(&mut current);

    if let Some(v) = previous {
        unsafe { std::env::set_var("GIT_AI_ALLOWED_REPOSITORIES", v) };
    }
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].as_str(), "/already/allowed");
}

#[test]
#[serial_test::serial]
fn apply_env_allowed_repositories_unions_with_existing_file_derived_patterns() {
    let previous = std::env::var("GIT_AI_ALLOWED_REPOSITORIES").ok();
    unsafe {
        std::env::set_var(
            "GIT_AI_ALLOWED_REPOSITORIES",
            "https://github.com/org/from-env",
        );
    }

    let mut current: Vec<Pattern> = vec![Pattern::new("https://github.com/org/from-file").unwrap()];
    apply_env_allowed_repositories(&mut current);

    match previous {
        Some(v) => unsafe { std::env::set_var("GIT_AI_ALLOWED_REPOSITORIES", v) },
        None => unsafe { std::env::remove_var("GIT_AI_ALLOWED_REPOSITORIES") },
    }

    let as_strings: Vec<&str> = current.iter().map(Pattern::as_str).collect();
    assert_eq!(
        as_strings,
        vec![
            "https://github.com/org/from-file",
            "https://github.com/org/from-env",
        ],
        "env patterns must be unioned onto (appended after) the existing file-derived list"
    );
}

#[test]
#[serial_test::serial]
fn apply_env_allowed_repositories_does_not_duplicate_an_already_present_pattern() {
    let previous = std::env::var("GIT_AI_ALLOWED_REPOSITORIES").ok();
    unsafe {
        std::env::set_var("GIT_AI_ALLOWED_REPOSITORIES", "https://github.com/org/dup");
    }

    let mut current: Vec<Pattern> = vec![Pattern::new("https://github.com/org/dup").unwrap()];
    apply_env_allowed_repositories(&mut current);

    match previous {
        Some(v) => unsafe { std::env::set_var("GIT_AI_ALLOWED_REPOSITORIES", v) },
        None => unsafe { std::env::remove_var("GIT_AI_ALLOWED_REPOSITORIES") },
    }
    assert_eq!(
        current.len(),
        1,
        "duplicate env pattern must not be added again"
    );
}

#[test]
#[serial_test::serial]
fn build_config_unions_env_allowed_repositories_over_a_test_patch_that_clears_the_file_list() {
    // This depends on a real git binary being findable, same as the
    // analogous GIT_AI_NOTES_BACKEND_KIND env-var test in `config::tests`.
    let previous_env = std::env::var("GIT_AI_ALLOWED_REPOSITORIES").ok();
    let previous_patch = std::env::var("GIT_AI_TEST_CONFIG_PATCH").ok();
    unsafe {
        std::env::set_var(
            "GIT_AI_ALLOWED_REPOSITORIES",
            "https://github.com/org/env-only",
        );
        std::env::set_var("GIT_AI_TEST_CONFIG_PATCH", r#"{"allowed_repositories":[]}"#);
    }

    let cfg = build_config();
    let has_env_pattern = cfg
        .allowed_repositories
        .iter()
        .any(|p| p.as_str() == "https://github.com/org/env-only");

    match previous_env {
        Some(v) => unsafe { std::env::set_var("GIT_AI_ALLOWED_REPOSITORIES", v) },
        None => unsafe { std::env::remove_var("GIT_AI_ALLOWED_REPOSITORIES") },
    }
    match previous_patch {
        Some(v) => unsafe { std::env::set_var("GIT_AI_TEST_CONFIG_PATCH", v) },
        None => unsafe { std::env::remove_var("GIT_AI_TEST_CONFIG_PATCH") },
    }

    assert!(
        has_env_pattern,
        "GIT_AI_ALLOWED_REPOSITORIES must be unioned in even when the test patch clears \
         allowed_repositories to an empty list"
    );
}
