use super::{parse_git_cli_args, s};

#[test]
fn parses_simple_commit() {
    let args = s(&["-C", "..", "commit", "-m", "foo"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-C", ".."]));
    assert_eq!(got.command, Some("commit".into()));
    assert_eq!(got.command_args, s(&["-m", "foo"]));
}

#[test]
fn long_eq_and_separate_forms() {
    let args = s(&["--git-dir=/x/repo.git", "--work-tree", "/x", "status"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["--git-dir=/x/repo.git", "--work-tree", "/x"])
    );
    assert_eq!(got.command, Some("status".into()));
}

#[test]
fn meta_exec_path_with_value_no_command() {
    let args = s(&["--exec-path", "/usr/libexec/git-core"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["--exec-path", "/usr/libexec/git-core"])
    );
    assert_eq!(got.command, None);
    assert!(got.command_args.is_empty());
}

#[test]
fn end_of_options_forces_command_even_if_dashy() {
    let args = s(&["-C", ".", "--", "--weird"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-C", "."]));
    assert_eq!(got.command, Some("--weird".into()));
    assert!(got.command_args.is_empty());
}

#[test]
fn unknown_top_level_option_means_no_command() {
    let args = s(&["--totally-unknown", "rest"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command, None);
    assert_eq!(got.command_args, s(&["--totally-unknown", "rest"]));
}

#[test]
fn mixed_equals_and_separate_for_long_globals() {
    let args = s(&[
        "--git-dir=/x/.git",
        "--work-tree",
        "/x",
        "--namespace=ns",
        "commit",
        "--amend",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["--git-dir=/x/.git", "--work-tree", "/x", "--namespace=ns"])
    );
    assert_eq!(got.command, Some("commit".into()));
    assert_eq!(got.command_args, s(&["--amend"]));
}

#[test]
fn dash_dash_forces_command_even_if_dashy() {
    let args = s(&["--", "--help"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command, Some("--help".into()));
    assert!(got.global_args.is_empty());
    assert!(got.command_args.is_empty());
}

#[test]
fn end_of_options_then_dashy_non_meta_command() {
    let args = s(&["--", "-notarealcmd", "--arg"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command, Some("-notarealcmd".into()));
    assert_eq!(got.command_args, s(&["--arg"]));
}

#[test]
fn unknown_top_level_option_disables_command_and_passthrough() {
    let args = s(&["--unknown-top", "status", "-s"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command, None);
    assert_eq!(got.command_args, s(&["--unknown-top", "status", "-s"]));
}

#[test]
fn meta_exec_path_equals_form_no_command() {
    let args = s(&["--exec-path=/usr/libexec/git-core"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["--exec-path=/usr/libexec/git-core"]));
    assert_eq!(got.command, None);
    assert!(got.command_args.is_empty());
}

#[test]
fn meta_info_path_without_command() {
    let args = s(&["--info-path"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command, None);
    assert_eq!(got.command_args, s(&["--info-path"]));
}

#[test]
fn meta_html_path_then_real_command_meta_is_dropped_current_behavior() {
    let args = s(&["--html-path", "log", "-1"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, Vec::<String>::new());
    assert_eq!(got.command, Some("log".into()));
    assert_eq!(got.command_args, s(&["-1"]));
}

#[test]
fn no_args_at_all() {
    let args: Vec<String> = vec![];
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command, None);
    assert!(got.command_args.is_empty());
}

#[test]
fn command_with_hyphen_in_name() {
    let args = s(&["ls-files", "--stage"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command, Some("ls-files".into()));
    assert_eq!(got.command_args, s(&["--stage"]));
}

#[test]
fn unknown_then_everything_passthrough_even_if_command_like_token_exists() {
    let args = s(&["--mystery", "commit", "-m", "x"]);
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command, None);
    assert_eq!(got.command_args, s(&["--mystery", "commit", "-m", "x"]));
}

#[test]
fn exec_path_without_value_no_command() {
    let args = s(&["--exec-path"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["--exec-path"]));
    assert_eq!(got.command, None);
    assert!(got.command_args.is_empty());
}

#[test]
fn blame_double_dash_then_filename() {
    let args = vec!["blame", "--", "Readme.md"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let got = parse_git_cli_args(&args);
    assert!(got.global_args.is_empty());
    assert_eq!(got.command.as_deref(), Some("blame"));
    assert_eq!(
        got.command_args,
        vec!["--".to_string(), "Readme.md".to_string()]
    );
    assert!(!got.saw_end_of_opts);

    assert_eq!(got.to_invocation_vec(), args);
}

#[test]
fn blame_filename_starts_with_dash() {
    let args = vec!["blame", "--", "--weird"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let got = parse_git_cli_args(&args);
    assert_eq!(got.command.as_deref(), Some("blame"));
    assert_eq!(
        got.command_args,
        vec!["--".to_string(), "--weird".to_string()]
    );
    assert!(!got.saw_end_of_opts);
}

#[test]
fn exec_path_then_command_is_global() {
    let args = ["--exec-path=foo", "under_score"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, vec!["--exec-path=foo".to_string()]);
    assert_eq!(got.command.as_deref(), Some("under_score"));
    assert!(got.command_args.is_empty());
    assert_eq!(got.to_invocation_vec(), args);
    assert!(!got.is_help);
}

#[test]
fn global_option_then_command_with_verbose() {
    // `git -C /tmp remote -v` should parse remote as command with -v as its arg
    let args = s(&["-C", "/tmp", "remote", "-v"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-C", "/tmp"]));
    assert_eq!(got.command.as_deref(), Some("remote"));
    assert_eq!(got.command_args, s(&["-v"]));
}

#[test]
fn multiple_global_options_then_command_with_verbose() {
    // `git -c foo=bar -C /tmp remote -v`
    let args = s(&["-c", "foo=bar", "-C", "/tmp", "remote", "-v"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-c", "foo=bar", "-C", "/tmp"]));
    assert_eq!(got.command.as_deref(), Some("remote"));
    assert_eq!(got.command_args, s(&["-v"]));
}
