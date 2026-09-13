use super::*;

#[test]
fn test_pos_command_basic() {
    // Test: git merge abc --squash
    let args = vec![
        "merge".to_string(),
        "abc".to_string(),
        "--squash".to_string(),
    ];
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.pos_command(0), Some("abc".to_string()));
    assert_eq!(parsed.pos_command(1), None);
}

#[test]
fn test_pos_command_flags_before() {
    // Test: git merge --squash --no-verify abc
    let args = vec![
        "merge".to_string(),
        "--squash".to_string(),
        "--no-verify".to_string(),
        "abc".to_string(),
    ];
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.pos_command(0), Some("abc".to_string()));
    assert_eq!(parsed.pos_command(1), None);
}

#[test]
fn test_pos_command_multiple_positional() {
    // Test: git merge abc def --squash
    let args = vec![
        "merge".to_string(),
        "abc".to_string(),
        "def".to_string(),
        "--squash".to_string(),
    ];
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.pos_command(0), Some("abc".to_string()));
    assert_eq!(parsed.pos_command(1), Some("def".to_string()));
    assert_eq!(parsed.pos_command(2), None);
}

#[test]
fn test_pos_command_with_flag_value() {
    // Test: git commit -m "message" file.txt
    let args = vec![
        "commit".to_string(),
        "-m".to_string(),
        "message".to_string(),
        "file.txt".to_string(),
    ];
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.pos_command(0), Some("file.txt".to_string()));
    assert_eq!(parsed.pos_command(1), None);
}

#[test]
fn test_pos_command_inline_flag_value() {
    // Test: git merge --strategy=recursive abc
    let args = vec![
        "merge".to_string(),
        "--strategy=recursive".to_string(),
        "abc".to_string(),
    ];
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.pos_command(0), Some("abc".to_string()));
}

#[test]
fn test_derive_directory_from_url() {
    assert_eq!(
        derive_directory_from_url("https://github.com/user/repo.git"),
        Some("repo".to_string())
    );
    assert_eq!(
        derive_directory_from_url("https://github.com/user/repo"),
        Some("repo".to_string())
    );
    assert_eq!(
        derive_directory_from_url("git@github.com:user/repo.git"),
        Some("repo".to_string())
    );
    assert_eq!(
        derive_directory_from_url("user@host:path/to/repo.git"),
        Some("repo".to_string())
    );
    assert_eq!(
        derive_directory_from_url("/local/path/repo.git"),
        Some("repo".to_string())
    );
    // Windows backslash paths
    assert_eq!(
        derive_directory_from_url(r"C:\Users\runner\AppData\Local\Temp\repo"),
        Some("repo".to_string())
    );
    assert_eq!(
        derive_directory_from_url(r"C:\Users\runner\AppData\Local\Temp\repo.git"),
        Some("repo".to_string())
    );
    assert_eq!(
        derive_directory_from_url(r"\\?\C:\Temp\bare-repo"),
        Some("bare-repo".to_string())
    );
    // Trailing backslash
    assert_eq!(
        derive_directory_from_url("C:\\Users\\user\\repos\\repo.git\\"),
        Some("repo".to_string())
    );
}

#[test]
fn test_extract_clone_target_directory() {
    // Explicit directory specified
    let args = vec![
        "https://github.com/user/repo.git".to_string(),
        "my-dir".to_string(),
    ];
    assert_eq!(
        extract_clone_target_directory(&args),
        Some("my-dir".to_string())
    );

    // Directory derived from URL
    let args = vec!["https://github.com/user/repo.git".to_string()];
    assert_eq!(
        extract_clone_target_directory(&args),
        Some("repo".to_string())
    );

    // With options
    let args = vec![
        "-b".to_string(),
        "main".to_string(),
        "https://github.com/user/repo.git".to_string(),
    ];
    assert_eq!(
        extract_clone_target_directory(&args),
        Some("repo".to_string())
    );

    // With options and explicit directory
    let args = vec![
        "-b".to_string(),
        "main".to_string(),
        "https://github.com/user/repo.git".to_string(),
        "my-dir".to_string(),
    ];
    assert_eq!(
        extract_clone_target_directory(&args),
        Some("my-dir".to_string())
    );

    // With --option=value syntax
    let args = vec![
        "--branch=main".to_string(),
        "https://github.com/user/repo.git".to_string(),
        "my-dir".to_string(),
    ];
    assert_eq!(
        extract_clone_target_directory(&args),
        Some("my-dir".to_string())
    );
}

