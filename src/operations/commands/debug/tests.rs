use super::capture::*;
use super::diagnostics::*;
use super::formatting::*;
use super::system::*;
use crate::operations::git::repository::{
    GitAuthorIdentity, GitConfigIdentityResolution, GitIdentityResolution,
};
use crate::process_spawn::format_posix_shell_command as format_command_for_error;
use crate::process_timeout::TimedCommandOutput;
use std::time::Duration;
#[test]
fn indented_blocks_preserve_empty_and_line_boundaries() {
    for (input, expected) in [
        ("", "  <empty>\n"),
        (" \t\n", "  <empty>\n"),
        ("first", "  first\n"),
        ("first\nsecond\n", "  first\n  second\n"),
        ("first\n\n", "  first\n  \n"),
        (" first \r\nsecond", "   first \n  second\n"),
    ] {
        let mut actual = String::new();
        super::append_indented_block(&mut actual, input);
        assert_eq!(actual, expected);
        let mut prefixed = String::new();
        append_indented_block_with_prefix(&mut prefixed, input, "  ");
        assert_eq!(prefixed, expected);
    }
}

use super::*;

#[test]
fn format_command_for_error_preserves_posix_shell_words() {
    assert_eq!(format_command_for_error("g", &["a b"]), "g 'a b'");
}

#[test]
fn test_redact_git_config_line_redacts_sensitive_key() {
    let line =
        "global\tfile:/Users/me/.gitconfig\thttp.https://example.com/.extraheader=AUTH token";
    let redacted = redact_git_config_line(line);
    assert_eq!(
        redacted,
        "global\tfile:/Users/me/.gitconfig\thttp.https://example.com/.extraheader=[REDACTED]"
    );
}

#[test]
fn test_redact_git_config_line_keeps_non_sensitive_key() {
    let line = "global\tfile:/Users/me/.gitconfig\tcore.editor=vim";
    let redacted = redact_git_config_line(line);
    assert_eq!(redacted, line);
}

#[test]
fn test_redact_git_config_line_two_field_format_redacts_sensitive() {
    // `git config --list --show-origin` (without --show-scope) produces 2-tab fields
    let line = "file:/Users/me/.gitconfig\thttp.https://example.com/.extraheader=BEARER secret123";
    let redacted = redact_git_config_line(line);
    assert_eq!(
        redacted,
        "file:/Users/me/.gitconfig\thttp.https://example.com/.extraheader=[REDACTED]"
    );
}

#[test]
fn test_redact_git_config_line_two_field_format_keeps_non_sensitive() {
    let line = "file:/Users/me/.gitconfig\tcore.editor=vim";
    let redacted = redact_git_config_line(line);
    assert_eq!(redacted, line);
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(1024), "1.00 KB (1024 bytes)");
}

#[test]
fn test_is_debug_git_env_key_matches_git_prefixes() {
    assert!(is_debug_git_env_key("GIT_AI_DEBUG"));
    assert!(is_debug_git_env_key("GITAI_TEST_DB_PATH"));
    assert!(is_debug_git_env_key("GIT_DIR"));
    assert!(is_debug_git_env_key("GIT_TRACE2_EVENT"));
    assert!(!is_debug_git_env_key("GITHUB_TOKEN"));
    assert!(!is_debug_git_env_key("PATH"));
}

#[test]
fn test_collect_git_environment_entries_sorts_and_redacts() {
    let entries = collect_git_environment_entries(vec![
        ("OTHER".to_string(), "ignored".to_string()),
        ("GIT_DIR".to_string(), ".git".to_string()),
        ("GITAI_TEST_DB_PATH".to_string(), "/tmp/db".to_string()),
        ("GIT_AI_API_KEY".to_string(), "secret".to_string()),
    ]);

    assert_eq!(
        entries,
        vec![
            "GITAI_TEST_DB_PATH=/tmp/db",
            "GIT_AI_API_KEY=[REDACTED]",
            "GIT_DIR=.git",
        ]
    );
}

