use super::{parse_git_cli_args, s};

#[test]
fn meta_version_no_command() {
    let args = s(&["--version"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command, Some("version".into()));
    assert!(got.command_args.is_empty());
}

#[test]
fn meta_version_no_command_even_with_extra_flags() {
    let args = s(&["--version", "-v"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command, Some("version".into()));
    assert_eq!(got.command_args, s(&["-v"]));
}

#[test]
fn meta_help_no_command() {
    let args = s(&["--help"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command, Some("help".into()));
    assert!(got.command_args.is_empty());
}

#[test]
fn precommand_help_rewrites_to_help_command() {
    let args = s(&["--help", "commit", "-a"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command.as_deref(), Some("help"));
    // `commit` becomes first arg to `help`; keep trailing tokens for `git help` viewer (e.g., -w).
    assert_eq!(got.command_args, s(&["commit", "-a"]));
    assert!(got.is_help);
}

#[test]
fn postcommand_help_does_not_rewrite_even_for_known_cmd() {
    let args = s(&["commit", "--help"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("commit"));
    assert_eq!(got.command_args, s(&["--help"]));
    assert!(got.is_help);
}

#[test]
fn top_level_short_h_is_alias_for_help() {
    let args = s(&["-h", "status"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("help"));
    assert_eq!(got.command_args, s(&["status"]));
    assert!(got.is_help);
}

#[test]
fn help_precedes_version_when_both_given() {
    let args = s(&["-v", "--help"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("help"));
    assert!(got.command_args.is_empty());
    assert!(got.is_help);
}

#[test]
fn version_rewrites_when_no_command() {
    let args = s(&["--version", "--build-options"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("version"));
    assert_eq!(got.command_args, s(&["--build-options"]));
    assert!(!got.is_help);
}

#[test]
fn version_rewrites_even_if_a_command_token_follows() {
    let args = s(&["--version", "commit"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command.as_deref(), Some("version"));
    assert!(got.command_args.is_empty()); // drop stray "commit"
    assert!(!got.is_help);
}

#[test]
fn version_keeps_build_options_and_drops_command_token() {
    let args = s(&["--version", "--build-options", "commit"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("version"));
    assert_eq!(got.command_args, s(&["--build-options"]));
    assert!(!got.is_help);
}

#[test]
fn short_v_behaves_like_version_even_with_command_token() {
    let args = s(&["-v", "status"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("version"));
    assert!(got.command_args.is_empty());
    assert!(!got.is_help);
}

#[test]
fn help_still_precedes_version_when_both_present_with_command() {
    let args = s(&["--version", "--help", "commit"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("help")); // help wins
    assert_eq!(got.command_args, s(&["commit"])); // show help for commit
    assert!(got.is_help);
}

#[test]
fn help_precedes_version_no_command_case_too() {
    let args = s(&["-v", "-h"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("help"));
    assert!(got.command_args.is_empty());
    assert!(got.is_help);
}

#[test]
fn end_of_opts_prevents_help_rewrite() {
    let args = s(&["--", "--help"]);
    let got = parse_git_cli_args(&args);
    assert!(got.saw_end_of_opts);
    // command is literally "--help"; do NOT rewrite
    assert_eq!(got.command.as_deref(), Some("--help"));
    assert!(got.command_args.is_empty());
    assert!(got.is_help);
}

#[test]
fn help_rewrites_only_when_precommand() {
    // `git --help revisions` -> `git help revisions`
    let args = s(&["--help", "revisions"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("help"));
    assert_eq!(got.command_args, s(&["revisions"]));
    assert!(got.is_help);
}

#[test]
fn guides_topic_postcommand_must_fail_case() {
    // `git revisions --help` must NOT rewrite
    let args = s(&["revisions", "--help"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("revisions"));
    assert_eq!(got.command_args, s(&["--help"]));
    assert!(got.is_help);
}

#[test]
fn commit_short_h_is_not_rewritten() {
    let args = s(&["commit", "-h"]);
    let got = parse_git_cli_args(&args);
    // -h belongs to the subcommand; do not rewrite to `git help commit`
    assert_eq!(got.command.as_deref(), Some("commit"));
    assert_eq!(got.command_args, s(&["-h"]));
    assert!(got.is_help);
}

#[test]
fn command_help_is_a_real_command() {
    let args = s(&["help", "-a"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command, Some("help".into()));
    assert_eq!(got.command_args, s(&["-a"]));
    assert!(got.is_help);
}

#[test]
fn unknown_top_level_blocks_help_rewrite() {
    let args = s(&["--bogus", "--help"]);
    let got = parse_git_cli_args(&args /* , is_known_cmd if you added it */);
    assert_eq!(got.command, None);
    assert_eq!(got.command_args, s(&["--bogus", "--help"])); // no rewrite to `help`
    assert!(got.is_help);
}

#[test]
fn unknown_top_level_blocks_version_rewrite() {
    let args = s(&["--bogus", "--version"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command, None);
    assert_eq!(got.command_args, s(&["--bogus", "--version"])); // no rewrite to `version`
    assert!(!got.is_help);
}

// =============================================================================
// Regression tests: subcommand -v flags must NOT be treated as version requests
// =============================================================================
// These tests ensure that `-v` appearing AFTER a subcommand is passed through
// as a command argument, not interpreted as a global version flag.
// Regression test for: `git remote -v` incorrectly showing version info.

#[test]
fn remote_verbose_flag_not_treated_as_version() {
    let args = s(&["remote", "-v"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("remote"));
    assert_eq!(got.command_args, s(&["-v"]));
    // Critically: command should NOT be "version"
    assert_ne!(got.command.as_deref(), Some("version"));
}

#[test]
fn remote_verbose_long_flag_not_treated_as_version() {
    let args = s(&["remote", "--verbose"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("remote"));
    assert_eq!(got.command_args, s(&["--verbose"]));
}

#[test]
fn diff_verbose_flag_not_treated_as_version() {
    let args = s(&["diff", "-v", "HEAD"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("diff"));
    assert_eq!(got.command_args, s(&["-v", "HEAD"]));
}

#[test]
fn log_verbose_flag_not_treated_as_version() {
    let args = s(&["log", "-v", "--oneline"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("log"));
    assert_eq!(got.command_args, s(&["-v", "--oneline"]));
}

#[test]
fn commit_verbose_flag_not_treated_as_version() {
    // `git commit -v` shows diff in commit message editor
    let args = s(&["commit", "-v"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("commit"));
    assert_eq!(got.command_args, s(&["-v"]));
}

#[test]
fn push_verbose_flag_not_treated_as_version() {
    let args = s(&["push", "-v", "origin", "main"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("push"));
    assert_eq!(got.command_args, s(&["-v", "origin", "main"]));
}

#[test]
fn fetch_verbose_flag_not_treated_as_version() {
    let args = s(&["fetch", "-v", "--all"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("fetch"));
    assert_eq!(got.command_args, s(&["-v", "--all"]));
}

#[test]
fn pull_verbose_flag_not_treated_as_version() {
    let args = s(&["pull", "-v"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("pull"));
    assert_eq!(got.command_args, s(&["-v"]));
}