#[test]
fn test_explicit_rebase_branch_arg_standard_mode() {
    let args = vec![
        "--rebase-merges".to_string(),
        "main".to_string(),
        "feature".to_string(),
    ];
    assert_eq!(
        explicit_rebase_branch_arg(&args),
        Some("feature".to_string())
    );
}

#[test]
fn test_explicit_rebase_branch_arg_root_mode() {
    let args = vec![
        "--root".to_string(),
        "--onto".to_string(),
        "main".to_string(),
        "feature".to_string(),
    ];
    assert_eq!(
        explicit_rebase_branch_arg(&args),
        Some("feature".to_string())
    );
}

#[test]
fn test_explicit_rebase_branch_arg_control_mode_returns_none() {
    let args = vec!["--continue".to_string()];
    assert_eq!(explicit_rebase_branch_arg(&args), None);
    assert!(rebase_has_control_mode(&args));
}

#[test]
fn test_rebase_summary_treats_show_current_patch_as_control_mode() {
    let args = vec!["--show-current-patch".to_string()];
    let summary = summarize_rebase_args(&args);
    assert!(summary.is_control_mode);
    assert!(summary.positionals.is_empty());
}

#[test]
fn test_explicit_rebase_branch_arg_skips_exec_and_empty_values() {
    let args = vec![
        "--exec".to_string(),
        "printf hi".to_string(),
        "--empty".to_string(),
        "keep".to_string(),
        "main".to_string(),
        "feature".to_string(),
    ];
    assert_eq!(
        explicit_rebase_branch_arg(&args),
        Some("feature".to_string())
    );
}

#[test]
fn test_rebase_summary_tracks_onto_with_c_path() {
    let args = vec![
        "-C".to_string(),
        "1".to_string(),
        "--onto".to_string(),
        "new-base".to_string(),
        "upstream".to_string(),
        "feature".to_string(),
    ];
    let summary = summarize_rebase_args(&args);
    assert!(!summary.is_control_mode);
    assert_eq!(summary.onto_spec.as_deref(), Some("new-base"));
    assert_eq!(summary.positionals, vec!["upstream", "feature"]);
}

#[test]
fn test_rebase_summary_continue_is_control_mode() {
    let summary = summarize_rebase_args(&["--continue".to_string()]);
    assert!(summary.is_control_mode);
}

#[test]
fn test_rebase_summary_abort_is_control_mode() {
    let summary = summarize_rebase_args(&["--abort".to_string()]);
    assert!(summary.is_control_mode);
}

#[test]
fn test_rebase_summary_skip_is_control_mode() {
    let summary = summarize_rebase_args(&["--skip".to_string()]);
    assert!(summary.is_control_mode);
}

#[test]
fn test_rebase_summary_upstream_only() {
    let summary = summarize_rebase_args(&["origin/main".to_string()]);
    assert!(!summary.is_control_mode);
    assert_eq!(summary.positionals, vec!["origin/main"]);
}

#[test]
fn test_rebase_summary_onto_equals_form() {
    let summary = summarize_rebase_args(&["--onto=abc123".to_string(), "origin/main".to_string()]);
    assert!(!summary.is_control_mode);
    assert_eq!(summary.onto_spec.as_deref(), Some("abc123"));
}

#[test]
fn test_rebase_summary_root_flag() {
    let summary = summarize_rebase_args(&["--root".to_string()]);
    assert!(!summary.is_control_mode);
    assert!(summary.has_root);
}

#[test]
fn test_rebase_summary_interactive_with_upstream() {
    let summary = summarize_rebase_args(&["-i".to_string(), "origin/main".to_string()]);
    assert!(!summary.is_control_mode);
    assert_eq!(summary.positionals, vec!["origin/main"]);
}

#[test]
fn test_rebase_summary_strategy_consumes_value() {
    let summary = summarize_rebase_args(&[
        "-s".to_string(),
        "ours".to_string(),
        "origin/main".to_string(),
    ]);
    assert!(!summary.is_control_mode);
    assert_eq!(summary.positionals, vec!["origin/main"]);
}