#[test]
fn test_parse_git_version_handles_platform_suffixes() {
    assert_eq!(
        parse_git_version("git version 2.54.0.windows.1"),
        Some(GitVersion {
            major: 2,
            minor: 54,
            patch: 0
        })
    );
    assert_eq!(
        parse_git_version("git version 2.39.5 (Apple Git-154)"),
        Some(GitVersion {
            major: 2,
            minor: 39,
            patch: 5
        })
    );
}

#[test]
fn test_parse_git_version_accepts_minimum_version() {
    assert!(parse_git_version("git version 2.22.0").unwrap() >= MIN_GIT_VERSION);
    assert!(parse_git_version("git version 2.21.9").unwrap() < MIN_GIT_VERSION);
}

#[test]
fn test_select_lookup_path_prefers_existing_path() {
    let exe = env::current_exe().unwrap();
    let output = format!("/definitely/not/git\n{}\n", exe.display());

    assert_eq!(
        select_lookup_path(&output).unwrap(),
        exe.display().to_string()
    );
}

#[test]
fn test_select_lookup_path_falls_back_to_first_non_empty_line() {
    assert_eq!(
        select_lookup_path("\n git: aliased to hub \n").unwrap(),
        "git: aliased to hub"
    );
}

#[test]
fn test_realpath_for_display_canonicalizes_existing_path() {
    let exe = env::current_exe().unwrap();
    let expected = fs::canonicalize(&exe).unwrap();

    assert_eq!(
        realpath_for_display(&exe.display().to_string()),
        expected.display().to_string()
    );
}

#[test]
fn test_capture_result_preserves_partial_timeout_output() {
    // Regression coverage for ENG-340.
    let timeout = Duration::from_millis(300);
    let output = crate::process_timeout::partial_output_fixture(timeout).unwrap();
    let err = capture_result("partial-output fixture", timeout, output).unwrap_err();

    assert!(err.contains("timed out after 0.3s"), "{err}");
    assert!(
        err.contains("sent kill to child process") || err.contains("failed to kill child process"),
        "{err}"
    );
    assert!(err.contains("; stdout before timeout: out"), "{err}");
    assert!(err.contains("; stderr before timeout: err"), "{err}");
}

#[test]
fn test_parse_debug_options_accepts_skip_trace2_checks() {
    let options = parse_debug_options(&[SKIP_TRACE2_CHECKS_FLAG.to_string()]).unwrap();
    assert!(options.skip_trace2_checks);
}

#[test]
fn test_parse_debug_options_rejects_unknown_arg() {
    let err = parse_debug_options(&["--wat".to_string()]).unwrap_err();
    assert!(err.contains("unknown debug argument: --wat"), "{err}");
}

#[test]
fn test_append_git_committer_identity_includes_effective_author() {
    let identity = GitCommitterIdentityInfo {
        global_config: Ok(GitConfigIdentityResolution {
            raw_name: Some("Git User".to_string()),
            raw_email: Some("git@example.com".to_string()),
            identity: GitAuthorIdentity {
                name: Some("Git User".to_string()),
                email: Some("git@example.com".to_string()),
            },
        }),
        repository: RepositoryCommitterIdentity::InRepository(GitIdentityResolution {
            raw_git_var: Some("Git User <git@example.com> 1234567890 +0000".to_string()),
            identity: GitAuthorIdentity {
                name: Some("Git User".to_string()),
                email: Some("git@example.com".to_string()),
            },
        }),
        author_config: config::AuthorConfig {
            name: Some("Config User".to_string()),
            email: None,
        },
    };

    let mut out = String::new();
    append_git_committer_identity(&mut out, &identity);

    assert!(out.contains("Git AI author config override:"), "{out}");
    assert!(out.contains("  author.name: Config User"), "{out}");
    assert!(out.contains("  author.email: <unset>"), "{out}");
    assert!(out.contains("Git AI effective author identity:"), "{out}");
    assert!(
        out.contains("  Formatted: Config User <git@example.com>"),
        "{out}"
    );
}

#[test]
fn test_command_output_to_result_formats_diagnostics_without_stderr() {
    let err = command_output_to_result(TimedCommandOutput {
        status: Some(1),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        diagnostics: vec!["output collection did not finish".to_string()],
        wait_error: None,
    })
    .unwrap_err();

    assert_eq!(err, "exit code 1: output collection did not finish");
}
