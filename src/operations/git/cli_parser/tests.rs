use super::*;

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
